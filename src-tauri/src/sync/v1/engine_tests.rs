use std::{collections::BTreeMap, fs, io::Write, path::Path, sync::Mutex};

use sha2::{Digest, Sha256};

use crate::sync::v1::MutationBatch;

use super::*;
use crate::sync::v1::engine;
use crate::{
    domain::{ClipboardItem, ClipboardKind},
    storage::{ClipboardRepository, Database, StoragePaths},
    sync::v1::{DownloadedFile, DownloadedObject, ObjectInfo, ObjectMetadata, ResourceCategory},
};

const REMOTE_SCOPE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct MemoryStore {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    fail_checkpoint_cas: Mutex<bool>,
    list_without_etags: Mutex<bool>,
    deleted: Mutex<Vec<String>>,
    gets: Mutex<Vec<String>>,
    lists: Mutex<Vec<String>>,
    puts: Mutex<Vec<String>>,
}

impl ObjectStore for MemoryStore {
    fn list(&self, prefix: &str, start_after: Option<&str>) -> Result<Vec<ObjectInfo>, String> {
        self.lists.lock().unwrap().push(prefix.to_string());
        let include_etags = !*self.list_without_etags.lock().unwrap();
        Ok(self
            .objects
            .lock()
            .unwrap()
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .filter(|(key, _)| start_after.is_none_or(|cursor| key.as_str() > cursor))
            .map(|(key, bytes)| ObjectInfo {
                key: key.clone(),
                size_bytes: Some(bytes.len() as u64),
                modified_ms: None,
                etag: include_etags.then(|| format!("\"{}\"", hex::encode(Sha256::digest(bytes)))),
            })
            .collect())
    }

    fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
        self.gets.lock().unwrap().push(key.to_string());
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .map(|bytes| DownloadedObject {
                etag: Some(format!("\"{}\"", hex::encode(Sha256::digest(&bytes)))),
                bytes,
            }))
    }

    fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(key)
            .map(|bytes| ObjectMetadata {
                size_bytes: Some(bytes.len() as u64),
                etag: None,
            }))
    }

    fn get_to_file(
        &self,
        key: &str,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<Option<DownloadedFile>, String> {
        self.gets.lock().unwrap().push(key.to_string());
        let Some(bytes) = self.objects.lock().unwrap().get(key).cloned() else {
            return Ok(None);
        };
        if bytes.len() as u64 > max_bytes {
            return Err("memory object exceeds limit".to_string());
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        Ok(Some(DownloadedFile {
            size_bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(&bytes)),
            etag: None,
        }))
    }

    fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        condition: PutCondition,
    ) -> Result<PutOutcome, String> {
        self.puts.lock().unwrap().push(key.to_string());
        let mut objects = self.objects.lock().unwrap();
        if key == CHECKPOINT_HEAD_KEY
            && matches!(condition, PutCondition::IfAbsent | PutCondition::IfMatch(_))
            && *self.fail_checkpoint_cas.lock().unwrap()
        {
            return Ok(PutOutcome::PreconditionFailed);
        }
        if matches!(condition, PutCondition::IfAbsent) && objects.contains_key(key) {
            return Ok(PutOutcome::PreconditionFailed);
        }
        if let PutCondition::IfMatch(expected) = &condition {
            let Some(existing) = objects.get(key) else {
                return Ok(PutOutcome::PreconditionFailed);
            };
            let actual = format!("\"{}\"", hex::encode(Sha256::digest(existing)));
            if &actual != expected {
                return Ok(PutOutcome::PreconditionFailed);
            }
        }
        let etag = format!("\"{}\"", hex::encode(Sha256::digest(&bytes)));
        objects.insert(key.to_string(), bytes);
        Ok(PutOutcome::Stored { etag: Some(etag) })
    }

    fn put_file(
        &self,
        key: &str,
        path: &Path,
        sha256: &str,
        size_bytes: u64,
        condition: PutCondition,
    ) -> Result<PutOutcome, String> {
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        if bytes.len() as u64 != size_bytes || hex::encode(Sha256::digest(&bytes)) != sha256 {
            return Err("memory file fingerprint mismatch".to_string());
        }
        self.put(key, bytes, condition)
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.objects.lock().unwrap().remove(key);
        self.deleted.lock().unwrap().push(key.to_string());
        Ok(())
    }
}

