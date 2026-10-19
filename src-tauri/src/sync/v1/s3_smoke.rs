//! Opt-in real-S3 correctness smoke against a disposable S3-compatible server
//! (rustfs or MinIO).
//!
//! `MemoryStore` covers engine semantics; it cannot prove that a real server
//! accepts our SigV4 signing, path-style addressing, conditional writes, ETag
//! quoting, or ListObjectsV2 continuation tokens. This module closes that gap
//! with a small, fast run (a handful of items, no scale workload) so the
//! transport contract can be re-verified whenever the engine changes.
//!
//! The whole module skips unless `CLIPBOARD_S3_TEST_ENDPOINT` is set, so a
//! plain `cargo nextest run` never fails for lack of a server. Drive it with
//! `scripts/sync-s3-test.ps1`, which provisions a pinned rustfs on Windows.
//!
//! Isolation contract (identical to `scale_bench`): the bucket is shared and
//! idempotently created, every run writes under a random object prefix, and
//! cleanup deletes only that prefix. Never point these tests at a bucket that
//! holds user data.

use std::fs;

use sha2::{Digest, Sha256};

use super::*;
use crate::{
    domain::ClipboardItem,
    storage::{ClipboardRepository, Database, StoragePaths},
    sync::s3,
};

/// Largest body the transport is allowed to buffer for an in-memory read.
///
/// Peer segments and pointer objects are metadata-sized by protocol design, so
/// the in-memory `ObjectStore::get` path must reject anything above this
/// instead of trusting a remote-declared size. A real oversized object is
/// published to prove the bound is enforced end to end.
const SEGMENT_IN_MEMORY_BUDGET_BYTES: u64 = s3::MAX_IN_MEMORY_PROTOCOL_OBJECT_BYTES;

#[derive(Clone)]
struct SmokeConfig {
    endpoint: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    password: Option<String>,
}

/// `None` when the environment does not opt in, which keeps these tests out of
/// an ordinary test run instead of failing it.
fn smoke_config() -> Option<SmokeConfig> {
    let endpoint = std::env::var("CLIPBOARD_S3_TEST_ENDPOINT")
        .ok()
        .filter(|value| !value.trim().is_empty())?;
    Some(SmokeConfig {
        endpoint,
        region: std::env::var("CLIPBOARD_S3_TEST_REGION")
            .unwrap_or_else(|_| "us-east-1".to_string()),
        bucket: std::env::var("CLIPBOARD_S3_TEST_BUCKET")
            .unwrap_or_else(|_| "clipboard-sync-smoke".to_string()),
        access_key: std::env::var("CLIPBOARD_S3_TEST_ACCESS_KEY")
            .expect("CLIPBOARD_S3_TEST_ACCESS_KEY must be set with CLIPBOARD_S3_TEST_ENDPOINT"),
        secret_key: std::env::var("CLIPBOARD_S3_TEST_SECRET_KEY")
            .expect("CLIPBOARD_S3_TEST_SECRET_KEY must be set with CLIPBOARD_S3_TEST_ENDPOINT"),
        password: std::env::var("CLIPBOARD_S3_TEST_PASSWORD")
            .ok()
            .filter(|value| !value.is_empty()),
    })
}

macro_rules! require_config {
    () => {
        match smoke_config() {
            Some(config) => config,
            None => {
                eprintln!(
                    "[sync-smoke] skipped: set CLIPBOARD_S3_TEST_ENDPOINT (see \
                     scripts/sync-s3-test.ps1) to run the real-S3 smoke"
                );
                return;
            }
        }
    };
}

/// One isolated remote namespace plus the credentials needed to reach it.
struct SmokeScope {
    config: SmokeConfig,
    remote_prefix: String,
    remote_scope: String,
    cleaned: bool,
}

impl SmokeScope {
    fn new(config: &SmokeConfig, purpose: &str) -> Self {
        let remote_prefix = format!("clipboard-sync-smoke/{}/{}", uuid::Uuid::new_v4(), purpose);
        // Retain a legacy credential-derived scope for frozen engine fixtures;
        // the desktop now resolves a namespace binding before entering the engine.
        let endpoint = config.endpoint.trim().trim_end_matches('/');
        let identity = format!(
            "clipboard-sync-v1\n{endpoint}\n{}\n{}\n{}\n{}",
            config.region, config.bucket, remote_prefix, config.access_key
        );
        let remote_scope = hex::encode(Sha256::digest(identity.as_bytes()));
        Self {
            config: config.clone(),
            remote_prefix,
            remote_scope,
            cleaned: false,
        }
    }

