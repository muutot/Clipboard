use std::{collections::BTreeMap, fs, path::PathBuf};

use sha2::{Digest, Sha256};

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
    SnapshotPackHeader, SyncEnginePaths, SyncHeadCache, SyncOutboxBatch, SyncRemoteState,
    SyncRepository, SyncSnapshotExport, CHECKPOINT_HEAD_KEY, HEADS_PREFIX,
};

const CHECKPOINT_SEQUENCE_DELTA_THRESHOLD: u64 = 50_000;
const LARGE_PACK_BATCH_ENTRIES: usize = 2048;
const MAX_LARGE_PACK_STORED_BYTES: u64 = 1024 * 1024 * 1024;

fn write_large_pack_batch(
    writer: &mut LargePackWriter<'_>,
    mutations: MutationBatch,
) -> Result<(), String> {
    if mutations.is_empty() {
        return Ok(());
    }
    let encoded_size = mutation_batch_encoded_size(&mutations)?;
    if encoded_size <= large_pack_chunk_limit_bytes() {
        return writer.write_batch(&mutations);
    }
    if mutations.len() == 1 {
        return Err("one sync record exceeds the large-pack chunk size limit".to_string());
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    options: SyncEngineOptions,
) -> Result<SyncEngineResult, String> {
    if options.segment_max_entries == 0 {
        return Err("sync segment entry limit must be greater than zero".to_string());
    }
    database.initialize_sync()?;
    let device_id = database.get_sync_device_id()?;
    let mut result = SyncEngineResult::default();
    let mut state = database.get_or_create_sync_remote_state(remote_scope)?;

    if !state.remote_prepared {
        let cleanup = cleanup_obsolete_objects(store)?;
        result.deleted_remote_objects = cleanup.deleted_objects;
        remove_obsolete_local_manifest(&paths.temporary_directory, remote_scope)?;
        database.mark_sync_remote_prepared(remote_scope)?;
        state = database.get_or_create_sync_remote_state(remote_scope)?;
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

    while let Some(batch) =
        database.get_sync_outbox_batch_for_scope(remote_scope, options.segment_max_entries)?
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
        state = database.get_or_create_sync_remote_state(remote_scope)?;
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
    state: &SyncRemoteState,
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    device_id: &str,
    state: SyncRemoteState,
    heads: &[ObjectInfo],
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<SyncRemoteState, String> {
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
            return database.reset_sync_remote_state(remote_scope);
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
        .get_sync_cursor(remote_scope, device_id)?
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

    database.reset_sync_remote_state(remote_scope)
}

fn local_publication_matches_remote(state: &SyncRemoteState, head: &DeviceHead) -> bool {
    state.initialized
        && state.epoch == head.epoch
        && state.snapshot.as_ref() == Some(&head.snapshot)
        && state.published_sequence == head.published_sequence
        && state.last_segment_key == head.last_segment_key
}

fn listed_head_identity(info: &ObjectInfo) -> Option<(&str, u64)> {
    Some((info.etag.as_deref()?, info.size_bytes?))
}

fn cache_matches_listing(cache: &SyncHeadCache, info: &ObjectInfo) -> bool {
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
    database: &impl SyncRepository,
    remote_scope: &str,
    device_id: &str,
    state: &SyncRemoteState,
    info: &ObjectInfo,
) -> Result<bool, String> {
    let Some(cache) = database.get_sync_head_cache(remote_scope, device_id)? else {
        return Ok(false);
    };
    let head = state.device_head(device_id)?;
    Ok(cache_matches_listing(&cache, info) && cache.matches_head(&head))
}

fn peer_head_cache_matches(
    database: &impl SyncRepository,
    remote_scope: &str,
    device_id: &str,
    info: &ObjectInfo,
) -> Result<bool, String> {
    let Some(cache) = database.get_sync_head_cache(remote_scope, device_id)? else {
        return Ok(false);
    };
    let Some(cursor) = database.get_sync_cursor(remote_scope, device_id)? else {
        return Ok(false);
    };
    Ok(cache_matches_listing(&cache, info) && cache.matches_cursor(&cursor))
}

fn record_head_cache(
    database: &impl SyncRepository,
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    _resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
    force: bool,
) -> Result<bool, String> {
    if !force {
        let has_checkpoint = database.get_sync_checkpoint_state(remote_scope)?.is_some();
        let has_peer_cursors = !database.list_sync_cursors(remote_scope)?.is_empty();
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
            .get_sync_checkpoint_state(remote_scope)?
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
        &paths.temporary_directory,
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
                &paths.temporary_directory,
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
    let mut batches = std::iter::from_fn(|| {
        if terminal_checked {
            return None;
        }
        match reader.next() {
            Some(batch) => Some(batch.and_then(|mut mutations| {
                let resource_refs = defer_mutation_resources(&mut mutations)?;
                Ok((mutations, resource_refs))
            })),
            None => {
                terminal_checked = true;
                if reader.record_count() != expected_record_count || !reader.is_complete() {
                    Some(Err(
                        "checkpoint payload does not match its reference".to_string()
                    ))
                } else {
                    None
                }
            }
        }
    });
    let applied = database.apply_sync_checkpoint_batches(
        remote_scope,
        generation,
        &checkpoint_digest_for_generation(&checkpoint_head, generation)?,
        &vector,
        &mut batches,
    )?;
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
    temporary_directory: &std::path::Path,
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
        temporary_directory,
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    kind: LargePackKind,
    snapshot_header: Option<&SnapshotPackHeader>,
    checkpoint_header: Option<&CheckpointPackHeader>,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(EncodedFile, SyncSnapshotExport), String> {
    let mut writer = match kind {
        LargePackKind::Snapshot => LargePackWriter::new(
            &paths.temporary_directory,
            kind,
            snapshot_header.ok_or_else(|| "snapshot pack header is missing".to_string())?,
            session_key,
        )?,
        LargePackKind::Checkpoint => LargePackWriter::new(
            &paths.temporary_directory,
            kind,
            checkpoint_header.ok_or_else(|| "checkpoint pack header is missing".to_string())?,
            session_key,
        )?,
    };
    let export = database.visit_sync_snapshot_for_scope(
        remote_scope,
        LARGE_PACK_BATCH_ENTRIES,
        &paths.temporary_directory,
        &mut |mut mutations| {
            let resources = prepare_mutation_resources(
                store,
                &mut mutations,
                &paths.resource_roots,
                resource_limits,
                session_key,
            )?;
            result.uploaded_resources = result
                .uploaded_resources
                .checked_add(resources.transferred_resources)
                .ok_or_else(|| "uploaded sync resource count overflowed".to_string())?;
            result.bytes_uploaded = result
                .bytes_uploaded
                .checked_add(resources.transferred_bytes)
                .ok_or_else(|| "uploaded sync byte count overflowed".to_string())?;
            let resource_refs = collect_mutation_resource_refs(&mutations)?;
            database.record_sync_resource_refs(remote_scope, &mutations, &resource_refs)?;
            write_large_pack_batch(&mut writer, mutations)
        },
    )?;
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    local_device_id: &str,
    local_state: &SyncRemoteState,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    let vector = frozen_checkpoint_vector(database, remote_scope, local_device_id, local_state)?;
    let baseline = database.get_sync_checkpoint_cursors(remote_scope)?;
    if vector.is_empty() || !checkpoint_compaction_due(&baseline, &vector) {
        return Ok(());
    }
    let existing = read_checkpoint_head(store, session_key, result)?;

    if let Some(current) = existing.as_ref() {
        let current_is_locally_verified = database
            .get_sync_checkpoint_state(remote_scope)?
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
                &paths.temporary_directory,
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
                &paths.temporary_directory,
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
        &paths.temporary_directory,
        result,
    )
}

#[allow(clippy::too_many_arguments)]
fn finalize_checkpoint_publication(
    store: &impl ObjectStore,
    database: &impl SyncRepository,
    remote_scope: &str,
    head: &CheckpointHead,
    covered_vector_hint: Option<&[DeviceCursor]>,
    session_key: Option<&SessionKey>,
    temporary_directory: &std::path::Path,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    if let Some(previous) = head.previous_checkpoint.as_ref() {
        let previous_generation = parse_checkpoint_key(&previous.key)?.generation;
        let covered_vector = if let Some(vector) = covered_vector_hint {
            vector.to_vec()
        } else {
            let local_state = database.get_sync_checkpoint_state(remote_scope)?;
            let local_vector = if local_state.is_some_and(|(generation, sha256)| {
                generation == previous_generation && sha256 == previous.sha256
            }) {
                database.get_sync_checkpoint_cursors(remote_scope)?
            } else {
                Vec::new()
            };
            if local_vector.is_empty() {
                download_checkpoint(
                    store,
                    previous,
                    previous_generation,
                    session_key,
                    temporary_directory,
                    result,
                )?
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
    database.record_sync_checkpoint_published(
        remote_scope,
        head.generation,
        &head.checkpoint.sha256,
        &head.vector,
    )
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
    database: &impl SyncRepository,
    remote_scope: &str,
    local_device_id: &str,
    local_state: &SyncRemoteState,
) -> Result<Vec<DeviceCursor>, String> {
    let mut vector = database
        .list_sync_cursors(remote_scope)?
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    device_id: &str,
    state: SyncRemoteState,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<SyncRemoteState, String> {
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
    let state = database.commit_sync_bootstrap_published(
        remote_scope,
        &state.epoch,
        &snapshot_ref,
        export.through_sequence,
    )?;
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    device_id: &str,
    state: SyncRemoteState,
    batch: SyncOutboxBatch,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<SyncRemoteState, String> {
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
        &paths.resource_roots,
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
    database.record_sync_resource_refs(remote_scope, &segment.mutations, &resource_refs)?;

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
    let head = next_state.device_head(device_id)?;
    let published_head = publish_head(store, &head, session_key, result)?;
    result.uploaded_entries = checked_add(
        result.uploaded_entries,
        segment.mutations.len() as u64,
        "uploaded entry count",
    )?;
    let state = database.commit_sync_segment_published(
        remote_scope,
        &state.epoch,
        &segment_key,
        segment.last_sequence,
    )?;
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    let mut cursor = database.get_sync_cursor(remote_scope, &head.device_id)?;
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
        let applied = database.apply_sync_segment_with_resources(
            remote_scope,
            &next_cursor,
            &segment.mutations,
            &resource_refs,
        )?;
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
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
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
        &paths.temporary_directory,
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
    let mut batches = std::iter::from_fn(|| {
        if terminal_checked {
            return None;
        }
        match reader.next() {
            Some(batch) => Some(batch.and_then(|mut mutations| {
                let resource_refs = defer_mutation_resources(&mut mutations)?;
                Ok((mutations, resource_refs))
            })),
            None => {
                terminal_checked = true;
                if reader.record_count() != expected_record_count || !reader.is_complete() {
                    Some(Err(format!(
                        "snapshot payload does not match head for {}",
                        head.device_id
                    )))
                } else {
                    None
                }
            }
        }
    });
    let applied = database.apply_sync_snapshot_batches(
        remote_scope,
        &cursor,
        &head.snapshot.sha256,
        &mut batches,
    )?;
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

fn remove_obsolete_local_manifest(
    temporary_directory: &std::path::Path,
    remote_scope: &str,
) -> Result<(), String> {
    let path = temporary_directory.join(format!("sync-pool-manifest-{remote_scope}.json"));
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

#[cfg(feature = "engine-test-support")]
pub mod test_support {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    pub fn maybe_compact(
        store: &impl ObjectStore,
        database: &impl SyncRepository,
        paths: &SyncEnginePaths,
        remote_scope: &str,
        local_device_id: &str,
        local_state: &SyncRemoteState,
        session_key: Option<&SessionKey>,
        resource_limits: ResourceLimits,
        result: &mut SyncEngineResult,
    ) -> Result<(), String> {
        super::maybe_compact(
            store,
            database,
            paths,
            remote_scope,
            local_device_id,
            local_state,
            session_key,
            resource_limits,
            result,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn pull_device(
        store: &impl ObjectStore,
        database: &impl SyncRepository,
        paths: &SyncEnginePaths,
        remote_scope: &str,
        head: &DeviceHead,
        session_key: Option<&SessionKey>,
        resource_limits: ResourceLimits,
        result: &mut SyncEngineResult,
    ) -> Result<(), String> {
        super::pull_device(
            store,
            database,
            paths,
            remote_scope,
            head,
            session_key,
            resource_limits,
            result,
        )
    }

    pub fn checkpoint_digest_for_generation(
        head: &CheckpointHead,
        generation: u64,
    ) -> Result<String, String> {
        super::checkpoint_digest_for_generation(head, generation)
    }

    pub fn prune_unreferenced_checkpoints(
        store: &impl ObjectStore,
        head: &CheckpointHead,
    ) -> Result<u64, String> {
        super::prune_unreferenced_checkpoints(store, head)
    }

    pub fn garbage_collect_covered_history(
        store: &impl ObjectStore,
        covered_vector: &[DeviceCursor],
    ) -> Result<u64, String> {
        super::garbage_collect_covered_history(store, covered_vector)
    }
}
