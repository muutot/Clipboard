use std::{collections::BTreeMap, fs, path::PathBuf};

use sha2::{Digest, Sha256};

use crate::storage::{Database, StoragePaths};

use super::wire::envelope_is_encrypted;
use super::{
    cleanup_obsolete_objects, collect_mutation_resource_refs, decode_checkpoint_head,
    decode_device_head, decode_segment, defer_mutation_resources, encode_device_head,
    encode_segment, head_object_key, large_pack_chunk_limit_bytes, mutation_batch_encoded_size,
    open_checkpoint_pack, open_snapshot_pack, parse_checkpoint_key, parse_head_key,
    parse_segment_key, prepare_mutation_resources, segment_object_key, segment_prefix,
    snapshot_object_key, CheckpointHead, CheckpointPackHeader, DeviceCursor, DeviceHead,
    EncodedFile, EncodedObject, LargePackKind, LargePackWriter, MutationBatch, ObjectInfo,
    ObjectRef, ObjectStore, PutCondition, PutOutcome, ResourceLimits, Segment, SessionKey,
    SnapshotPackHeader, CHECKPOINT_HEAD_KEY, HEADS_PREFIX,
};

const CHECKPOINT_SEQUENCE_DELTA_THRESHOLD: u64 = 50_000;
const LARGE_PACK_BATCH_ENTRIES: usize = 2048;
const MAX_LARGE_PACK_STORED_BYTES: u64 = 1024 * 1024 * 1024;

fn write_large_pack_batch(
    writer: &mut LargePackWriter<'_>,
    mutations: MutationBatch,
) -> Result<(), crate::storage::StorageError> {
    if mutations.is_empty() {
        return Ok(());
    }
    let encoded_size = mutation_batch_encoded_size(&mutations)
        .map_err(crate::storage::StorageError::InvalidSyncState)?;
    if encoded_size <= large_pack_chunk_limit_bytes() {
        return writer
            .write_batch(&mutations)
            .map_err(crate::storage::StorageError::InvalidSyncState);
    }
    if mutations.len() == 1 {
        return Err(crate::storage::StorageError::InvalidSyncState(
            "one sync record exceeds the large-pack chunk size limit".to_string(),
        ));
    }

    if !mutations.upserts.is_empty() {
        let middle = mutations.upserts.len() / 2;
        if middle > 0 {
            let mut left = mutations;
            let right = left.upserts.split_off(middle);
            write_large_pack_batch(writer, left)?;
            return write_large_pack_batch(
                writer,
                MutationBatch {
                    upserts: right,
                    tombstones: Vec::new(),
                },
            );
        }
    }

    let middle = mutations.tombstones.len() / 2;
    let mut left = mutations;
    let right = left.tombstones.split_off(middle);
    write_large_pack_batch(writer, left)?;
    write_large_pack_batch(
        writer,
        MutationBatch {
            upserts: Vec::new(),
            tombstones: right,
        },
    )
}

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
    pub failed_peers: u64,
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

    let mut heads = store.list(HEADS_PREFIX, None)?;
    heads.sort_by(|left, right| left.key.cmp(&right.key));

    state = reconcile_local_device_head(
        store,
        database,
        paths,
        remote_scope,
        &device_id,
        state,
        &heads,
        session_key,
        options.resource_limits,
        &mut result,
    )?;

    validate_remote_access_before_first_publish(store, &state, &heads, session_key, &mut result)?;

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
        .get_sync_outbox_batch_for_scope(remote_scope, options.segment_max_entries)
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

    pull_checkpoint_if_needed(
        store,
        database,
        paths,
        remote_scope,
        session_key,
        options.resource_limits,
        &mut result,
        false,
    )?;

    pull_remote_devices(
        store,
        database,
        paths,
        remote_scope,
        &device_id,
        &heads,
        session_key,
        options.resource_limits,
        &mut result,
    )?;
    if result.failed_peers == 0 {
        state = database
            .get_or_create_sync_remote_state(remote_scope)
            .map_err(|error| error.to_string())?;
        maybe_compact(
            store,
            database,
            paths,
            remote_scope,
            &device_id,
            &state,
            session_key,
            options.resource_limits,
            &mut result,
        )?;
    }
    Ok(result)
}

fn validate_remote_access_before_first_publish(
    store: &impl ObjectStore,
    state: &crate::storage::SyncRemoteState,
    heads: &[ObjectInfo],
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    if state.initialized {
        return Ok(());
    }

    let mut canonical_pointers = 0u64;
    let mut valid_pointers = 0u64;
    let mut last_error = None::<String>;
    for info in heads {
        let Ok(device_id) = parse_head_key(&info.key) else {
            continue;
        };
        let Some(downloaded) = store.get(&info.key)? else {
            continue;
        };
        canonical_pointers = canonical_pointers.saturating_add(1);
        result.bytes_downloaded = checked_add(
            result.bytes_downloaded,
            downloaded.bytes.len() as u64,
            "downloaded byte count",
        )?;
        let validation = envelope_is_encrypted(&downloaded.bytes).and_then(|encrypted| {
            if encrypted != session_key.is_some() {
                return Err(
                    "sync v1 namespace encryption mode does not match the configuration"
                        .to_string(),
                );
            }
            decode_device_head(&downloaded.bytes, session_key)
                .and_then(|head| validate_head(&info.key, &device_id, &head))
        });
        match validation {
            Ok(()) => valid_pointers = valid_pointers.saturating_add(1),
            Err(error) if error.contains("namespace encryption mode") => {
                return Err(remote_access_error(error));
            }
            Err(error) => last_error = Some(error),
        }
    }

    if let Some(downloaded) = store.get(CHECKPOINT_HEAD_KEY)? {
        canonical_pointers = canonical_pointers.saturating_add(1);
        result.bytes_downloaded = checked_add(
            result.bytes_downloaded,
            downloaded.bytes.len() as u64,
            "downloaded byte count",
        )?;
        envelope_is_encrypted(&downloaded.bytes)
            .and_then(|encrypted| {
                if encrypted != session_key.is_some() {
                    return Err(
                        "sync v1 namespace encryption mode does not match the configuration"
                            .to_string(),
                    );
                }
                decode_checkpoint_head(&downloaded.bytes, session_key)
                    .and_then(|head| validate_checkpoint_head(&head))
            })
            .map_err(remote_access_error)?;
        valid_pointers = valid_pointers.saturating_add(1);
    }

    if canonical_pointers == 0 || valid_pointers > 0 {
        return Ok(());
    }
    Err(remote_access_error(last_error.unwrap_or_else(|| {
        "no valid remote pointer was found".to_string()
    })))
}