    /// Bucket-relative key, i.e. what `ObjectStore` addresses.
    fn object_key(&self, suffix: &str) -> String {
        format!("{}/{}", self.remote_prefix, suffix)
    }

    fn store(&self, metrics: Option<S3RequestMetrics>) -> S3ObjectStore {
        let store = S3ObjectStore::new(
            self.config.endpoint.clone(),
            self.config.region.clone(),
            self.config.bucket.clone(),
            self.config.access_key.clone(),
            self.config.secret_key.clone(),
            &self.remote_prefix,
        )
        .expect("valid S3 smoke configuration");
        match metrics {
            Some(metrics) => store.with_metrics(metrics),
            None => store,
        }
    }

    fn session_key(&self) -> Option<SessionKey> {
        self.config
            .password
            .as_deref()
            .map(|password| SessionKey::derive(password, &self.remote_scope).unwrap())
    }

    fn ensure_bucket(&self) {
        s3::ensure_test_bucket(
            &self.config.endpoint,
            &self.config.region,
            &self.config.bucket,
            &self.config.access_key,
            &self.config.secret_key,
        )
        .expect("S3 smoke bucket must be creatable");
    }

    fn cleanup(&mut self) -> Result<(), String> {
        if self.cleaned {
            return Ok(());
        }
        let store = self.store(None);
        for object in store.list("", None)? {
            store.delete(&object.key)?;
        }
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for SmokeScope {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!(
                "[sync-smoke] failed to clean isolated prefix {:?}: {error}",
                self.remote_prefix
            );
        }
    }
}

#[test]
#[ignore = "requires an explicitly configured disposable S3-compatible server"]
fn namespace_binding_is_durable_and_cannot_be_overwritten() {
    use clipboard_sync::v1::namespace::{resolve_namespace, NAMESPACE_KEY};
    let config = require_config!();
    let scope = SmokeScope::new(&config, "namespace-binding");
    scope.ensure_bucket();
    let store = scope.store(None);
    let stable = hex::encode(Sha256::digest(scope.remote_prefix.as_bytes()));
    let (binding, _) = resolve_namespace(
        &store,
        &stable,
        &scope.remote_scope,
        config.password.as_deref(),
        None,
    )
    .unwrap();
    assert_eq!(binding.scope, stable);
    let (second, _) = resolve_namespace(
        &store,
        &stable,
        &"d".repeat(64),
        config.password.as_deref(),
        None,
    )
    .unwrap();
    assert_eq!(second, binding);
    assert_eq!(
        store
            .put(
                NAMESPACE_KEY,
                b"must not replace".to_vec(),
                PutCondition::IfAbsent
            )
            .unwrap(),
        PutOutcome::PreconditionFailed
    );
    assert_eq!(
        store.get(NAMESPACE_KEY).unwrap().unwrap().bytes,
        binding.to_bytes()
    );
}

/// A disposable on-disk database, mirroring the engine test helper.
struct SmokeDevice {
    paths: StoragePaths,
    database: Option<Database>,
}