fn temp_paths(label: &str) -> StoragePaths {
    let project = std::env::temp_dir().join(format!(
        "clipboard-v1-engine-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    StoragePaths::initialize(project).unwrap()
}

fn engine_paths(paths: &StoragePaths) -> SyncEnginePaths {
    paths.into()
}

fn sync_database(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    options: SyncEngineOptions,
) -> Result<SyncEngineResult, String> {
    engine::sync_database(
        store,
        database,
        &engine_paths(paths),
        remote_scope,
        session_key,
        options,
    )
}

#[allow(clippy::too_many_arguments)]
fn maybe_compact(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    local_device_id: &str,
    local_state: &SyncRemoteState,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    engine::test_support::maybe_compact(
        store,
        database,
        &engine_paths(paths),
        remote_scope,
        local_device_id,
        local_state,
        session_key,
        resource_limits,
        result,
    )
}

#[allow(clippy::too_many_arguments)]
fn pull_device(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    engine::test_support::pull_device(
        store,
        database,
        &engine_paths(paths),
        remote_scope,
        head,
        session_key,
        resource_limits,
        result,
    )
}

fn text_item(id: &str, text: &str) -> ClipboardItem {
    ClipboardItem {
        id: id.to_string(),
        kind: ClipboardKind::Text,
        title: text.to_string(),
        text_content: Some(text.to_string()),
        html_content: None,
        rtf_content: None,
        resource_path: None,
        preview_path: None,
        content_hash: format!("hash-{id}"),
        source_app: None,
        icon_path: None,
        size_bytes: text.len() as u64,
        created_at_ms: 1,
        last_used_at_ms: None,
        is_favorite: false,
        metadata_json: Some("{}".to_string()),
    }
}

fn image_item(id: &str, path: &Path) -> ClipboardItem {
    let path = path.to_string_lossy().to_string();
    ClipboardItem {
        id: id.to_string(),
        kind: ClipboardKind::Image,
        title: id.to_string(),
        text_content: None,
        html_content: None,
        rtf_content: None,
        resource_path: Some(path.clone()),
        preview_path: Some(path.clone()),
        content_hash: format!("hash-{id}"),
        source_app: None,
        icon_path: None,
        size_bytes: fs::metadata(path.as_str()).unwrap().len(),
        created_at_ms: 1,
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

fn replicated_text(
    id: &str,
    text: &str,
    modified_at_ms: i64,
    writer_device_id: &str,
) -> ReplicatedItem {
    ReplicatedItem {
        item: text_item(id, text).into(),
        version: RecordVersion {
            modified_at_ms,
            writer_device_id: writer_device_id.to_string(),
        },
    }
}

#[derive(Clone)]
struct CheckpointFixture {
    generation: u64,
    vector: Vec<DeviceCursor>,
    mutations: MutationBatch,
}

fn insert_checkpoint(store: &MemoryStore, checkpoint: &CheckpointFixture) -> ObjectRef {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-checkpoint-fixture-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let encoded = crate::sync::v1::wire::encode_checkpoint_pack(
        &directory,
        &CheckpointPackHeader {
            generation: checkpoint.generation,
            vector: checkpoint.vector.clone(),
        },
        (!checkpoint.mutations.is_empty()).then_some(checkpoint.mutations.clone()),
        None,
    )
    .unwrap();
    let key = checkpoint_object_key(checkpoint.generation, &encoded.sha256).unwrap();
    let reference = ObjectRef {
        key: key.clone(),
        sha256: encoded.sha256.clone(),
        stored_size_bytes: encoded.stored_size_bytes,
        record_count: checkpoint.mutations.len() as u64,
    };
    store
        .objects
        .lock()
        .unwrap()
        .insert(key, fs::read(encoded.path()).unwrap());
    drop(encoded);
    let _ = fs::remove_dir(&directory);
    reference
}

fn insert_checkpoint_head(
    store: &MemoryStore,
    checkpoint: &CheckpointFixture,
    reference: ObjectRef,
    previous_checkpoint: Option<ObjectRef>,
) {
    let head = CheckpointHead {
        generation: checkpoint.generation,
        checkpoint: reference,
        vector: checkpoint.vector.clone(),
        previous_checkpoint,
        updated_at_ms: 1,
    };
    let encoded = encode_checkpoint_head(&head, None).unwrap();
    store
        .objects
        .lock()
        .unwrap()
        .insert(CHECKPOINT_HEAD_KEY.to_string(), encoded.bytes);
}

#[test]
fn independent_snapshots_and_incremental_segments_converge() {
    let store = MemoryStore::default();
    store
        .objects
        .lock()
        .unwrap()
        .insert("baseline-old.zip".to_string(), b"old".to_vec());
    let first_paths = temp_paths("first");
    let second_paths = temp_paths("second");
    let first = Database::open(&first_paths.database).unwrap();
    let second = Database::open(&second_paths.database).unwrap();
    first.save_item(&text_item("first-item", "first")).unwrap();
    second
        .save_item(&text_item("second-item", "second"))
        .unwrap();

    let first_run =
        sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    assert_eq!(first_run.deleted_remote_objects, 1);
    let second_run = sync_database(
        &store,
        &second,
        &second_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert!(second_run.applied_entries >= 1);
    assert!(second.get_item("first-item").unwrap().is_some());

    sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    assert!(first.get_item("second-item").unwrap().is_some());

    first.save_item(&text_item("incremental", "new")).unwrap();
    let pushed =
        sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    assert_eq!(pushed.uploaded_entries, 1);
    let pulled = sync_database(
        &store,
        &second,
        &second_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert_eq!(pulled.downloaded_entries, 1);
    assert!(second.get_item("incremental").unwrap().is_some());

    let idle = sync_database(
        &store,
        &second,
        &second_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert_eq!(idle.uploaded_entries, 0);
    assert_eq!(idle.downloaded_entries, 0);
    assert_eq!(idle.bytes_uploaded, 0);

    drop(first);
    drop(second);
    fs::remove_dir_all(first_paths.project).unwrap();
    fs::remove_dir_all(second_paths.project).unwrap();
}

#[test]
fn ordinary_pull_defers_resource_download_and_can_republish_the_reference() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("deferred-source");
    let target_paths = temp_paths("deferred-target");
    fs::create_dir_all(&source_paths.images).unwrap();
    let source_image = source_paths.images.join("source.png");
    fs::write(&source_image, b"image-bytes").unwrap();
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source
        .save_item(&image_item("remote-image", &source_image))
        .unwrap();

    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    let resource_key = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .find(|key| key.starts_with("v1/resources/image/"))
        .cloned()
        .unwrap();
    store.gets.lock().unwrap().clear();

    let pulled = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert_eq!(pulled.downloaded_resources, 0);
    assert!(!store.gets.lock().unwrap().contains(&resource_key));
    let local = target.get_item("remote-image").unwrap().unwrap();
    assert!(local.resource_path.is_none());
    assert!(local.preview_path.is_none());
    assert!(!local.metadata_json.unwrap().contains("v1/resources/"));

    let exported = target.export_sync_snapshot_for_scope(REMOTE_SCOPE).unwrap();
    let remote = exported
        .mutations
        .upserts
        .iter()
        .find(|item| item.item.id == "remote-image")
        .unwrap();
    assert_eq!(
        remote.item.resource_path.as_deref(),
        Some(resource_key.as_str())
    );
    assert!(remote.item.preview_path.is_none());
    assert!(remote
        .item
        .metadata_json
        .as_deref()
        .unwrap()
        .contains(resource_key.as_str()));

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn encrypted_resource_stays_private_and_materializes_only_on_demand() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("encrypted-resource-source");
    let target_paths = temp_paths("encrypted-resource-target");
    fs::create_dir_all(&source_paths.images).unwrap();
    let source_image = source_paths.images.join("private.png");
    let plaintext = b"private-image-fragment".repeat(4096);
    fs::write(&source_image, &plaintext).unwrap();
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    let key = SessionKey::derive("password", REMOTE_SCOPE).unwrap();
    source
        .save_item(&image_item("encrypted-image", &source_image))
        .unwrap();

    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap();
    let resource_key = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .find(|key| key.starts_with("v1/resources/image/"))
        .cloned()
        .unwrap();
    let stored_resource = store
        .objects
        .lock()
        .unwrap()
        .get(&resource_key)
        .cloned()
        .unwrap();
    assert_ne!(stored_resource, plaintext);
    assert!(!stored_resource
        .windows(b"private-image-fragment".len())
        .any(|window| window == b"private-image-fragment"));
    store.gets.lock().unwrap().clear();

    let pulled = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap();
    assert_eq!(pulled.downloaded_resources, 0);
    assert!(!store.gets.lock().unwrap().contains(&resource_key));
    assert!(target
        .get_item("encrypted-image")
        .unwrap()
        .unwrap()
        .resource_path
        .is_none());

    let exported = target.export_sync_snapshot_for_scope(REMOTE_SCOPE).unwrap();
    let forwarded = exported
        .mutations
        .upserts
        .iter()
        .find(|item| item.item.id == "encrypted-image")
        .unwrap();
    assert_eq!(
        forwarded.item.resource_path.as_deref(),
        Some(resource_key.as_str())
    );

    let materialized = materialize_resource(
        &store,
        &resource_key,
        &target_paths.images,
        options().resource_limits.image_bytes,
        Some(&key),
    )
    .unwrap();
    assert_eq!(fs::read(materialized.path).unwrap(), plaintext);
    assert_eq!(materialized.transferred_bytes, stored_resource.len() as u64);

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn wrong_password_is_rejected_before_a_new_device_publishes() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("password-source");
    let target_paths = temp_paths("password-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source.save_item(&text_item("private", "private")).unwrap();
    let right = SessionKey::derive("right", REMOTE_SCOPE).unwrap();
    let wrong = SessionKey::derive("wrong", REMOTE_SCOPE).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        Some(&right),
        options(),
    )
    .unwrap();
    let target_device_id = target.get_sync_device_id().unwrap();
    let target_head_key = head_object_key(&target_device_id).unwrap();
    store.puts.lock().unwrap().clear();

    let error = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        Some(&wrong),
        options(),
    )
    .unwrap_err();
    assert!(error.contains("cannot authenticate the existing sync v1 namespace"));
    assert!(store.puts.lock().unwrap().is_empty());
    assert!(!store.objects.lock().unwrap().contains_key(&target_head_key));

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn encryption_mode_change_is_rejected_before_a_new_device_publishes() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("plaintext-source");
    let target_paths = temp_paths("encrypted-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source.save_item(&text_item("public", "public")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    let key = SessionKey::derive("password", REMOTE_SCOPE).unwrap();
    let target_device_id = target.get_sync_device_id().unwrap();
    let target_head_key = head_object_key(&target_device_id).unwrap();
    store.puts.lock().unwrap().clear();

    let error = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap_err();
    assert!(error.contains("namespace encryption mode does not match"));
    assert!(store.puts.lock().unwrap().is_empty());
    assert!(!store.objects.lock().unwrap().contains_key(&target_head_key));

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn encrypted_retry_produces_one_immutable_segment_object() {
    let store = MemoryStore::default();
    let paths = temp_paths("encrypted");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    let key = SessionKey::derive("password", REMOTE_SCOPE).unwrap();
    sync_database(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap();
    database.save_item(&text_item("later", "later")).unwrap();
    sync_database(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap();

    let device_id = database.get_sync_device_id().unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let prefix = segment_prefix(&device_id, &state.epoch).unwrap();
    let segment_count = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .filter(|key| key.starts_with(&prefix))
        .count();
    assert_eq!(segment_count, 1);

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn corrupt_peer_head_does_not_block_other_devices() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("isolated-source");
    let target_paths = temp_paths("isolated-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source
        .save_item(&text_item("healthy-peer-item", "healthy"))
        .unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    store.objects.lock().unwrap().insert(
        "v1/heads/00000000-0000-4000-8000-000000000000.bin".to_string(),
        b"corrupt-head".to_vec(),
    );

    let result = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();

    assert_eq!(result.failed_peers, 1);
    assert!(result.applied_entries >= 1);
    assert!(target.get_item("healthy-peer-item").unwrap().is_some());

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn empty_device_bootstraps_from_checkpoint_without_peer_history() {
    let store = MemoryStore::default();
    let source_device = "11111111-1111-4111-8111-111111111111";
    let source_epoch = "22222222-2222-4222-8222-222222222222";
    let checkpoint = CheckpointFixture {
        generation: 1,
        vector: vec![DeviceCursor {
            device_id: source_device.to_string(),
            epoch: source_epoch.to_string(),
            sequence: 0,
            last_segment_key: None,
        }],
        mutations: crate::sync::v1::MutationBatch {
            upserts: vec![replicated_text(
                "checkpoint-only",
                "checkpoint",
                100,
                source_device,
            )],
            tombstones: Vec::new(),
        },
    };
    let reference = insert_checkpoint(&store, &checkpoint);
    insert_checkpoint_head(&store, &checkpoint, reference, None);
    let paths = temp_paths("checkpoint-bootstrap");
    let database = Database::open(&paths.database).unwrap();

    let result = sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert_eq!(result.downloaded_entries, 1);
    assert_eq!(result.applied_entries, 1);
    assert!(database.get_item("checkpoint-only").unwrap().is_some());
    assert!(database
        .get_sync_checkpoint_state(REMOTE_SCOPE)
        .unwrap()
        .is_some_and(|(generation, _)| generation >= 1));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn corrupt_current_checkpoint_falls_back_to_previous_generation() {
    let store = MemoryStore::default();
    let source_device = "11111111-1111-4111-8111-111111111111";
    let source_epoch = "22222222-2222-4222-8222-222222222222";
    let previous = CheckpointFixture {
        generation: 1,
        vector: vec![DeviceCursor {
            device_id: source_device.to_string(),
            epoch: source_epoch.to_string(),
            sequence: 0,
            last_segment_key: None,
        }],
        mutations: crate::sync::v1::MutationBatch {
            upserts: vec![replicated_text(
                "previous-only",
                "previous",
                100,
                source_device,
            )],
            tombstones: Vec::new(),
        },
    };
    let previous_ref = insert_checkpoint(&store, &previous);
    let current = CheckpointFixture {
        generation: 2,
        vector: previous.vector.clone(),
        mutations: crate::sync::v1::MutationBatch {
            upserts: vec![replicated_text(
                "current-only",
                "current",
                200,
                source_device,
            )],
            tombstones: Vec::new(),
        },
    };
    let current_ref = insert_checkpoint(&store, &current);
    store
        .objects
        .lock()
        .unwrap()
        .insert(current_ref.key.clone(), b"corrupt".to_vec());
    insert_checkpoint_head(&store, &current, current_ref, Some(previous_ref.clone()));
    let paths = temp_paths("checkpoint-fallback");
    let database = Database::open(&paths.database).unwrap();

    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert!(database.get_item("previous-only").unwrap().is_some());
    assert!(database.get_item("current-only").unwrap().is_none());
    assert!(database
        .get_sync_checkpoint_state(REMOTE_SCOPE)
        .unwrap()
        .is_some_and(|(generation, _)| generation >= 1));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn idle_sync_does_not_read_or_publish_a_checkpoint() {
    let store = MemoryStore::default();
    let paths = temp_paths("checkpoint-idle");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    store.gets.lock().unwrap().clear();
    store.puts.lock().unwrap().clear();

    let idle = sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert_eq!(idle.uploaded_entries, 0);
    assert_eq!(idle.bytes_uploaded, 0);
    assert_eq!(idle.deleted_remote_objects, 0);
    assert!(!store
        .gets
        .lock()
        .unwrap()
        .iter()
        .any(|key| key == CHECKPOINT_HEAD_KEY));
    assert!(!store
        .puts
        .lock()
        .unwrap()
        .iter()
        .any(|key| key == CHECKPOINT_HEAD_KEY || key.starts_with("v1/checkpoints/")));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn idle_sync_uses_one_head_listing_and_zero_head_gets_after_cache_warmup() {
    let store = MemoryStore::default();
    let first_paths = temp_paths("head-cache-first");
    let second_paths = temp_paths("head-cache-second");
    let first = Database::open(&first_paths.database).unwrap();
    let second = Database::open(&second_paths.database).unwrap();
    first.save_item(&text_item("first", "first")).unwrap();
    second.save_item(&text_item("second", "second")).unwrap();

    sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    sync_database(
        &store,
        &second,
        &second_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    store.gets.lock().unwrap().clear();
    store.lists.lock().unwrap().clear();

    let idle = sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();

    assert_eq!(idle.uploaded_entries, 0);
    assert_eq!(idle.downloaded_entries, 0);
    assert_eq!(idle.bytes_downloaded, 0);
    assert_eq!(
        store
            .lists
            .lock()
            .unwrap()
            .iter()
            .filter(|prefix| prefix.as_str() == HEADS_PREFIX)
            .count(),
        1
    );
    assert!(!store
        .gets
        .lock()
        .unwrap()
        .iter()
        .any(|key| key.starts_with(HEADS_PREFIX)));

    drop(first);
    drop(second);
    fs::remove_dir_all(first_paths.project).unwrap();
    fs::remove_dir_all(second_paths.project).unwrap();
}

#[test]
fn changed_head_etag_invalidates_cache_and_applies_the_new_segment() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("head-cache-change-source");
    let target_paths = temp_paths("head-cache-change-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source.save_item(&text_item("initial", "initial")).unwrap();

    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    source.save_item(&text_item("later", "later")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    let source_head = head_object_key(&source.get_sync_device_id().unwrap()).unwrap();
    store.gets.lock().unwrap().clear();

    let pulled = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();

    assert_eq!(pulled.downloaded_entries, 1);
    assert!(target.get_item("later").unwrap().is_some());
    assert!(store.gets.lock().unwrap().contains(&source_head));

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn missing_list_etag_falls_back_to_head_gets() {
    let store = MemoryStore::default();
    let paths = temp_paths("head-cache-no-etag");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let local_head = head_object_key(&database.get_sync_device_id().unwrap()).unwrap();
    *store.list_without_etags.lock().unwrap() = true;
    store.gets.lock().unwrap().clear();

    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert!(store.gets.lock().unwrap().contains(&local_head));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn checkpoint_cas_loser_never_garbage_collects_history() {
    let store = MemoryStore::default();
    let paths = temp_paths("checkpoint-cas-loser");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let device_id = database.get_sync_device_id().unwrap();
    database.save_item(&text_item("later", "later")).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let protected_snapshot = state.snapshot.as_ref().unwrap().key.clone();
    store.deleted.lock().unwrap().clear();
    *store.fail_checkpoint_cas.lock().unwrap() = true;
    database
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();

    let mut result = SyncEngineResult::default();
    maybe_compact(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &device_id,
        &state,
        None,
        options().resource_limits,
        &mut result,
    )
    .unwrap();

    assert_eq!(result.deleted_remote_objects, 0);
    assert!(result.bytes_uploaded > 0);
    assert!(store.deleted.lock().unwrap().is_empty());
    assert!(store
        .objects
        .lock()
        .unwrap()
        .contains_key(&protected_snapshot));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn successful_compaction_keeps_current_and_previous_checkpoints_only() {
    let store = MemoryStore::default();
    let paths = temp_paths("checkpoint-retention");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let device_id = database.get_sync_device_id().unwrap();
    let first_state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let first_snapshot = first_state.snapshot.as_ref().unwrap().key.clone();
    database.save_item(&text_item("second", "second")).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let first_state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
    let mut first_compaction = SyncEngineResult::default();
    maybe_compact(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &device_id,
        &first_state,
        None,
        options().resource_limits,
        &mut first_compaction,
    )
    .unwrap();
    assert!(!store.objects.lock().unwrap().contains_key(&first_snapshot));

    database.save_item(&text_item("third", "third")).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let mut second_compaction = SyncEngineResult::default();
    maybe_compact(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &device_id,
        &state,
        None,
        options().resource_limits,
        &mut second_compaction,
    )
    .unwrap();

    let checkpoint_count = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .filter(|key| key.starts_with("v1/checkpoints/"))
        .count();
    assert_eq!(checkpoint_count, 2);
    let head = decode_checkpoint_head(
        store
            .objects
            .lock()
            .unwrap()
            .get(CHECKPOINT_HEAD_KEY)
            .unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(head.generation, 3);
    assert!(head.previous_checkpoint.is_some());

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn stale_compactor_never_prunes_a_newer_checkpoint_candidate() {
    let store = MemoryStore::default();
    let device_id = "11111111-1111-4111-8111-111111111111";
    let epoch = "22222222-2222-4222-8222-222222222222";
    let vector = vec![DeviceCursor {
        device_id: device_id.to_string(),
        epoch: epoch.to_string(),
        sequence: 0,
        last_segment_key: None,
    }];
    let checkpoint = |generation| CheckpointFixture {
        generation,
        vector: vector.clone(),
        mutations: crate::sync::v1::MutationBatch {
            upserts: Vec::new(),
            tombstones: Vec::new(),
        },
    };
    let first = insert_checkpoint(&store, &checkpoint(1));
    let second = insert_checkpoint(&store, &checkpoint(2));
    let third = insert_checkpoint(&store, &checkpoint(3));
    let future = insert_checkpoint(&store, &checkpoint(4));
    let head = CheckpointHead {
        generation: 3,
        checkpoint: third.clone(),
        vector,
        previous_checkpoint: Some(second.clone()),
        updated_at_ms: 1,
    };

    assert_eq!(
        engine::test_support::prune_unreferenced_checkpoints(&store, &head).unwrap(),
        1
    );

    let objects = store.objects.lock().unwrap();
    assert!(!objects.contains_key(&first.key));
    assert!(objects.contains_key(&second.key));
    assert!(objects.contains_key(&third.key));
    assert!(objects.contains_key(&future.key));
}

#[test]
fn fourth_device_bootstraps_after_three_device_compaction_and_gc() {
    let store = MemoryStore::default();
    let first_paths = temp_paths("compact-first");
    let second_paths = temp_paths("compact-second");
    let third_paths = temp_paths("compact-third");
    let fourth_paths = temp_paths("compact-fourth");
    let first = Database::open(&first_paths.database).unwrap();
    let second = Database::open(&second_paths.database).unwrap();
    let third = Database::open(&third_paths.database).unwrap();
    let fourth = Database::open(&fourth_paths.database).unwrap();
    first.save_item(&text_item("first-only", "first")).unwrap();
    second
        .save_item(&text_item("second-only", "second"))
        .unwrap();
    third.save_item(&text_item("third-only", "third")).unwrap();

    for _ in 0..2 {
        sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
        sync_database(
            &store,
            &second,
            &second_paths,
            REMOTE_SCOPE,
            None,
            options(),
        )
        .unwrap();
        sync_database(&store, &third, &third_paths, REMOTE_SCOPE, None, options()).unwrap();
    }
    let all_ids = ["first-only", "second-only", "third-only"];
    for database in [&first, &second, &third] {
        for id in all_ids {
            assert!(
                database.get_item(id).unwrap().is_some(),
                "missing item {id}"
            );
        }
    }

    first
        .save_item(&text_item("after-convergence", "after convergence"))
        .unwrap();
    sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();

    let first_id = first.get_sync_device_id().unwrap();
    let first_state = first.get_or_create_sync_remote_state(REMOTE_SCOPE).unwrap();
    first
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
    let mut compacted = SyncEngineResult::default();
    maybe_compact(
        &store,
        &first,
        &first_paths,
        REMOTE_SCOPE,
        &first_id,
        &first_state,
        None,
        options().resource_limits,
        &mut compacted,
    )
    .unwrap();
    assert!(compacted.deleted_remote_objects > 0);

    store.gets.lock().unwrap().clear();
    let bootstrap = sync_database(
        &store,
        &fourth,
        &fourth_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert!(bootstrap.downloaded_entries >= 3);
    for id in [
        "first-only",
        "second-only",
        "third-only",
        "after-convergence",
    ] {
        assert!(fourth.get_item(id).unwrap().is_some(), "missing item {id}");
    }
    assert!(store
        .gets
        .lock()
        .unwrap()
        .iter()
        .any(|key| key == CHECKPOINT_HEAD_KEY));

    drop(first);
    drop(second);
    drop(third);
    drop(fourth);
    for paths in [first_paths, second_paths, third_paths, fourth_paths] {
        fs::remove_dir_all(paths.project).unwrap();
    }
}

#[test]
fn missing_segment_chain_recovers_from_newer_checkpoint() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("gap-source");
    let target_paths = temp_paths("gap-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source.save_item(&text_item("initial", "initial")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    source.save_item(&text_item("segment-one", "one")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    source.save_item(&text_item("segment-two", "two")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    source
        .save_item(&text_item("segment-three", "three"))
        .unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();

    let source_id = source.get_sync_device_id().unwrap();
    let state = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let second_segment_key = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .filter(|key| key.starts_with(&segment_prefix(&source_id, &state.epoch).unwrap()))
        .find(|key| parse_segment_key(key).unwrap().first_sequence == 2)
        .cloned()
        .unwrap();
    store.objects.lock().unwrap().remove(&second_segment_key);
    let current_head = decode_checkpoint_head(
        store
            .objects
            .lock()
            .unwrap()
            .get(CHECKPOINT_HEAD_KEY)
            .unwrap(),
        None,
    )
    .unwrap();
    let checkpoint = CheckpointFixture {
        generation: current_head.generation + 1,
        vector: vec![DeviceCursor {
            device_id: source_id.clone(),
            epoch: state.epoch.clone(),
            sequence: 3,
            last_segment_key: state.last_segment_key.clone(),
        }],
        mutations: source.export_sync_snapshot().unwrap().mutations,
    };
    let reference = insert_checkpoint(&store, &checkpoint);
    insert_checkpoint_head(
        &store,
        &checkpoint,
        reference,
        Some(current_head.checkpoint),
    );
    target
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            checkpoint.generation,
            &engine::test_support::checkpoint_digest_for_generation(
                &decode_checkpoint_head(
                    &store
                        .objects
                        .lock()
                        .unwrap()
                        .get(CHECKPOINT_HEAD_KEY)
                        .unwrap()
                        .clone(),
                    None,
                )
                .unwrap(),
                checkpoint.generation,
            )
            .unwrap(),
            &checkpoint.vector,
            &checkpoint.mutations,
        )
        .unwrap();
    assert_eq!(
        target
            .get_sync_cursor(REMOTE_SCOPE, &source_id)
            .unwrap()
            .unwrap()
            .sequence,
        3
    );
    target
        .with_connection(|connection| {
            connection.execute(
                "UPDATE sync_cursors
                    SET sequence = 1, last_segment_key = ?3
                  WHERE remote_scope = ?1 AND device_id = ?2",
                rusqlite::params![REMOTE_SCOPE, &source_id, {
                    let first_segment = store
                        .objects
                        .lock()
                        .unwrap()
                        .keys()
                        .filter(|key| {
                            key.starts_with(&segment_prefix(&source_id, &state.epoch).unwrap())
                        })
                        .find(|key| parse_segment_key(key).unwrap().first_sequence == 1)
                        .cloned()
                        .unwrap();
                    first_segment
                }],
            )?;
            Ok(())
        })
        .unwrap();

    let recovered = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();

    assert!(recovered.downloaded_entries >= 4);
    for id in ["initial", "segment-one", "segment-two", "segment-three"] {
        assert!(target.get_item(id).unwrap().is_some(), "missing item {id}");
    }
    assert_eq!(
        target
            .get_sync_cursor(REMOTE_SCOPE, &source_id)
            .unwrap()
            .unwrap()
            .sequence,
        3
    );

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn pulls_and_garbage_collects_more_than_one_thousand_segments() {
    const SEGMENT_COUNT: u64 = 1_001;

    let store = MemoryStore::default();
    let paths = temp_paths("segment-pagination");
    let database = Database::open(&paths.database).unwrap();
    database.initialize_sync().unwrap();
    let device_id = "11111111-1111-4111-8111-111111111111";
    let epoch = "22222222-2222-4222-8222-222222222222";
    let snapshot_header = SnapshotPackHeader {
        device_id: device_id.to_string(),
        epoch: epoch.to_string(),
        through_sequence: 0,
    };
    let directory = std::env::temp_dir().join(format!(
        "clipboard-snapshot-fixture-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let encoded_snapshot = crate::sync::v1::wire::encode_snapshot_pack(
        &directory,
        &snapshot_header,
        std::iter::empty::<MutationBatch>(),
        None,
    )
    .unwrap();
    let snapshot_key = snapshot_object_key(device_id, epoch, &encoded_snapshot.sha256).unwrap();
    let snapshot_ref = ObjectRef {
        key: snapshot_key.clone(),
        sha256: encoded_snapshot.sha256.clone(),
        stored_size_bytes: encoded_snapshot.stored_size_bytes,
        record_count: 0,
    };
    store
        .objects
        .lock()
        .unwrap()
        .insert(snapshot_key, fs::read(encoded_snapshot.path()).unwrap());
    drop(encoded_snapshot);
    let _ = fs::remove_dir(&directory);

    let mut last_segment_key = None;
    let mut thousandth_segment_key = None;
    for sequence in 1..=SEGMENT_COUNT {
        let segment = Segment {
            device_id: device_id.to_string(),
            epoch: epoch.to_string(),
            first_sequence: sequence,
            last_sequence: sequence,
            mutations: crate::sync::v1::MutationBatch {
                upserts: vec![replicated_text(
                    &format!("segment-{sequence}"),
                    &format!("segment {sequence}"),
                    sequence as i64,
                    device_id,
                )],
                tombstones: Vec::new(),
            },
        };
        let encoded = encode_segment(&segment, None).unwrap();
        let key =
            segment_object_key(device_id, epoch, sequence, sequence, &encoded.sha256).unwrap();
        if sequence == 1_000 {
            thousandth_segment_key = Some(key.clone());
        }
        store
            .objects
            .lock()
            .unwrap()
            .insert(key.clone(), encoded.bytes);
        last_segment_key = Some(key);
    }
    let head = DeviceHead {
        device_id: device_id.to_string(),
        epoch: epoch.to_string(),
        snapshot: snapshot_ref,
        published_sequence: SEGMENT_COUNT,
        last_segment_key: last_segment_key.clone(),
        updated_at_ms: 1,
    };
    let mut result = SyncEngineResult::default();

    pull_device(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &head,
        None,
        options().resource_limits,
        &mut result,
    )
    .unwrap();

    assert_eq!(result.downloaded_entries, SEGMENT_COUNT);
    assert_eq!(result.applied_entries, SEGMENT_COUNT);
    assert!(database
        .get_item(&format!("segment-{SEGMENT_COUNT}"))
        .unwrap()
        .is_some());
    assert_eq!(
        database
            .get_sync_cursor(REMOTE_SCOPE, device_id)
            .unwrap()
            .unwrap()
            .sequence,
        SEGMENT_COUNT
    );

    let deleted = engine::test_support::garbage_collect_covered_history(
        &store,
        &[DeviceCursor {
            device_id: device_id.to_string(),
            epoch: epoch.to_string(),
            sequence: 1_000,
            last_segment_key: thousandth_segment_key,
        }],
    )
    .unwrap();
    assert_eq!(deleted, 1_001);
    assert!(store
        .objects
        .lock()
        .unwrap()
        .contains_key(last_segment_key.as_ref().unwrap()));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn restored_database_merges_its_remote_head_before_rotating_epoch() {
    let store = MemoryStore::default();
    let paths = temp_paths("restored-head");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let device_id = database.get_sync_device_id().unwrap();
    let head_key = head_object_key(&device_id).unwrap();
    let backup_path = paths.project.join("restored.sqlite3");
    database.vacuum_into(&backup_path).unwrap();

    database
        .save_item(&text_item("remote-only", "remote-only"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let advanced_head =
        decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None).unwrap();
    assert_eq!(advanced_head.published_sequence, 1);
    drop(database);

    let restored = Database::open(&backup_path).unwrap();
    restored
        .save_item(&text_item("local-after-restore", "local-after-restore"))
        .unwrap();
    let healed = sync_database(&store, &restored, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert!(healed.downloaded_entries >= 2);
    assert_eq!(healed.uploaded_entries, 3);
    for id in ["initial", "remote-only", "local-after-restore"] {
        assert!(
            restored.get_item(id).unwrap().is_some(),
            "missing item {id}"
        );
    }
    let replacement_head =
        decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None).unwrap();
    assert_ne!(replacement_head.epoch, advanced_head.epoch);
    assert!(replacement_head.last_segment_key.is_none());
    assert_eq!(replacement_head.snapshot.record_count, 3);

    drop(restored);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn divergent_local_head_state_does_not_overwrite_remote_history() {
    let store = MemoryStore::default();
    let paths = temp_paths("divergent-head");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let device_id = database.get_sync_device_id().unwrap();
    let head_key = head_object_key(&device_id).unwrap();
    database
        .save_item(&text_item("remote-only", "remote-only"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let advanced_head =
        decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None).unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE sync_publication_state
                    SET published_sequence = 999,
                        last_segment_key = 'v1/segments/diverged.pack'
                  WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            connection.execute("DELETE FROM clipboard_items WHERE id = 'remote-only'", [])?;
            connection.execute(
                "DELETE FROM sync_tombstones WHERE item_id = 'remote-only'",
                [],
            )?;
            connection.execute("DELETE FROM sync_outbox", [])?;
            Ok(())
        })
        .unwrap();

    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert!(database.get_item("remote-only").unwrap().is_some());
    let replacement_head =
        decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None).unwrap();
    assert_ne!(replacement_head.epoch, advanced_head.epoch);
    assert_eq!(replacement_head.snapshot.record_count, 2);

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn resource_category_is_part_of_the_object_identity() {
    let digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert_ne!(
        crate::sync::v1::resource_object_key(ResourceCategory::Image, digest, "png").unwrap(),
        crate::sync::v1::resource_object_key(ResourceCategory::Icon, digest, "png").unwrap()
    );
}
