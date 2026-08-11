use sha2::{Digest, Sha256};

use crate::storage::{Database, StoragePaths};

use super::{
    cleanup_obsolete_objects, decode_device_head, decode_segment, decode_snapshot,
    encode_device_head, encode_segment, encode_snapshot, head_object_key,
    materialize_mutation_resources, parse_head_key, parse_segment_key, prepare_mutation_resources,
    segment_object_key, segment_prefix, snapshot_object_key, DeviceCursor, DeviceHead,
    EncodedObject, ObjectRef, ObjectStore, PutCondition, PutOutcome, ResourceLimits, Segment,
    SessionKey, Snapshot, HEADS_PREFIX,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncEngineOptions {
    pub segment_max_entries: usize,
    pub resource_limits: ResourceLimits,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncEngineResult {
    pub uploaded_entries: u64,
    pub downloaded_entries: u64,
    pub applied_entries: u64,
    pub uploaded_resources: u64,
    pub downloaded_resources: u64,
    pub deleted_remote_objects: u64,
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
}

/// Runs one complete S3/object-store synchronization pass. Publication is
/// upload-first; immutable packs and resources become visible through the
/// device head only after their writes succeed. Pull cursors advance in the
/// same SQLite transaction as the corresponding mutation batch.
pub fn sync_database(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    options: SyncEngineOptions,
) -> Result<SyncEngineResult, String> {
    if options.segment_max_entries == 0 {
        return Err("sync segment entry limit must be greater than zero".to_string());
    }
    database
        .initialize_sync()
        .map_err(|error| error.to_string())?;
    let device_id = database
        .get_sync_device_id()
        .map_err(|error| error.to_string())?;
    let mut result = SyncEngineResult::default();
    let mut state = database
        .get_or_create_sync_remote_state(remote_scope)
        .map_err(|error| error.to_string())?;

    if !state.remote_prepared {
        let cleanup = cleanup_obsolete_objects(store)?;
        result.deleted_remote_objects = cleanup.deleted_objects;
        remove_obsolete_local_manifest(paths, remote_scope)?;
        database
            .mark_sync_remote_prepared(remote_scope)
            .map_err(|error| error.to_string())?;
        state = database
            .get_or_create_sync_remote_state(remote_scope)
            .map_err(|error| error.to_string())?;
    }

    if !state.initialized {
        state = publish_bootstrap(
            store,
            database,
            paths,
            remote_scope,
            &device_id,
            state,
            session_key,
            options.resource_limits,
            &mut result,
        )?;
    }

    while let Some(batch) = database
        .get_sync_outbox_batch(options.segment_max_entries)
        .map_err(|error| error.to_string())?
    {
        state = publish_segment(
            store,
            database,
            paths,
            remote_scope,
            &device_id,
            state,
            batch,
            session_key,
            options.resource_limits,
            &mut result,
        )?;
    }

    pull_remote_devices(
        store,
        database,
        paths,
        remote_scope,
        &device_id,
        session_key,
        options.resource_limits,
        &mut result,
    )?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn publish_bootstrap(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    device_id: &str,
    state: crate::storage::SyncRemoteState,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<crate::storage::SyncRemoteState, String> {
    let exported = database
        .export_sync_snapshot()
        .map_err(|error| error.to_string())?;
    let mut snapshot = Snapshot {
        device_id: device_id.to_string(),
        epoch: state.epoch.clone(),
        through_sequence: exported.through_sequence,
        mutations: exported.mutations,
    };
    let resources =
        prepare_mutation_resources(store, &mut snapshot.mutations, paths, resource_limits)?;
    result.uploaded_resources += resources.transferred_resources;
    result.bytes_uploaded = checked_add(
        result.bytes_uploaded,
        resources.transferred_bytes,
        "uploaded byte count",
    )?;

    let encoded = encode_snapshot(&snapshot, session_key)?;
    let snapshot_key = snapshot_object_key(device_id, &state.epoch, &encoded.sha256)?;
    put_immutable(store, &snapshot_key, &encoded, result)?;
    let snapshot_ref = ObjectRef {
        key: snapshot_key,
        sha256: encoded.sha256,
        stored_size_bytes: encoded.bytes.len() as u64,
        record_count: snapshot.mutations.len() as u64,
    };
    let head = DeviceHead {
        device_id: device_id.to_string(),
        epoch: state.epoch.clone(),
        snapshot: snapshot_ref.clone(),
        published_sequence: snapshot.through_sequence,
        last_segment_key: None,
        updated_at_ms: current_time_ms(),
    };
    publish_head(store, &head, session_key, result)?;
    result.uploaded_entries = checked_add(
        result.uploaded_entries,
        snapshot.mutations.len() as u64,
        "uploaded entry count",
    )?;
    database
        .commit_sync_bootstrap_published(
            remote_scope,
            &state.epoch,
            &snapshot_ref,
            snapshot.through_sequence,
        )
        .map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
fn publish_segment(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    device_id: &str,
    state: crate::storage::SyncRemoteState,
    batch: crate::storage::SyncOutboxBatch,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<crate::storage::SyncRemoteState, String> {
    let mut segment = Segment {
        device_id: device_id.to_string(),
        epoch: state.epoch.clone(),
        first_sequence: batch.first_sequence,
        last_sequence: batch.last_sequence,
        mutations: batch.mutations,
    };
    let resources =
        prepare_mutation_resources(store, &mut segment.mutations, paths, resource_limits)?;
    result.uploaded_resources += resources.transferred_resources;
    result.bytes_uploaded = checked_add(
        result.bytes_uploaded,
        resources.transferred_bytes,
        "uploaded byte count",
    )?;

    let encoded = encode_segment(&segment, session_key)?;
    let segment_key = segment_object_key(
        device_id,
        &state.epoch,
        segment.first_sequence,
        segment.last_sequence,
        &encoded.sha256,
    )?;
    put_immutable(store, &segment_key, &encoded, result)?;
    let mut next_state = state.clone();
    next_state.published_sequence = segment.last_sequence;
    next_state.last_segment_key = Some(segment_key.clone());
    next_state.updated_at_ms = current_time_ms();
    let head = next_state
        .device_head(device_id)
        .map_err(|error| error.to_string())?;
    publish_head(store, &head, session_key, result)?;
    result.uploaded_entries = checked_add(
        result.uploaded_entries,
        segment.mutations.len() as u64,
        "uploaded entry count",
    )?;
    database
        .commit_sync_segment_published(
            remote_scope,
            &state.epoch,
            &segment_key,
            segment.last_sequence,
        )
        .map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
fn pull_remote_devices(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    local_device_id: &str,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    let mut heads = store.list(HEADS_PREFIX, None)?;
    heads.sort_by(|left, right| left.key.cmp(&right.key));
    for info in heads {
        let device_id = parse_head_key(&info.key)?;
        if device_id == local_device_id {
            continue;
        }
        let downloaded = store
            .get(&info.key)?
            .ok_or_else(|| format!("remote head {:?} disappeared during sync", info.key))?;
        result.bytes_downloaded = checked_add(
            result.bytes_downloaded,
            downloaded.bytes.len() as u64,
            "downloaded byte count",
        )?;
        let head = decode_device_head(&downloaded.bytes, session_key)?;
        validate_head(&info.key, &device_id, &head)?;
        pull_device(
            store,
            database,
            paths,
            remote_scope,
            &head,
            session_key,
            resource_limits,
            result,
        )?;
    }
    Ok(())
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
    let mut cursor = database
        .get_sync_cursor(remote_scope, &head.device_id)
        .map_err(|error| error.to_string())?;
    if cursor
        .as_ref()
        .is_none_or(|cursor| cursor.epoch != head.epoch)
    {
        cursor = Some(pull_snapshot(
            store,
            database,
            paths,
            remote_scope,
            head,
            session_key,
            resource_limits,
            result,
        )?);
    }
    let mut cursor =
        cursor.ok_or_else(|| "remote snapshot did not establish a cursor".to_string())?;
    if cursor.sequence > head.published_sequence {
        return Err(format!(
            "remote head for {} regressed from local sequence {} to {}",
            head.device_id, cursor.sequence, head.published_sequence
        ));
    }
    if cursor.sequence == head.published_sequence {
        return Ok(());
    }
    let head_last_key = head
        .last_segment_key
        .as_deref()
        .ok_or_else(|| "remote head advances without a last segment key".to_string())?;
    let prefix = segment_prefix(&head.device_id, &head.epoch)?;
    let mut segments = store.list(&prefix, cursor.last_segment_key.as_deref())?;
    segments.sort_by(|left, right| left.key.cmp(&right.key));
    let mut reached_head = false;
    for info in segments {
        if info.key.as_str() > head_last_key {
            break;
        }
        let parsed = parse_segment_key(&info.key)?;
        if parsed.device_id != head.device_id || parsed.epoch != head.epoch {
            return Err(format!(
                "segment {:?} does not belong to its device head",
                info.key
            ));
        }
        if parsed.last_sequence <= cursor.sequence {
            continue;
        }
        if parsed.first_sequence <= cursor.sequence {
            return Err(format!(
                "segment {:?} overlaps applied sequence {}",
                info.key, cursor.sequence
            ));
        }
        if parsed.last_sequence > head.published_sequence {
            break;
        }
        let downloaded =
            get_verified_object(store, &info.key, &parsed.sha256, info.size_bytes, result)?;
        let mut segment = decode_segment(&downloaded, session_key)?;
        if segment.device_id != parsed.device_id
            || segment.epoch != parsed.epoch
            || segment.first_sequence != parsed.first_sequence
            || segment.last_sequence != parsed.last_sequence
        {
            return Err(format!("segment payload does not match key {:?}", info.key));
        }
        let resources =
            materialize_mutation_resources(store, &mut segment.mutations, paths, resource_limits)?;
        result.downloaded_resources += resources.transferred_resources;
        result.bytes_downloaded = checked_add(
            result.bytes_downloaded,
            resources.transferred_bytes,
            "downloaded byte count",
        )?;
        result.downloaded_entries = checked_add(
            result.downloaded_entries,
            segment.mutations.len() as u64,
            "downloaded entry count",
        )?;
        let next_cursor = DeviceCursor {
            device_id: head.device_id.clone(),
            epoch: head.epoch.clone(),
            sequence: segment.last_sequence,
            last_segment_key: Some(info.key.clone()),
        };
        let applied = database
            .apply_sync_segment(remote_scope, &next_cursor, &segment.mutations)
            .map_err(|error| error.to_string())?;
        result.applied_entries =
            checked_add(result.applied_entries, applied, "applied entry count")?;
        cursor = next_cursor;
        if info.key == head_last_key {
            reached_head = true;
            break;
        }
    }
    if !reached_head || cursor.sequence != head.published_sequence {
        return Err(format!(
            "remote head for {} references an incomplete segment chain",
            head.device_id
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn pull_snapshot(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<DeviceCursor, String> {
    let downloaded = get_verified_object(
        store,
        &head.snapshot.key,
        &head.snapshot.sha256,
        Some(head.snapshot.stored_size_bytes),
        result,
    )?;
    let mut snapshot = decode_snapshot(&downloaded, session_key)?;
    if snapshot.device_id != head.device_id
        || snapshot.epoch != head.epoch
        || snapshot.mutations.len() as u64 != head.snapshot.record_count
        || snapshot.through_sequence > head.published_sequence
    {
        return Err(format!(
            "snapshot payload does not match head for {}",
            head.device_id
        ));
    }
    let resources =
        materialize_mutation_resources(store, &mut snapshot.mutations, paths, resource_limits)?;
    result.downloaded_resources += resources.transferred_resources;
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        resources.transferred_bytes,
        "downloaded byte count",
    )?;
    result.downloaded_entries = checked_add(
        result.downloaded_entries,
        snapshot.mutations.len() as u64,
        "downloaded entry count",
    )?;
    let cursor = DeviceCursor {
        device_id: head.device_id.clone(),
        epoch: head.epoch.clone(),
        sequence: snapshot.through_sequence,
        last_segment_key: None,
    };
    let applied = database
        .apply_sync_snapshot(
            remote_scope,
            &cursor,
            &head.snapshot.sha256,
            &snapshot.mutations,
        )
        .map_err(|error| error.to_string())?;
    result.applied_entries = checked_add(result.applied_entries, applied, "applied entry count")?;
    Ok(cursor)
}

fn validate_head(key: &str, device_id: &str, head: &DeviceHead) -> Result<(), String> {
    if head.device_id != device_id {
        return Err(format!("head payload device does not match key {key:?}"));
    }
    if head.snapshot.key
        != snapshot_object_key(&head.device_id, &head.epoch, &head.snapshot.sha256)?
    {
        return Err(format!(
            "head {key:?} contains a noncanonical snapshot reference"
        ));
    }
    match &head.last_segment_key {
        None => Ok(()),
        Some(segment_key) => {
            let parsed = parse_segment_key(segment_key)?;
            if parsed.device_id != head.device_id
                || parsed.epoch != head.epoch
                || parsed.last_sequence != head.published_sequence
            {
                return Err(format!(
                    "head {key:?} has an invalid last segment reference"
                ));
            }
            Ok(())
        }
    }
}

fn publish_head(
    store: &impl ObjectStore,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    let key = head_object_key(&head.device_id)?;
    let encoded = encode_device_head(head, session_key)?;
    let stored_size = encoded.stored_size_bytes();
    match store.put(&key, encoded.bytes, PutCondition::Unconditional)? {
        PutOutcome::Stored { .. } => {
            result.bytes_uploaded =
                checked_add(result.bytes_uploaded, stored_size, "uploaded byte count")?;
            Ok(())
        }
        PutOutcome::PreconditionFailed => {
            Err("unconditional device-head write failed its precondition".to_string())
        }
    }
}

fn put_immutable(
    store: &impl ObjectStore,
    key: &str,
    encoded: &EncodedObject,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    match store.put(key, encoded.bytes.clone(), PutCondition::IfAbsent)? {
        PutOutcome::Stored { .. } => {
            result.bytes_uploaded = checked_add(
                result.bytes_uploaded,
                encoded.stored_size_bytes(),
                "uploaded byte count",
            )?;
            Ok(())
        }
        PutOutcome::PreconditionFailed => {
            let metadata = store
                .head(key)?
                .ok_or_else(|| format!("immutable object {key:?} disappeared after a retry"))?;
            if metadata
                .size_bytes
                .is_some_and(|size| size != encoded.stored_size_bytes())
            {
                return Err(format!("immutable object {key:?} has an unexpected size"));
            }
            Ok(())
        }
    }
}

fn get_verified_object(
    store: &impl ObjectStore,
    key: &str,
    expected_sha256: &str,
    expected_size: Option<u64>,
    result: &mut SyncEngineResult,
) -> Result<Vec<u8>, String> {
    let downloaded = store
        .get(key)?
        .ok_or_else(|| format!("remote object {key:?} does not exist"))?;
    let actual_size = downloaded.bytes.len() as u64;
    if expected_size.is_some_and(|size| size != actual_size) {
        return Err(format!(
            "remote object {key:?} size does not match its reference"
        ));
    }
    let actual_sha256 = hex::encode(Sha256::digest(&downloaded.bytes));
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "remote object {key:?} digest does not match its reference"
        ));
    }
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        actual_size,
        "downloaded byte count",
    )?;
    Ok(downloaded.bytes)
}

fn remove_obsolete_local_manifest(paths: &StoragePaths, remote_scope: &str) -> Result<(), String> {
    let path = paths
        .data_directory
        .join(format!("sync-pool-manifest-{remote_scope}.json"));
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
            std::fs::remove_file(&path)
                .map_err(|error| format!("failed to delete obsolete local sync manifest: {error}"))
        }
        Ok(_) => Err("obsolete local sync manifest path is not a file".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to inspect obsolete local sync manifest: {error}"
        )),
    }
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn checked_add(left: u64, right: u64, label: &str) -> Result<u64, String> {
    left.checked_add(right)
        .ok_or_else(|| format!("{label} overflowed"))
}

trait EncodedObjectExt {
    fn stored_size_bytes(&self) -> u64;
}

impl EncodedObjectExt for EncodedObject {
    fn stored_size_bytes(&self) -> u64 {
        self.bytes.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, io::Write, path::Path, sync::Mutex};

    use super::*;
    use crate::{
        domain::{ClipboardItem, ClipboardKind},
        storage::ClipboardRepository,
        sync::v1::{
            DownloadedFile, DownloadedObject, ObjectInfo, ObjectMetadata, ResourceCategory,
        },
    };

    const REMOTE_SCOPE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Default)]
    struct MemoryStore {
        objects: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl ObjectStore for MemoryStore {
        fn list(&self, prefix: &str, start_after: Option<&str>) -> Result<Vec<ObjectInfo>, String> {
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
                })
                .collect())
        }

        fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .map(|bytes| DownloadedObject { bytes, etag: None }))
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
            let mut objects = self.objects.lock().unwrap();
            if matches!(condition, PutCondition::IfAbsent) && objects.contains_key(key) {
                return Ok(PutOutcome::PreconditionFailed);
            }
            objects.insert(key.to_string(), bytes);
            Ok(PutOutcome::Stored { etag: None })
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
    fn resource_category_is_part_of_the_object_identity() {
        let digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_ne!(
            super::super::resource_object_key(ResourceCategory::Image, digest, "png").unwrap(),
            super::super::resource_object_key(ResourceCategory::Icon, digest, "png").unwrap()
        );
    }
}