impl SmokeDevice {
    fn new(label: &str) -> Self {
        let project = std::env::temp_dir().join(format!(
            "clipboard-s3-smoke-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let paths = StoragePaths::initialize(project).expect("temporary storage paths");
        let database = Database::open(&paths.database).expect("temporary database");
        Self {
            paths,
            database: Some(database),
        }
    }

    fn database(&self) -> &Database {
        self.database.as_ref().expect("device database")
    }

    fn engine_paths(&self) -> SyncEnginePaths {
        (&self.paths).into()
    }

    fn save_text(&self, id: &str, text: &str) {
        self.database()
            .save_item(&text_item(id, text))
            .expect("save smoke item");
    }

    fn save_image(&self, id: &str, bytes: &[u8]) {
        let image = self.paths.images.join(format!("{id}.png"));
        fs::write(&image, bytes).expect("write smoke image");
        self.database()
            .save_item(&image_item(id, &image))
            .expect("save smoke image item");
    }
}

impl Drop for SmokeDevice {
    fn drop(&mut self) {
        // Release SQLite (and its WAL/SHM handles) before removing the tree;
        // Windows will not delete an open file.
        drop(self.database.take());
        let _ = fs::remove_dir_all(&self.paths.project);
    }
}

fn text_item(id: &str, text: &str) -> ClipboardItem {
    ClipboardItem {
        id: id.to_string(),
        kind: crate::domain::ClipboardKind::Text,
        title: format!("{id} title"),
        text_content: Some(text.to_string()),
        html_content: None,
        rtf_content: None,
        resource_path: None,
        preview_path: None,
        content_hash: format!("hash-{id}"),
        source_app: Some("sync-smoke".to_string()),
        icon_path: None,
        size_bytes: text.len() as u64,
        created_at_ms: 1_700_000_000_000 + text.len() as i64,
        last_used_at_ms: None,
        is_favorite: false,
        metadata_json: Some("{}".to_string()),
    }
}

fn options() -> SyncEngineOptions {
    SyncEngineOptions {
        segment_max_entries: 100,
        resource_limits: ResourceLimits {
            image_bytes: 1024 * 1024,
            file_bytes: 1024 * 1024,
            icon_bytes: 1024 * 1024,
        },
        resource_gc_mode: ResourceGcMode::Off,
    }
}

/// Larger resource limits so the encrypted-resource smoke can publish a body
/// spanning several of the fixed 1 MiB AES-GCM chunks instead of one chunk.
fn resource_options() -> SyncEngineOptions {
    SyncEngineOptions {
        segment_max_entries: 100,
        resource_limits: ResourceLimits {
            image_bytes: 8 * 1024 * 1024,
            file_bytes: 8 * 1024 * 1024,
            icon_bytes: 1024 * 1024,
        },
        resource_gc_mode: ResourceGcMode::Off,
    }
}

fn image_item(id: &str, path: &std::path::Path) -> ClipboardItem {
    let path = path.to_string_lossy().to_string();
    ClipboardItem {
        id: id.to_string(),
        kind: crate::domain::ClipboardKind::Image,
        title: id.to_string(),
        text_content: None,
        html_content: None,
        rtf_content: None,
        resource_path: Some(path.clone()),
        preview_path: Some(path.clone()),
        content_hash: format!("hash-{id}"),
        source_app: Some("sync-smoke".to_string()),
        icon_path: None,
        size_bytes: fs::metadata(path.as_str()).unwrap().len(),
        created_at_ms: 1_700_000_000_000,
        last_used_at_ms: None,
        is_favorite: false,
        metadata_json: Some(
            serde_json::json!({
                "resourcePath": path,
                "storagePath": path,
                "previewPath": path,
            })
            .to_string(),
        ),
    }
}

fn run_sync(store: &S3ObjectStore, device: &SmokeDevice, scope: &SmokeScope) -> SyncEngineResult {
    sync_database(
        store,
        device.database(),
        &device.engine_paths(),
        &scope.remote_scope,
        scope.session_key().as_ref(),
        options(),
    )
    .expect("real-S3 sync run must succeed")
}

/// All I/O reaches the real server. Interrupt only after an immutable PUT has
/// completed, exercising the orphan/publication boundary deterministically.
struct CancelAfterPut<'a> {
    store: &'a S3ObjectStore,
    fragment: &'a str,
    token: clipboard_sync::cancellation::CancellationToken,
}

impl CancelAfterPut<'_> {
    fn interrupt(&self, key: &str, outcome: &PutOutcome) {
        if key.contains(self.fragment) && matches!(outcome, PutOutcome::Stored { .. }) {
            self.token.cancel();
        }
    }
}

impl ObjectStore for CancelAfterPut<'_> {
    fn list(&self, prefix: &str, after: Option<&str>) -> Result<Vec<ObjectInfo>, String> {
        self.store.list(prefix, after)
    }
    fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
        self.store.get(key)
    }
    fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String> {
        self.store.head(key)
    }
    fn get_to_file(
        &self,
        key: &str,
        path: &std::path::Path,
        max: u64,
    ) -> Result<Option<DownloadedFile>, String> {
        self.store.get_to_file(key, path, max)
    }
    fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        condition: PutCondition,
    ) -> Result<PutOutcome, String> {
        let outcome = self.store.put(key, bytes, condition)?;
        self.interrupt(key, &outcome);
        Ok(outcome)
    }
    fn put_file(
        &self,
        key: &str,
        path: &std::path::Path,
        sha256: &str,
        size: u64,
        condition: PutCondition,
    ) -> Result<PutOutcome, String> {
        let outcome = self.store.put_file(key, path, sha256, size, condition)?;
        self.interrupt(key, &outcome);
        Ok(outcome)
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.store.delete(key)
    }
}

