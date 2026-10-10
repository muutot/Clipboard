//! End-to-end v1 replication engine tests.
//!
//! The shared `MemoryStore` fake, path/item fixtures and checkpoint helpers
//! live here so every scenario module can reach them with `use super::*`; the
//! `#[test]` functions are grouped by the behavior they exercise.

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

mod cancellation;
mod checkpoint;
mod compaction;
mod encryption;
mod flow;
mod namespace;
mod orphan;

#[derive(Default)]
struct MemoryStore {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    fail_checkpoint_cas: Mutex<bool>,
    drop_checkpoint_after_success: Mutex<bool>,
    list_without_etags: Mutex<bool>,
    get_without_etags: Mutex<bool>,
    deleted: Mutex<Vec<String>>,
    gets: Mutex<Vec<String>>,
    lists: Mutex<Vec<String>>,
    puts: Mutex<Vec<String>>,
    cancel_after_key_fragment:
        Mutex<Option<(String, clipboard_sync::cancellation::CancellationToken)>>,
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
        let etagless = key == CHECKPOINT_HEAD_KEY && *self.get_without_etags.lock().unwrap();
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .map(|bytes| DownloadedObject {
                etag: (!etagless).then(|| format!("\"{}\"", hex::encode(Sha256::digest(&bytes)))),
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
        if key == CHECKPOINT_HEAD_KEY && *self.drop_checkpoint_after_success.lock().unwrap() {
            objects.remove(key);
            return Ok(PutOutcome::Stored {
                etag: Some("\"lost-checkpoint-pointer\"".to_string()),
            });
        }
        let etag = format!("\"{}\"", hex::encode(Sha256::digest(&bytes)));
        objects.insert(key.to_string(), bytes);
        if let Some((fragment, token)) = self.cancel_after_key_fragment.lock().unwrap().as_ref() {
            if key.contains(fragment) {
                token.cancel();
            }
        }
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
