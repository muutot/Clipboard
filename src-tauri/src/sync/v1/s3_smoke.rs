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
        // The scope identity must match the app's own derivation
        // (`commands/sync/mod.rs`), otherwise an object written here would be
        // invisible to the engine and vice versa.
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