#[test]
#[ignore = "requires a disposable S3-compatible server; run scripts/sync-s3-test.ps1"]
fn real_s3_cancelled_publications_retry_without_losing_pending_records() {
    use clipboard_sync::cancellation::CancellationToken;
    let config = require_config!();
    let scope = SmokeScope::new(&config, "cancel-publication");
    scope.ensure_bucket();
    let store = scope.store(None);
    let source = SmokeDevice::new("cancel-source");
    let target = SmokeDevice::new("cancel-target");
    let root = CancellationToken::default();

    for (index, fragment) in ["/snapshots/", "/segments/"].into_iter().enumerate() {
        source.save_text(
            &format!("cancel-{index}"),
            &format!("synthetic record {index}"),
        );
        let head_key = head_object_key(&source.database().get_sync_device_id().unwrap()).unwrap();
        let previous_head = store.get(&head_key).unwrap();
        let token = root.child_token();
        let interrupted = CancelAfterPut {
            store: &store,
            fragment,
            token: token.clone(),
        };
        let outcome = sync_database_cancellable(
            &interrupted,
            source.database(),
            &source.engine_paths(),
            &scope.remote_scope,
            scope.session_key().as_ref(),
            options(),
            &token,
        );
        assert_eq!(outcome.unwrap_err(), "sync cancelled");
        assert!(
            token.is_cancelled(),
            "the real immutable PUT must trigger cancellation"
        );
        assert!(!root.is_cancelled());
        assert_eq!(
            store.get(&head_key).unwrap(),
            previous_head,
            "unpublished work must not advance the head"
        );
        assert!(store
            .list("", None)
            .unwrap()
            .iter()
            .any(|object| object.key.contains(fragment)));
        if index == 0 {
            assert!(
                !source
                    .database()
                    .get_or_create_sync_remote_state(&scope.remote_scope)
                    .unwrap()
                    .initialized
            );
        } else {
            assert!(source
                .database()
                .get_sync_outbox_batch_for_scope(&scope.remote_scope, 100)
                .unwrap()
                .is_some());
        }
        run_sync(&store, &source, &scope);
        assert!(source
            .database()
            .get_sync_outbox_batch_for_scope(&scope.remote_scope, 100)
            .unwrap()
            .is_none());
        run_sync(&scope.store(None), &target, &scope);
        assert_eq!(target.database().item_count().unwrap(), (index + 1) as u64);
        assert_eq!(
            target
                .database()
                .get_item(&format!("cancel-{index}"))
                .unwrap()
                .unwrap()
                .text_content,
            Some(format!("synthetic record {index}"))
        );
    }
    assert_eq!(run_sync(&store, &source, &scope).uploaded_entries, 0);
    assert_eq!(
        run_sync(&scope.store(None), &target, &scope).applied_entries,
        0
    );
}

#[test]
#[ignore = "requires a disposable S3-compatible server; run scripts/sync-s3-test.ps1"]
fn real_s3_two_device_convergence_over_the_wire() {
    let config = require_config!();
    let scope = SmokeScope::new(&config, "convergence");
    scope.ensure_bucket();
    let metrics = S3RequestMetrics::default();
    let store = scope.store(Some(metrics.clone()));
    let source = SmokeDevice::new("source");
    let target = SmokeDevice::new("target");

    source.save_text("smoke-a", "first record");
    source.save_text("smoke-b", "second record");
    let published = run_sync(&store, &source, &scope);
    assert_eq!(published.applied_entries, 0);
    assert!(published.uploaded_entries >= 2);

    // A second, independent store handle proves the run really went over the
    // wire rather than reading a local cache.
    let pulled = run_sync(&scope.store(None), &target, &scope);
    assert_eq!(pulled.applied_entries, 2);
    assert_eq!(
        target.database().get_item("smoke-a").unwrap().map(|i| i.id),
        Some("smoke-a".to_string())
    );
    assert_eq!(
        target.database().get_item("smoke-b").unwrap().map(|i| i.id),
        Some("smoke-b".to_string())
    );

    // Two consecutive converged runs. The head cache is recorded while a head
    // is published, so the first run after a publication legitimately re-reads
    // the objects it just wrote; the second run is the steady state and must
    // not transfer any object body.
    for pass in 1..=2u8 {
        let idle = S3RequestMetrics::default();
        let idle_store = scope.store(Some(idle.clone()));
        let idle_run = run_sync(&idle_store, &source, &scope);
        assert_eq!(
            idle_run.uploaded_entries, 0,
            "converged run must not publish"
        );
        let snapshot = idle.snapshot();
        eprintln!(
            "[sync-smoke] idle pass {pass}: list={} get={} head={} put={} delete={}",
            snapshot.list_requests,
            snapshot.get_requests,
            snapshot.head_requests,
            snapshot.put_requests,
            snapshot.delete_requests
        );
        if pass == 2 {
            assert!(snapshot.list_requests >= 1, "idle sync must list heads");
            assert_eq!(
                snapshot.get_requests + snapshot.head_requests + snapshot.put_requests,
                0,
                "a converged steady-state sync must not transfer objects, got {snapshot:?}"
            );
        }
    }
}