fn remote_access_error(error: String) -> String {
    format!(
        "cannot authenticate the existing sync v1 namespace before first publication; verify the encryption password or use a dedicated remote-scope reset workflow: {error}"
    )
}

#[allow(clippy::too_many_arguments)]
fn reconcile_local_device_head(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    device_id: &str,
    state: crate::storage::SyncRemoteState,
    heads: &[ObjectInfo],
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<crate::storage::SyncRemoteState, String> {
    let key = head_object_key(device_id)?;
    let listed = heads.iter().find(|info| info.key == key);
    if state.initialized
        && listed.is_some_and(|info| {
            local_head_cache_matches(database, remote_scope, device_id, &state, info)
                .unwrap_or(false)
        })
    {
        return Ok(state);
    }
    let Some(downloaded) = store.get(&key)? else {
        if state.initialized {
            return database
                .reset_sync_remote_state(remote_scope)
                .map_err(|error| error.to_string());
        }
        return Ok(state);
    };
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        downloaded.bytes.len() as u64,
        "downloaded byte count",
    )?;
    let remote_head = decode_device_head(&downloaded.bytes, session_key)?;
    validate_head(&key, device_id, &remote_head)?;
    if local_publication_matches_remote(&state, &remote_head) {
        record_head_cache(
            database,
            remote_scope,
            device_id,
            listed,
            downloaded.etag.as_deref(),
            downloaded.bytes.len() as u64,
            &remote_head,
        );
        return Ok(state);
    }

    let already_applied = database
        .get_sync_cursor(remote_scope, device_id)
        .map_err(|error| error.to_string())?
        .is_some_and(|cursor| {
            cursor.epoch == remote_head.epoch && cursor.sequence >= remote_head.published_sequence
        });
    if !already_applied {
        pull_device_with_checkpoint_recovery(
            store,
            database,
            paths,
            remote_scope,
            &remote_head,
            session_key,
            resource_limits,
            result,
        )?;
    }

    database
        .reset_sync_remote_state(remote_scope)
        .map_err(|error| error.to_string())
}

fn local_publication_matches_remote(
    state: &crate::storage::SyncRemoteState,
    head: &DeviceHead,
) -> bool {
    state.initialized
        && state.epoch == head.epoch
        && state.snapshot.as_ref() == Some(&head.snapshot)
        && state.published_sequence == head.published_sequence
        && state.last_segment_key == head.last_segment_key
}

fn listed_head_identity(info: &ObjectInfo) -> Option<(&str, u64)> {
    Some((info.etag.as_deref()?, info.size_bytes?))
}

fn cache_matches_listing(cache: &crate::storage::SyncHeadCache, info: &ObjectInfo) -> bool {
    listed_head_identity(info).is_some_and(|(etag, size)| {
        cache.etag == etag
            && cache.stored_size_bytes == size
            && cache
                .modified_ms
                .zip(info.modified_ms)
                .is_none_or(|(cached, listed)| cached == listed)
    })
}