#[test]
#[ignore = "requires a disposable S3-compatible server; run scripts/sync-s3-test.ps1"]
fn real_s3_conditional_writes_and_etags_match_the_transport_contract() {
    let config = require_config!();
    let scope = SmokeScope::new(&config, "conditional-writes");
    scope.ensure_bucket();
    let store = scope.store(None);
    let key = "conditional-writes/probe";
    let payload = b"conditional write probe".to_vec();

    // The engine's checkpoint CAS depends on all three outcomes being
    // distinguishable by a real server.
    let created = s3::put_s3_object(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &scope.object_key(key),
        payload.clone(),
        &config.access_key,
        &config.secret_key,
        s3::S3PutCondition::IfAbsent,
    )
    .expect("first conditional put must succeed");
    let etag = match created {
        s3::S3PutOutcome::Stored { etag } => etag,
        other => panic!("IfAbsent on a missing key must store, got {other:?}"),
    }
    .expect("a real server must return an ETag for a stored object");

    let repeated = s3::put_s3_object(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &scope.object_key(key),
        payload.clone(),
        &config.access_key,
        &config.secret_key,
        s3::S3PutCondition::IfAbsent,
    )
    .expect("second conditional put must return an answer");
    assert_eq!(
        repeated,
        s3::S3PutOutcome::PreconditionFailed,
        "IfAbsent on an existing key must be refused by the server"
    );

    let stale = s3::put_s3_object(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &scope.object_key(key),
        b"replacement".to_vec(),
        &config.access_key,
        &config.secret_key,
        s3::S3PutCondition::IfMatch("\"0000000000000000000000000000dead\"".to_string()),
    )
    .expect("IfMatch with a stale ETag must return an answer");
    assert_eq!(
        stale,
        s3::S3PutOutcome::PreconditionFailed,
        "IfMatch with a non-current ETag must be refused by the server"
    );

    let replaced = s3::put_s3_object(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &scope.object_key(key),
        b"replacement".to_vec(),
        &config.access_key,
        &config.secret_key,
        s3::S3PutCondition::IfMatch(etag.clone()),
    )
    .expect("IfMatch with the current ETag must be accepted");
    let current_etag = match replaced {
        s3::S3PutOutcome::Stored { etag } => etag,
        other => panic!("IfMatch with the current ETag must store, got {other:?}"),
    }
    .expect("a real server must report the ETag of the replacement");
    assert_ne!(
        current_etag, etag,
        "replacing the payload must change the ETag, otherwise CAS cannot detect staleness"
    );

    // The engine compares listing ETags and GET ETags as raw quoted text, so a
    // server that returned an unquoted value would silently break the head
    // cache. Assert the shape we actually depend on.
    // `ObjectStore` keys are relative to the store's configured prefix.
    let downloaded = store
        .get("conditional-writes/probe")
        .expect("stored object must be readable")
        .expect("stored object must exist");
    assert_eq!(downloaded.bytes, b"replacement");
    assert_eq!(
        downloaded.etag.as_deref(),
        Some(current_etag.as_str()),
        "a GET must return the current ETag text the PUT reported"
    );
    let listed = store
        .list("conditional-writes/", None)
        .expect("prefix listing must succeed");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].etag.as_deref(),
        Some(current_etag.as_str()),
        "a LIST must return the current quoted ETag text the PUT reported"
    );
}

#[test]
#[ignore = "requires a disposable S3-compatible server; run scripts/sync-s3-test.ps1"]
fn real_s3_in_memory_object_read_is_bounded_for_segments() {
    let config = require_config!();
    let scope = SmokeScope::new(&config, "in-memory-budget");
    scope.ensure_bucket();

    // Shape the key like a peer segment: the in-memory `ObjectStore::get` path
    // is what pulls segments, and it is the only S3 read that buffers a whole
    // body without a protocol budget.
    //
    // A conforming publisher must refuse the oversized object locally, so the
    // publisher guard is asserted first and the reader bound is then exercised
    // through the file-streamed upload path, which is a different code path and
    // therefore a fair way to place a hostile object in the bucket.
    let oversized_len = SEGMENT_IN_MEMORY_BUDGET_BYTES + 1024;
    let publish = s3::put_s3_object(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &scope.object_key("v1/segments/oversized-segment"),
        vec![b'x'; oversized_len.try_into().expect("probe size fits usize")],
        &config.access_key,
        &config.secret_key,
        s3::S3PutCondition::IfAbsent,
    );
    let publish_error = match publish {
        Ok(outcome) => panic!(
            "publishing above the {SEGMENT_IN_MEMORY_BUDGET_BYTES}-byte ceiling must fail \
             locally, got {outcome:?}"
        ),
        Err(error) => error,
    };
    assert!(
        publish_error.contains(&SEGMENT_IN_MEMORY_BUDGET_BYTES.to_string()),
        "the publish refusal must name the byte ceiling, got: {publish_error}"
    );

    let staged = std::env::temp_dir().join(format!(
        "clipboard-s3-smoke-oversized-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let staged_body = vec![b'x'; oversized_len.try_into().expect("probe size fits usize")];
    let staged_sha256 = hex::encode(Sha256::digest(&staged_body));
    std::fs::write(&staged, &staged_body).expect("stage the oversized probe body");
    let uploaded = s3::put_s3_file(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &scope.object_key("v1/segments/bypassed-segment"),
        &staged,
        &staged_sha256,
        staged_body.len() as u64,
        &config.access_key,
        &config.secret_key,
        s3::S3PutCondition::IfAbsent,
    );
    drop(staged_body);
    let _ = std::fs::remove_file(&staged);
    uploaded.expect("the file-streamed upload path is not the path under test");

    // `ObjectStore::get` is what `get_verified_object` uses for segments, so it
    // must refuse a body above the segment budget instead of buffering it.
    //
    // Match instead of `expect_err` on purpose: the success branch would
    // `Debug`-print the whole buffered body and drown the failure output.
    let store = scope.store(None);
    match store.get("v1/segments/bypassed-segment") {
        Ok(Some(object)) => panic!(
            "an in-memory segment read above the {SEGMENT_IN_MEMORY_BUDGET_BYTES}-byte budget must \
             fail, but the transport buffered {} bytes",
            object.bytes.len()
        ),
        Ok(None) => panic!("the oversized probe object must exist in the bucket"),
        Err(error) => assert!(
            error.contains(&SEGMENT_IN_MEMORY_BUDGET_BYTES.to_string()) || error.contains("limit"),
            "the refusal must name the byte limit, got: {error}"
        ),
    }

    // A body inside the budget still streams to a file and is unaffected.
    let small_key = scope.object_key("v1/segments/small-segment");
    let small = vec![b'y'; 1024];
    s3::put_s3_object(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &small_key,
        small.clone(),
        &config.access_key,
        &config.secret_key,
        s3::S3PutCondition::IfAbsent,
    )
    .expect("in-budget probe object must be publishable");
    let downloaded = store
        .get("v1/segments/small-segment")
        .expect("in-budget segment read must succeed")
        .expect("in-budget segment must exist");
    assert_eq!(downloaded.bytes.len(), small.len());
}

/// The encrypted binary-resource wire path: chunked AES-GCM upload, an
/// ordinary pull that defers resource bodies, and materialization with
/// plaintext verification.
///
/// The engine tests cover these semantics against `MemoryStore`; only a real
/// server proves it accepts the chunked streaming framing, the per-chunk
/// nonce/AAD derivation, and the keyed digest naming for encrypted resources.
#[test]
#[ignore = "requires a disposable S3-compatible server; run scripts/sync-s3-test.ps1"]
fn real_s3_encrypted_resource_materializes_across_devices() {
    let config = require_config!();
    // Encryption is the point of this test; without a password it would only
    // re-test plaintext transfer the engine tests already cover.
    if config.password.is_none() {
        eprintln!(
            "[sync-smoke] skipped: set CLIPBOARD_S3_TEST_PASSWORD for the encrypted \
             resource wire path"
        );
        return;
    }
    let scope = SmokeScope::new(&config, "encrypted-resource");
    scope.ensure_bucket();
    let store = scope.store(None);
    let source = SmokeDevice::new("resource-source");
    let target = SmokeDevice::new("resource-target");
    let key = scope.session_key().expect("password-derived session key");

    // The repeated marker makes a plaintext leak detectable even when the
    // body compresses well; 3 MiB + 1234 spans four of the fixed 1 MiB
    // AES-GCM chunks, exercising multi-chunk framing over the wire.
    let marker = b"smoke-encrypted-resource-plaintext-marker";
    let plaintext: Vec<u8> = marker
        .iter()
        .copied()
        .cycle()
        .take(3 * 1024 * 1024 + 1234)
        .collect();
    source.save_image("encrypted-image", &plaintext);

    let published = sync_database(
        &store,
        source.database(),
        &source.engine_paths(),
        &scope.remote_scope,
        Some(&key),
        resource_options(),
    )
    .expect("real-S3 resource sync run must succeed");
    assert_eq!(published.applied_entries, 0);
    assert!(published.uploaded_entries >= 1);
    assert!(
        published.uploaded_resources >= 1,
        "the bootstrap must upload the image resource"
    );

    let resource_keys: Vec<String> = store
        .list("v1/resources/image/", None)
        .unwrap()
        .into_iter()
        .map(|object| object.key)
        .collect();
    assert_eq!(
        resource_keys.len(),
        1,
        "one content-addressed image object, got {resource_keys:?}"
    );
    let resource_key = resource_keys[0].clone();
    let stored = store
        .get(&resource_key)
        .unwrap()
        .expect("the resource object must exist");
    assert_ne!(
        stored.bytes, plaintext,
        "the stored object must be ciphertext"
    );
    assert!(
        !stored
            .bytes
            .windows(marker.len())
            .any(|window| window == marker.as_slice()),
        "no plaintext window may appear in the stored object"
    );

    let pulled = sync_database(
        &store,
        target.database(),
        &target.engine_paths(),
        &scope.remote_scope,
        Some(&key),
        resource_options(),
    )
    .expect("real-S3 resource pull must succeed");
    assert_eq!(pulled.applied_entries, 1);
    assert_eq!(
        pulled.downloaded_resources, 0,
        "an ordinary pull must defer resource bodies"
    );
    assert!(target
        .database()
        .get_item("encrypted-image")
        .unwrap()
        .unwrap()
        .resource_path
        .is_none());

    let materialized = materialize_resource(
        &store,
        &resource_key,
        &target.paths.images,
        resource_options().resource_limits.image_bytes,
        Some(&key),
    )
    .expect("materializing the encrypted resource must succeed");
    assert_eq!(fs::read(&materialized.path).unwrap(), plaintext);
    assert_eq!(materialized.transferred_bytes, stored.bytes.len() as u64);
}

/// A peer whose configured prefix carries the rotation tombstone must refuse
/// before namespace resolution; a clean prefix must pass. Proves the tombstone
/// key resolves against a real server at the prefix root, outside `v1/`
/// (`docs/SYNC_V2.md` D2).
#[test]
#[ignore = "requires a disposable S3-compatible server; run scripts/sync-s3-test.ps1"]
fn real_s3_rotation_pointer_blocks_a_stale_peer() {
    use crate::commands::sync::{refuse_if_prefix_rotated, ROTATED_TO_KEY};
    let config = require_config!();
    let scope = SmokeScope::new(&config, "rotation-pointer");
    scope.ensure_bucket();
    let store = scope.store(None);

    // No tombstone yet: the guard passes and the run would enter the engine.
    refuse_if_prefix_rotated(&store).expect("a clean prefix must not refuse");

    let pointer = serde_json::json!({
        "newPrefix": format!("{}/rotated", scope.remote_prefix),
        "rotatedAtMs": 1_700_000_000_000i64,
        "deviceId": "rotating-device",
    })
    .to_string();
    s3::put_s3_object(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &scope.object_key(ROTATED_TO_KEY),
        pointer.into_bytes(),
        &config.access_key,
        &config.secret_key,
        s3::S3PutCondition::IfAbsent,
    )
    .expect("the tombstone must be publishable at the prefix root");

    let error = refuse_if_prefix_rotated(&store).unwrap_err();
    assert!(error.contains("rotating-device"), "{error}");
    assert!(error.contains("/rotated"), "{error}");
}

/// The full rotation flow over the wire: a peer device that only holds
/// metadata (its resource body was never materialized) rotates the scope to a
/// fresh prefix with a new password. Proves the materialize-all step, the
/// encrypted republish, the tombstone contents, the old-namespace deletion and
/// the byte-identical local body — all against a real server.
#[test]
#[ignore = "requires a disposable S3-compatible server; run scripts/sync-s3-test.ps1"]
fn real_s3_namespace_rotation_moves_state_and_tombstones() {
    use crate::commands::sync::{perform_namespace_rotation, SyncSettings};
    use crate::config::{SyncConfig, SyncProvider};
    use clipboard_sync::cancellation::CancellationToken;

    let config = require_config!();
    let scope = SmokeScope::new(&config, "namespace-rotation");
    scope.ensure_bucket();
    let store = scope.store(None);

    fn sync_config_for(remote_path: &str, password: Option<&str>) -> SyncConfig {
        SyncConfig {
            provider: SyncProvider::S3,
            endpoint: Some(std::env::var("CLIPBOARD_S3_TEST_ENDPOINT").unwrap()),
            remote_path: Some(remote_path.to_string()),
            s3_region: Some("us-east-1".to_string()),
            s3_bucket: Some("clipboard".to_string()),
            s3_access_key: Some("access".to_string()),
            s3_secret_key: Some("secret".to_string()),
            sync_password: password.map(str::to_string),
            ..SyncConfig::default()
        }
    }

    // A device that captured the history seeds the old namespace.
    let source = SmokeDevice::new("rotate-wire-source");
    let image_bytes = b"rotating wire image".repeat(512);
    source.save_image("rotate-image", &image_bytes);

    let old_settings = SyncSettings::validated_from_sync_config(&sync_config_for(
        &scope.remote_prefix,
        config.password.as_deref(),
    ))
    .unwrap();
    let (binding, key) = clipboard_sync::v1::namespace::resolve_namespace(
        &store,
        &old_settings.remote_scope_id(),
        &old_settings.legacy_scope_id(),
        config.password.as_deref(),
        None,
    )
    .unwrap();
    clipboard_sync::v1::engine::sync_database(
        &store,
        source.database(),
        &source.engine_paths(),
        &binding.scope,
        key.as_ref(),
        old_settings.engine_options(),
    )
    .unwrap();

    // The rotating device pulled the metadata but never the body.
    let target = SmokeDevice::new("rotate-wire-target");
    clipboard_sync::v1::engine::sync_database(
        &store,
        target.database(),
        &target.engine_paths(),
        &binding.scope,
        key.as_ref(),
        old_settings.engine_options(),
    )
    .unwrap();
    assert!(target
        .database()
        .get_item("rotate-image")
        .unwrap()
        .unwrap()
        .resource_path
        .is_none());

    let new_password = "brand-new-rotation-password";
    let new_prefix = format!("{}-rotated-1700000000000", scope.remote_prefix);
    let new_store = S3ObjectStore::new(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &config.access_key,
        &config.secret_key,
        &new_prefix,
    )
    .unwrap();
    let new_settings =
        SyncSettings::validated_from_sync_config(&sync_config_for(&new_prefix, Some(new_password)))
            .unwrap();
    let token = CancellationToken::default();
    let mut config_committed = false;
    let outcome = perform_namespace_rotation(
        &store,
        &new_store,
        target.database(),
        &target.paths,
        &old_settings,
        &new_settings,
        Some(new_password),
        &token,
        &mut || {
            config_committed = true;
            Ok(())
        },
        &mut |_| {},
    )
    .unwrap();
    assert!(
        config_committed,
        "the local configuration must move before the old namespace is deleted"
    );

    assert_eq!(outcome.new_prefix, new_prefix);
    assert!(outcome.materialized_items >= 1);
    assert!(outcome.uploaded_resources >= 1);
    assert!(outcome.deleted_objects >= 3);

    // Old prefix: only the tombstone survives, and it carries the new prefix,
    // the key check peers verify a typed password against, and the device id.
    let old_keys: Vec<String> = store
        .list("", None)
        .unwrap()
        .into_iter()
        .map(|object| object.key)
        .collect();
    assert_eq!(old_keys, vec!["rotated-to".to_string()]);
    let tombstone: serde_json::Value =
        serde_json::from_slice(&store.get("rotated-to").unwrap().unwrap().bytes).unwrap();
    assert_eq!(tombstone["newPrefix"], serde_json::json!(new_prefix));
    assert!(tombstone["keyCheck"].is_string());
    assert!(tombstone["deviceId"].is_string());

    // New prefix: a complete namespace, including the resource body encrypted
    // under the new keyed digest.
    let new_keys: Vec<String> = new_store
        .list("", None)
        .unwrap()
        .into_iter()
        .map(|object| object.key)
        .collect();
    assert!(new_keys.iter().any(|key| key == "v1/namespace.json"));
    assert!(new_keys.iter().any(|key| key.starts_with("v1/heads/")));
    assert!(new_keys
        .iter()
        .any(|key| key.starts_with("v1/resources/image/")));

    // The rotating device now holds the body locally, byte-identical.
    let rotated = target.database().get_item("rotate-image").unwrap().unwrap();
    assert_eq!(
        fs::read(rotated.resource_path.as_deref().unwrap()).unwrap(),
        image_bytes
    );

    // A second rotation on the same old prefix refuses at the tombstone.
    let error = perform_namespace_rotation(
        &store,
        &new_store,
        target.database(),
        &target.paths,
        &old_settings,
        &new_settings,
        Some(new_password),
        &token,
        &mut || Ok(()),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(error.contains("rotated"), "{error}");

    drop(source);
    drop(target);
}