fn local_head_cache_matches(
    database: &Database,
    remote_scope: &str,
    device_id: &str,
    state: &crate::storage::SyncRemoteState,
    info: &ObjectInfo,
) -> Result<bool, String> {
    let Some(cache) = database
        .get_sync_head_cache(remote_scope, device_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    let head = state
        .device_head(device_id)
        .map_err(|error| error.to_string())?;
    Ok(cache_matches_listing(&cache, info) && cache.matches_head(&head))
}

fn peer_head_cache_matches(
    database: &Database,
    remote_scope: &str,
    device_id: &str,
    info: &ObjectInfo,
) -> Result<bool, String> {
    let Some(cache) = database
        .get_sync_head_cache(remote_scope, device_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    let Some(cursor) = database
        .get_sync_cursor(remote_scope, device_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    Ok(cache_matches_listing(&cache, info) && cache.matches_cursor(&cursor))
}

fn record_head_cache(
    database: &Database,
    remote_scope: &str,
    device_id: &str,
    listed: Option<&ObjectInfo>,
    downloaded_etag: Option<&str>,
    stored_size_bytes: u64,
    head: &DeviceHead,
) {
    let etag = downloaded_etag.or_else(|| listed.and_then(|info| info.etag.as_deref()));
    if let Some(etag) = etag {
        let modified_ms = listed.and_then(|info| info.modified_ms);
        let _ = database.record_sync_head_cache(
            remote_scope,
            device_id,
            etag,
            stored_size_bytes,
            modified_ms,
            head,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn pull_checkpoint_if_needed(
    store: &impl ObjectStore,
    database: &Database,
    _paths: &StoragePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    _resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
    force: bool,
) -> Result<bool, String> {
    if !force {
        let has_checkpoint = database
            .get_sync_checkpoint_state(remote_scope)
            .map_err(|error| error.to_string())?
            .is_some();
        let has_peer_cursors = !database
            .list_sync_cursors(remote_scope)
            .map_err(|error| error.to_string())?
            .is_empty();
        if has_checkpoint || has_peer_cursors {
            return Ok(false);
        }
    }
    let Some(downloaded_head) = store.get(CHECKPOINT_HEAD_KEY)? else {
        return if force {
            Err("sync checkpoint is required to recover missing history".to_string())
        } else {
            Ok(false)
        };
    };
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        downloaded_head.bytes.len() as u64,
        "downloaded byte count",
    )?;
    let checkpoint_head = decode_checkpoint_head(&downloaded_head.bytes, session_key)?;
    validate_checkpoint_head(&checkpoint_head)?;
    if !force
        && database
            .get_sync_checkpoint_state(remote_scope)
            .map_err(|error| error.to_string())?
            .is_some_and(|(generation, sha256)| {
                generation == checkpoint_head.generation
                    && sha256 == checkpoint_head.checkpoint.sha256
            })
    {
        return Ok(false);
    }

    let checkpoint = match download_checkpoint(
        store,
        &checkpoint_head.checkpoint,
        checkpoint_head.generation,
        session_key,
        result,
    ) {
        Ok(checkpoint) => {
            if checkpoint.header.vector != checkpoint_head.vector {
                return Err("checkpoint payload vector does not match its head".to_string());
            }
            checkpoint
        }
        Err(current_error) => {
            let previous = checkpoint_head.previous_checkpoint.as_ref().ok_or_else(|| {
                format!("current checkpoint is unusable and no previous checkpoint is retained: {current_error}")
            })?;
            let parsed = parse_checkpoint_key(&previous.key)?;
            download_checkpoint(
                store,
                previous,
                parsed.generation,
                session_key,
                result,
            )
            .map_err(|previous_error| {
                format!(
                    "current checkpoint failed ({current_error}); previous checkpoint failed ({previous_error})"
                )
            })?
        }
    };
    let generation = checkpoint.header.generation;
    let vector = checkpoint.header.vector.clone();
    let expected_record_count = checkpoint.expected_record_count;
    let mut reader = open_checkpoint_pack(&checkpoint.file.path, session_key)?;
    let mut terminal_checked = false;
    let batches = std::iter::from_fn(|| {
        if terminal_checked {
            return None;
        }
        match reader.next() {
            Some(batch) => Some(
                batch
                    .and_then(|mut mutations| {
                        let resource_refs = defer_mutation_resources(&mut mutations)?;
                        Ok((mutations, resource_refs))
                    })
                    .map_err(crate::storage::StorageError::InvalidSyncState),
            ),
            None => {
                terminal_checked = true;
                if reader.record_count() != expected_record_count || !reader.is_complete() {
                    Some(Err(crate::storage::StorageError::InvalidSyncState(
                        "checkpoint payload does not match its reference".to_string(),
                    )))
                } else {
                    None
                }
            }
        }
    });
    let applied = database
        .apply_sync_checkpoint_batches(
            remote_scope,
            generation,
            &checkpoint_digest_for_generation(&checkpoint_head, generation)?,
            &vector,
            batches,
        )
        .map_err(|error| error.to_string())?;
    result.downloaded_entries = checked_add(
        result.downloaded_entries,
        expected_record_count,
        "downloaded entry count",
    )?;
    result.applied_entries = checked_add(result.applied_entries, applied, "applied entry count")?;
    Ok(true)
}

fn checkpoint_digest_for_generation(
    head: &CheckpointHead,
    generation: u64,
) -> Result<String, String> {
    if generation == head.generation {
        return Ok(head.checkpoint.sha256.clone());
    }
    let previous = head
        .previous_checkpoint
        .as_ref()
        .ok_or_else(|| "checkpoint generation is not referenced by its head".to_string())?;
    let parsed = parse_checkpoint_key(&previous.key)?;
    if parsed.generation != generation {
        return Err("checkpoint generation is not referenced by its head".to_string());
    }
    Ok(previous.sha256.clone())
}

fn download_checkpoint(
    store: &impl ObjectStore,
    reference: &ObjectRef,
    generation: u64,
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<DownloadedCheckpoint, String> {
    let parsed = parse_checkpoint_key(&reference.key)?;
    if parsed.generation != generation || parsed.sha256 != reference.sha256 {
        return Err("checkpoint reference is not canonical".to_string());
    }
    let file = get_verified_object_to_file(
        store,
        &reference.key,
        &reference.sha256,
        Some(reference.stored_size_bytes),
        &std::env::temp_dir(),
        result,
    )?;
    let reader = open_checkpoint_pack(&file.path, session_key)?;
    if reader.header.generation != generation {
        return Err("checkpoint payload does not match its reference".to_string());
    }
    validate_checkpoint_vector(&reader.header.vector)?;
    Ok(DownloadedCheckpoint {
        file,
        header: reader.header.clone(),
        expected_record_count: reference.record_count,
    })
}

struct DownloadedCheckpoint {
    file: TemporarySyncFile,
    header: CheckpointPackHeader,
    expected_record_count: u64,
}

fn validate_checkpoint_head(head: &CheckpointHead) -> Result<(), String> {
    let parsed = parse_checkpoint_key(&head.checkpoint.key)?;
    if head.generation == 0
        || parsed.generation != head.generation
        || parsed.sha256 != head.checkpoint.sha256
        || head.vector.is_empty()
    {
        return Err("checkpoint head is invalid".to_string());
    }
    validate_checkpoint_vector(&head.vector)?;
    if let Some(previous) = &head.previous_checkpoint {
        let previous_key = parse_checkpoint_key(&previous.key)?;
        if previous_key.generation >= head.generation || previous_key.sha256 != previous.sha256 {
            return Err("previous checkpoint reference is invalid".to_string());
        }
    }
    Ok(())
}

fn validate_checkpoint_vector(vector: &[DeviceCursor]) -> Result<(), String> {
    let mut previous_device = None::<&str>;
    for cursor in vector {
        if previous_device.is_some_and(|previous| previous >= cursor.device_id.as_str()) {
            return Err("checkpoint vector must contain unique sorted device ids".to_string());
        }
        let device = uuid::Uuid::parse_str(&cursor.device_id)
            .map_err(|_| "checkpoint cursor device is not canonical".to_string())?;
        if device.to_string() != cursor.device_id {
            return Err("checkpoint cursor device is not canonical".to_string());
        }
        let epoch = uuid::Uuid::parse_str(&cursor.epoch)
            .map_err(|_| "checkpoint cursor epoch is not canonical".to_string())?;
        if epoch.to_string() != cursor.epoch {
            return Err("checkpoint cursor epoch is not canonical".to_string());
        }
        if let Some(key) = cursor.last_segment_key.as_deref() {
            let parsed = parse_segment_key(key)?;
            if parsed.device_id != cursor.device_id
                || parsed.epoch != cursor.epoch
                || parsed.last_sequence != cursor.sequence
            {
                return Err("checkpoint cursor does not match its segment key".to_string());
            }
        }
        previous_device = Some(&cursor.device_id);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_database_pack(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    kind: LargePackKind,
    snapshot_header: Option<&SnapshotPackHeader>,
    checkpoint_header: Option<&CheckpointPackHeader>,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(EncodedFile, crate::storage::SyncSnapshotExport), String> {
    let database_snapshot = TemporaryDatabaseSnapshot::create(database, &paths.data_directory)?;
    let export_database =
        Database::open(&database_snapshot.path).map_err(|error| error.to_string())?;
    let mut writer = match kind {
        LargePackKind::Snapshot => LargePackWriter::new(
            &paths.data_directory,
            kind,
            snapshot_header.ok_or_else(|| "snapshot pack header is missing".to_string())?,
            session_key,
        )?,
        LargePackKind::Checkpoint => LargePackWriter::new(
            &paths.data_directory,
            kind,
            checkpoint_header.ok_or_else(|| "checkpoint pack header is missing".to_string())?,
            session_key,
        )?,
    };
    let export = export_database
        .visit_sync_snapshot_for_scope(remote_scope, LARGE_PACK_BATCH_ENTRIES, |mut mutations| {
            let resources = prepare_mutation_resources(
                store,
                &mut mutations,
                paths,
                resource_limits,
                session_key,
            )
            .map_err(crate::storage::StorageError::InvalidSyncState)?;
            result.uploaded_resources = result
                .uploaded_resources
                .checked_add(resources.transferred_resources)
                .ok_or(crate::storage::StorageError::ValueOutOfRange {
                    field: "uploaded sync resource count",
                })?;
            result.bytes_uploaded = result
                .bytes_uploaded
                .checked_add(resources.transferred_bytes)
                .ok_or(crate::storage::StorageError::ValueOutOfRange {
                    field: "uploaded sync byte count",
                })?;
            let resource_refs = collect_mutation_resource_refs(&mutations)
                .map_err(crate::storage::StorageError::InvalidSyncState)?;
            database.record_sync_resource_refs(remote_scope, &mutations, &resource_refs)?;
            write_large_pack_batch(&mut writer, mutations)
        })
        .map_err(|error| error.to_string())?;
    if let Some(header) = snapshot_header {
        writer.rewrite_header(&SnapshotPackHeader {
            device_id: header.device_id.clone(),
            epoch: header.epoch.clone(),
            through_sequence: export.through_sequence,
        })?;
    }
    let encoded = writer.finish()?;
    if encoded.record_count != export.record_count {
        return Err("sync pack record count changed during export".to_string());
    }
    Ok((encoded, export))
}

#[allow(clippy::too_many_arguments)]
fn maybe_compact(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    local_device_id: &str,
    local_state: &crate::storage::SyncRemoteState,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    let vector = frozen_checkpoint_vector(database, remote_scope, local_device_id, local_state)?;
    let baseline = database
        .get_sync_checkpoint_cursors(remote_scope)
        .map_err(|error| error.to_string())?;
    if vector.is_empty() || !checkpoint_compaction_due(&baseline, &vector) {
        return Ok(());
    }
    let existing = read_checkpoint_head(store, session_key, result)?;

    if let Some(current) = existing.as_ref() {
        let current_is_locally_verified = database
            .get_sync_checkpoint_state(remote_scope)
            .map_err(|error| error.to_string())?
            .is_some_and(|(generation, sha256)| {
                generation == current.head.generation && sha256 == current.head.checkpoint.sha256
            })
            && baseline == current.head.vector;
        if !current_is_locally_verified {
            match download_checkpoint(
                store,
                &current.head.checkpoint,
                current.head.generation,
                session_key,
                result,
            ) {
                Ok(checkpoint) if checkpoint.header.vector == current.head.vector => {}
                Ok(_) | Err(_) => return Ok(()),
            }
        }
        validate_compaction_vector(&current.head.vector, &vector)?;
        if current.head.vector == vector {
            finalize_checkpoint_publication(
                store,
                database,
                remote_scope,
                &current.head,
                None,
                session_key,
                result,
            )?;
            return Ok(());
        }
    }

    let generation = existing.as_ref().map_or(Ok(1), |current| {
        current
            .head
            .generation
            .checked_add(1)
            .ok_or_else(|| "checkpoint generation overflowed".to_string())
    })?;
    let checkpoint_header = CheckpointPackHeader {
        generation,
        vector: vector.clone(),
    };
    let (encoded, export) = encode_database_pack(
        store,
        database,
        paths,
        remote_scope,
        LargePackKind::Checkpoint,
        None,
        Some(&checkpoint_header),
        session_key,
        resource_limits,
        result,
    )?;
    if export.through_sequence != local_state.published_sequence {
        return Ok(());
    }
    let key = super::checkpoint_object_key(generation, &encoded.sha256)?;
    put_immutable_file(store, &key, &encoded, result)?;
    let checkpoint_ref = ObjectRef {
        key,
        sha256: encoded.sha256.clone(),
        stored_size_bytes: encoded.stored_size_bytes,
        record_count: encoded.record_count,
    };
    let head = CheckpointHead {
        generation,
        checkpoint: checkpoint_ref,
        vector,
        previous_checkpoint: existing
            .as_ref()
            .map(|current| current.head.checkpoint.clone()),
        updated_at_ms: current_time_ms(),
    };
    let encoded_head = super::encode_checkpoint_head(&head, session_key)?;
    let encoded_head_size = encoded_head.stored_size_bytes();
    let condition = existing.as_ref().map_or(PutCondition::IfAbsent, |current| {
        PutCondition::IfMatch(current.etag.clone())
    });
    match store.put(CHECKPOINT_HEAD_KEY, encoded_head.bytes, condition)? {
        PutOutcome::PreconditionFailed => return Ok(()),
        PutOutcome::Stored { .. } => {
            result.bytes_uploaded = checked_add(
                result.bytes_uploaded,
                encoded_head_size,
                "uploaded byte count",
            )?;
        }
    }
    finalize_checkpoint_publication(
        store,
        database,
        remote_scope,
        &head,
        existing
            .as_ref()
            .map(|current| current.head.vector.as_slice()),
        session_key,
        result,
    )
}

fn finalize_checkpoint_publication(
    store: &impl ObjectStore,
    database: &Database,
    remote_scope: &str,
    head: &CheckpointHead,
    covered_vector_hint: Option<&[DeviceCursor]>,
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    if let Some(previous) = head.previous_checkpoint.as_ref() {
        let previous_generation = parse_checkpoint_key(&previous.key)?.generation;
        let covered_vector = if let Some(vector) = covered_vector_hint {
            vector.to_vec()
        } else {
            let local_state = database
                .get_sync_checkpoint_state(remote_scope)
                .map_err(|error| error.to_string())?;
            let local_vector = if local_state.is_some_and(|(generation, sha256)| {
                generation == previous_generation && sha256 == previous.sha256
            }) {
                database
                    .get_sync_checkpoint_cursors(remote_scope)
                    .map_err(|error| error.to_string())?
            } else {
                Vec::new()
            };
            if local_vector.is_empty() {
                download_checkpoint(store, previous, previous_generation, session_key, result)?
                    .header
                    .vector
            } else {
                local_vector
            }
        };
        validate_checkpoint_vector(&covered_vector)?;
        let deleted = garbage_collect_covered_history(store, &covered_vector)?;
        result.deleted_remote_objects = checked_add(
            result.deleted_remote_objects,
            deleted,
            "deleted remote object count",
        )?;
    }
    let deleted = prune_unreferenced_checkpoints(store, head)?;
    result.deleted_remote_objects = checked_add(
        result.deleted_remote_objects,
        deleted,
        "deleted remote object count",
    )?;
    database
        .record_sync_checkpoint_published(
            remote_scope,
            head.generation,
            &head.checkpoint.sha256,
            &head.vector,
        )
        .map_err(|error| error.to_string())
}

fn prune_unreferenced_checkpoints(
    store: &impl ObjectStore,
    head: &CheckpointHead,
) -> Result<u64, String> {
    let mut retained = vec![head.checkpoint.key.as_str()];
    if let Some(previous) = head.previous_checkpoint.as_ref() {
        retained.push(previous.key.as_str());
    }
    let mut deleted = 0u64;
    for object in store.list("v1/checkpoints/", None)? {
        let Ok(parsed) = parse_checkpoint_key(&object.key) else {
            continue;
        };
        if retained.contains(&object.key.as_str()) || parsed.generation >= head.generation {
            continue;
        }
        store.delete(&object.key)?;
        deleted = checked_add(deleted, 1, "deleted remote object count")?;
    }
    Ok(deleted)
}

struct StoredCheckpointHead {
    head: CheckpointHead,
    etag: String,
}

fn read_checkpoint_head(
    store: &impl ObjectStore,
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<Option<StoredCheckpointHead>, String> {
    let Some(downloaded) = store.get(CHECKPOINT_HEAD_KEY)? else {
        return Ok(None);
    };
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        downloaded.bytes.len() as u64,
        "downloaded byte count",
    )?;
    let etag = downloaded
        .etag
        .ok_or_else(|| "checkpoint pointer response is missing an ETag".to_string())?;
    let head = decode_checkpoint_head(&downloaded.bytes, session_key)?;
    validate_checkpoint_head(&head)?;
    Ok(Some(StoredCheckpointHead { head, etag }))
}

fn frozen_checkpoint_vector(
    database: &Database,
    remote_scope: &str,
    local_device_id: &str,
    local_state: &crate::storage::SyncRemoteState,
) -> Result<Vec<DeviceCursor>, String> {
    let mut vector = database
        .list_sync_cursors(remote_scope)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|cursor| (cursor.device_id.clone(), cursor))
        .collect::<BTreeMap<_, _>>();
    vector.insert(
        local_device_id.to_string(),
        DeviceCursor {
            device_id: local_device_id.to_string(),
            epoch: local_state.epoch.clone(),
            sequence: local_state.published_sequence,
            last_segment_key: local_state.last_segment_key.clone(),
        },
    );
    let vector = vector.into_values().collect::<Vec<_>>();
    validate_checkpoint_vector(&vector)?;
    Ok(vector)
}

fn checkpoint_compaction_due(baseline: &[DeviceCursor], vector: &[DeviceCursor]) -> bool {
    if baseline.is_empty() {
        return true;
    }
    let previous = baseline
        .iter()
        .map(|cursor| (cursor.device_id.as_str(), cursor))
        .collect::<BTreeMap<_, _>>();
    if previous.len() != vector.len() {
        return true;
    }
    let mut delta = 0u64;
    for cursor in vector {
        let Some(old) = previous.get(cursor.device_id.as_str()) else {
            return true;
        };
        if old.epoch != cursor.epoch || old.sequence > cursor.sequence {
            return true;
        }
        delta = delta.saturating_add(cursor.sequence - old.sequence);
    }
    delta >= CHECKPOINT_SEQUENCE_DELTA_THRESHOLD
}

fn validate_compaction_vector(
    current: &[DeviceCursor],
    candidate: &[DeviceCursor],
) -> Result<(), String> {
    let current = current
        .iter()
        .map(|cursor| (cursor.device_id.as_str(), cursor))
        .collect::<BTreeMap<_, _>>();
    for device_id in current.keys() {
        if !candidate
            .iter()
            .any(|cursor| cursor.device_id == *device_id)
        {
            return Err(format!(
                "checkpoint candidate dropped known device {device_id}"
            ));
        }
    }
    for cursor in candidate {
        if let Some(existing) = current.get(cursor.device_id.as_str()) {
            if existing.epoch == cursor.epoch && existing.sequence > cursor.sequence {
                return Err(format!(
                    "checkpoint candidate regresses device {} from {} to {}",
                    cursor.device_id, existing.sequence, cursor.sequence
                ));
            }
        }
    }
    Ok(())
}

fn garbage_collect_covered_history(
    store: &impl ObjectStore,
    covered_vector: &[DeviceCursor],
) -> Result<u64, String> {
    let mut deleted = 0u64;
    for cursor in covered_vector {
        let snapshot_prefix = format!("v1/snapshots/{}/{}/", cursor.device_id, cursor.epoch);
        for object in store.list(&snapshot_prefix, None)? {
            store.delete(&object.key)?;
            deleted = checked_add(deleted, 1, "deleted remote object count")?;
        }
        let prefix = segment_prefix(&cursor.device_id, &cursor.epoch)?;
        for object in store.list(&prefix, None)? {
            let segment = parse_segment_key(&object.key)?;
            if segment.last_sequence <= cursor.sequence {
                store.delete(&object.key)?;
                deleted = checked_add(deleted, 1, "deleted remote object count")?;
            }
        }
    }
    Ok(deleted)
}

#[allow(clippy::too_many_arguments)]
fn pull_device_with_checkpoint_recovery(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    match pull_device(
        store,
        database,
        paths,
        remote_scope,
        head,
        session_key,
        resource_limits,
        result,
    ) {
        Ok(()) => Ok(()),
        Err(original_error) => {
            let recovered = pull_checkpoint_if_needed(
                store,
                database,
                paths,
                remote_scope,
                session_key,
                resource_limits,
                result,
                true,
            )?;
            if !recovered {
                return Err(original_error);
            }
            pull_device(
                store,
                database,
                paths,
                remote_scope,
                head,
                session_key,
                resource_limits,
                result,
            )
            .map_err(|retry_error| {
                format!(
                    "device pull failed before checkpoint recovery ({original_error}); retry failed ({retry_error})"
                )
            })
        }
    }
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
    let snapshot_header = SnapshotPackHeader {
        device_id: device_id.to_string(),
        epoch: state.epoch.clone(),
        through_sequence: 0,
    };
    let (encoded, export) = encode_database_pack(
        store,
        database,
        paths,
        remote_scope,
        LargePackKind::Snapshot,
        Some(&snapshot_header),
        None,
        session_key,
        resource_limits,
        result,
    )?;
    let snapshot_key = snapshot_object_key(device_id, &state.epoch, &encoded.sha256)?;
    put_immutable_file(store, &snapshot_key, &encoded, result)?;
    let snapshot_ref = ObjectRef {
        key: snapshot_key,
        sha256: encoded.sha256.clone(),
        stored_size_bytes: encoded.stored_size_bytes,
        record_count: encoded.record_count,
    };
    let head = DeviceHead {
        device_id: device_id.to_string(),
        epoch: state.epoch.clone(),
        snapshot: snapshot_ref.clone(),
        published_sequence: export.through_sequence,
        last_segment_key: None,
        updated_at_ms: current_time_ms(),
    };
    let published_head = publish_head(store, &head, session_key, result)?;
    result.uploaded_entries = checked_add(
        result.uploaded_entries,
        encoded.record_count,
        "uploaded entry count",
    )?;
    let state = database
        .commit_sync_bootstrap_published(
            remote_scope,
            &state.epoch,
            &snapshot_ref,
            export.through_sequence,
        )
        .map_err(|error| error.to_string())?;
    record_head_cache(
        database,
        remote_scope,
        device_id,
        None,
        published_head.etag.as_deref(),
        published_head.stored_size_bytes,
        &head,
    );
    Ok(state)
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
    let resources = prepare_mutation_resources(
        store,
        &mut segment.mutations,
        paths,
        resource_limits,
        session_key,
    )?;
    result.uploaded_resources += resources.transferred_resources;
    result.bytes_uploaded = checked_add(
        result.bytes_uploaded,
        resources.transferred_bytes,
        "uploaded byte count",
    )?;
    let resource_refs = collect_mutation_resource_refs(&segment.mutations)?;
    database
        .record_sync_resource_refs(remote_scope, &segment.mutations, &resource_refs)
        .map_err(|error| error.to_string())?;

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
    let published_head = publish_head(store, &head, session_key, result)?;
    result.uploaded_entries = checked_add(
        result.uploaded_entries,
        segment.mutations.len() as u64,
        "uploaded entry count",
    )?;
    let state = database
        .commit_sync_segment_published(
            remote_scope,
            &state.epoch,
            &segment_key,
            segment.last_sequence,
        )
        .map_err(|error| error.to_string())?;
    record_head_cache(
        database,
        remote_scope,
        device_id,
        None,
        published_head.etag.as_deref(),
        published_head.stored_size_bytes,
        &head,
    );
    Ok(state)
}

#[allow(clippy::too_many_arguments)]
fn pull_remote_devices(
    store: &impl ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    remote_scope: &str,
    local_device_id: &str,
    heads: &[ObjectInfo],
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    for info in heads {
        let peer_result = (|| -> Result<(), String> {
            let device_id = parse_head_key(&info.key)?;
            if device_id == local_device_id {
                return Ok(());
            }
            if peer_head_cache_matches(database, remote_scope, &device_id, info)? {
                return Ok(());
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
            pull_device_with_checkpoint_recovery(
                store,
                database,
                paths,
                remote_scope,
                &head,
                session_key,
                resource_limits,
                result,
            )?;
            record_head_cache(
                database,
                remote_scope,
                &device_id,
                Some(info),
                downloaded.etag.as_deref(),
                downloaded.bytes.len() as u64,
                &head,
            );
            Ok(())
        })();
        if let Err(error) = peer_result {
            result.failed_peers = checked_add(result.failed_peers, 1, "failed peer count")?;
            eprintln!("[sync] skipped remote head {:?}: {error}", info.key);
        }
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
        let expected_first = cursor
            .sequence
            .checked_add(1)
            .ok_or_else(|| "remote cursor sequence overflowed".to_string())?;
        if parsed.first_sequence != expected_first {
            return Err(format!(
                "segment {:?} starts at {}, expected {} after applied sequence {}",
                info.key, parsed.first_sequence, expected_first, cursor.sequence
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
        let resource_refs = defer_mutation_resources(&mut segment.mutations)?;
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
            .apply_sync_segment_with_resources(
                remote_scope,
                &next_cursor,
                &segment.mutations,
                &resource_refs,
            )
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
    _paths: &StoragePaths,
    remote_scope: &str,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    _resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<DeviceCursor, String> {
    let downloaded = get_verified_object_to_file(
        store,
        &head.snapshot.key,
        &head.snapshot.sha256,
        Some(head.snapshot.stored_size_bytes),
        &std::env::temp_dir(),
        result,
    )?;
    let mut reader = open_snapshot_pack(&downloaded.path, session_key)?;
    if reader.header.device_id != head.device_id
        || reader.header.epoch != head.epoch
        || reader.header.through_sequence > head.published_sequence
    {
        return Err(format!(
            "snapshot payload does not match head for {}",
            head.device_id
        ));
    }
    let cursor = DeviceCursor {
        device_id: head.device_id.clone(),
        epoch: head.epoch.clone(),
        sequence: reader.header.through_sequence,
        last_segment_key: None,
    };
    let expected_record_count = head.snapshot.record_count;
    let mut terminal_checked = false;
    let batches = std::iter::from_fn(|| {
        if terminal_checked {
            return None;
        }
        match reader.next() {
            Some(batch) => Some(
                batch
                    .and_then(|mut mutations| {
                        let resource_refs = defer_mutation_resources(&mut mutations)?;
                        Ok((mutations, resource_refs))
                    })
                    .map_err(crate::storage::StorageError::InvalidSyncState),
            ),
            None => {
                terminal_checked = true;
                if reader.record_count() != expected_record_count || !reader.is_complete() {
                    Some(Err(crate::storage::StorageError::InvalidSyncState(
                        format!(
                            "snapshot payload does not match head for {}",
                            head.device_id
                        ),
                    )))
                } else {
                    None
                }
            }
        }
    });
    let applied = database
        .apply_sync_snapshot_batches(remote_scope, &cursor, &head.snapshot.sha256, batches)
        .map_err(|error| error.to_string())?;
    result.downloaded_entries = checked_add(
        result.downloaded_entries,
        expected_record_count,
        "downloaded entry count",
    )?;
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

struct PublishedHead {
    etag: Option<String>,
    stored_size_bytes: u64,
}

fn publish_head(
    store: &impl ObjectStore,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<PublishedHead, String> {
    let key = head_object_key(&head.device_id)?;
    let encoded = encode_device_head(head, session_key)?;
    let stored_size = encoded.stored_size_bytes();
    match store.put(&key, encoded.bytes, PutCondition::Unconditional)? {
        PutOutcome::Stored { etag } => {
            result.bytes_uploaded =
                checked_add(result.bytes_uploaded, stored_size, "uploaded byte count")?;
            Ok(PublishedHead {
                etag,
                stored_size_bytes: stored_size,
            })
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

struct TemporarySyncFile {
    path: PathBuf,
}

impl TemporarySyncFile {
    fn new(directory: &std::path::Path, label: &str) -> Result<Self, String> {
        fs::create_dir_all(directory)
            .map_err(|error| format!("failed to create sync temporary directory: {error}"))?;
        for _ in 0..16 {
            let path = directory.join(format!(
                ".sync-{label}-{}-{:016x}.tmp",
                std::process::id(),
                rand::random::<u64>()
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("failed to create sync temporary file: {error}")),
            }
        }
        Err("failed to allocate a unique sync temporary file".to_string())
    }
}

impl Drop for TemporarySyncFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct TemporaryDatabaseSnapshot {
    path: PathBuf,
}

impl TemporaryDatabaseSnapshot {
    fn create(database: &Database, directory: &std::path::Path) -> Result<Self, String> {
        fs::create_dir_all(directory)
            .map_err(|error| format!("failed to create sync temporary directory: {error}"))?;
        for _ in 0..16 {
            let path = directory.join(format!(
                ".sync-database-snapshot-{}-{:016x}.sqlite3",
                std::process::id(),
                rand::random::<u64>()
            ));
            match fs::symlink_metadata(&path) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    database
                        .snapshot_into(&path)
                        .map_err(|error| error.to_string())?;
                    return Ok(Self { path });
                }
                Err(error) => return Err(format!("failed to inspect sync snapshot path: {error}")),
            }
        }
        Err("failed to allocate a unique sync database snapshot".to_string())
    }
}

impl Drop for TemporaryDatabaseSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(format!("{}-wal", self.path.display()));
        let _ = fs::remove_file(format!("{}-shm", self.path.display()));
    }
}

fn get_verified_object_to_file(
    store: &impl ObjectStore,
    key: &str,
    expected_sha256: &str,
    expected_size: Option<u64>,
    directory: &std::path::Path,
    result: &mut SyncEngineResult,
) -> Result<TemporarySyncFile, String> {
    let temporary = TemporarySyncFile::new(directory, "download")?;
    fs::remove_file(&temporary.path)
        .map_err(|error| format!("failed to prepare sync download file: {error}"))?;
    let max_bytes = expected_size.unwrap_or(MAX_LARGE_PACK_STORED_BYTES);
    if max_bytes > MAX_LARGE_PACK_STORED_BYTES {
        return Err(format!(
            "remote object {key:?} exceeds the sync pack size limit"
        ));
    }
    let downloaded = store
        .get_to_file(key, &temporary.path, max_bytes)?
        .ok_or_else(|| format!("remote object {key:?} does not exist"))?;
    if expected_size.is_some_and(|size| size != downloaded.size_bytes) {
        return Err(format!(
            "remote object {key:?} size does not match its reference"
        ));
    }
    if downloaded.sha256 != expected_sha256 {
        return Err(format!(
            "remote object {key:?} digest does not match its reference"
        ));
    }
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        downloaded.size_bytes,
        "downloaded byte count",
    )?;
    Ok(temporary)
}

fn put_immutable_file(
    store: &impl ObjectStore,
    key: &str,
    encoded: &EncodedFile,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    match store.put_file(
        key,
        encoded.path(),
        &encoded.sha256,
        encoded.stored_size_bytes,
        PutCondition::IfAbsent,
    )? {
        PutOutcome::Stored { .. } => {
            result.bytes_uploaded = checked_add(
                result.bytes_uploaded,
                encoded.stored_size_bytes,
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
                .is_some_and(|size| size != encoded.stored_size_bytes)
            {
                return Err(format!("immutable object {key:?} has an unexpected size"));
            }
            Ok(())
        }
    }
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

    use crate::sync::v1::MutationBatch;

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
                    etag: include_etags
                        .then(|| format!("\"{}\"", hex::encode(Sha256::digest(bytes)))),
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
    ) -> super::super::ReplicatedItem {
        super::super::ReplicatedItem {
            item: text_item(id, text).into(),
            version: super::super::RecordVersion {
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
        let encoded = super::super::wire::encode_checkpoint_pack(
            &directory,
            &CheckpointPackHeader {
                generation: checkpoint.generation,
                vector: checkpoint.vector.clone(),
            },
            (!checkpoint.mutations.is_empty()).then_some(checkpoint.mutations.clone()),
            None,
        )
        .unwrap();
        let key =
            super::super::checkpoint_object_key(checkpoint.generation, &encoded.sha256).unwrap();
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
        let encoded = super::super::encode_checkpoint_head(&head, None).unwrap();
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

        let materialized = super::super::materialize_resource(
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
            mutations: super::super::MutationBatch {
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

        let result =
            sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

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
            mutations: super::super::MutationBatch {
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
            mutations: super::super::MutationBatch {
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

        let idle =
            sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();

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
            mutations: super::super::MutationBatch {
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

        assert_eq!(prune_unreferenced_checkpoints(&store, &head).unwrap(), 1);

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
                &checkpoint_digest_for_generation(
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
        let encoded_snapshot = super::super::wire::encode_snapshot_pack(
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
                mutations: super::super::MutationBatch {
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

        let deleted = garbage_collect_covered_history(
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
            decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None)
                .unwrap();
        assert_eq!(advanced_head.published_sequence, 1);
        drop(database);

        let restored = Database::open(&backup_path).unwrap();
        restored
            .save_item(&text_item("local-after-restore", "local-after-restore"))
            .unwrap();
        let healed =
            sync_database(&store, &restored, &paths, REMOTE_SCOPE, None, options()).unwrap();

        assert!(healed.downloaded_entries >= 2);
        assert_eq!(healed.uploaded_entries, 3);
        for id in ["initial", "remote-only", "local-after-restore"] {
            assert!(
                restored.get_item(id).unwrap().is_some(),
                "missing item {id}"
            );
        }
        let replacement_head =
            decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None)
                .unwrap();
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
            decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None)
                .unwrap();
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
            decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None)
                .unwrap();
        assert_ne!(replacement_head.epoch, advanced_head.epoch);
        assert_eq!(replacement_head.snapshot.record_count, 2);

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
