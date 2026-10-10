//! Checkpoint pull, compaction, publication and garbage collection.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn pull_checkpoint_if_needed(
    store: &impl ObjectStore,
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    _resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
    force: bool,
) -> Result<bool, String> {
    cancellation::check()?;
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
    let mut batches = cancellation::checked_batches(std::iter::from_fn(|| {
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
    }));
    cancellation::check()?;
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

pub(super) fn checkpoint_digest_for_generation(
    head: &CheckpointHead,
    generation: u64,
) -> Result<String, String> {
    cancellation::check()?;
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
    cancellation::check()?;
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

pub(super) fn validate_checkpoint_head(head: &CheckpointHead) -> Result<(), String> {
    cancellation::check()?;
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
    cancellation::check()?;
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
pub(super) fn encode_database_pack(
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
    cancellation::check()?;
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
            cancellation::check()?;
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
            result.skipped_resources = result
                .skipped_resources
                .checked_add(resources.skipped_resources)
                .ok_or_else(|| "skipped sync resource count overflowed".to_string())?;
            result.bytes_uploaded = result
                .bytes_uploaded
                .checked_add(resources.transferred_bytes)
                .ok_or_else(|| "uploaded sync byte count overflowed".to_string())?;
            let resource_refs = collect_mutation_resource_refs(&mutations)?;
            cancellation::check()?;
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
pub(super) fn maybe_compact(
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
    cancellation::check()?;
    let vector = frozen_checkpoint_vector(database, remote_scope, local_device_id, local_state)?;
    let baseline = database.get_sync_checkpoint_cursors(remote_scope)?;
    if vector.is_empty()
        || !checkpoint_compaction_due(
            database,
            remote_scope,
            local_device_id,
            local_state,
            &baseline,
            &vector,
        )?
    {
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
    let key = crate::v1::checkpoint_object_key(generation, &encoded.sha256)?;
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
    let encoded_head = crate::v1::encode_checkpoint_head(&head, session_key)?;
    let encoded_head_size = encoded_head.stored_size_bytes();
    let expected_head_bytes = encoded_head.bytes;
    let condition = existing.as_ref().map_or(PutCondition::IfAbsent, |current| {
        // Stores that omit ETags (some S3-compatible gateways) cannot serve a
        // compare-and-swap: fall back to an unconditional put instead of
        // failing every compaction. The pointer is re-downloaded and verified
        // right after (checkpoint_pointer_is_durable), so a clobbered write is
        // detected rather than silently trusted.
        current
            .etag
            .clone()
            .map_or(PutCondition::Unconditional, PutCondition::IfMatch)
    });
    match store.put(CHECKPOINT_HEAD_KEY, expected_head_bytes.clone(), condition)? {
        PutOutcome::PreconditionFailed => return Ok(()),
        PutOutcome::Stored { .. } => {
            result.bytes_uploaded = checked_add(
                result.bytes_uploaded,
                encoded_head_size,
                "uploaded byte count",
            )?;
        }
    }
    if !checkpoint_pointer_is_durable(store, &head, &expected_head_bytes, session_key, result)? {
        return Ok(());
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

fn checkpoint_pointer_is_durable(
    store: &impl ObjectStore,
    expected: &CheckpointHead,
    expected_bytes: &[u8],
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<bool, String> {
    cancellation::check()?;
    let downloaded = store.get(CHECKPOINT_HEAD_KEY)?.ok_or_else(|| {
        "checkpoint pointer disappeared after a successful conditional write".to_string()
    })?;
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        downloaded.bytes.len() as u64,
        "downloaded byte count",
    )?;
    if downloaded.bytes == expected_bytes {
        return Ok(true);
    }

    let observed = decode_checkpoint_head(&downloaded.bytes, session_key)?;
    validate_checkpoint_head(&observed)?;
    if observed.generation > expected.generation {
        validate_compaction_vector(&expected.vector, &observed.vector)?;
        return Ok(false);
    }
    Err("checkpoint pointer changed unexpectedly after a successful conditional write".to_string())
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
    cancellation::check()?;
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
    cancellation::check()?;
    database.record_sync_checkpoint_published(
        remote_scope,
        head.generation,
        &head.checkpoint.sha256,
        &head.vector,
    )
}

pub(super) fn prune_unreferenced_checkpoints(
    store: &impl ObjectStore,
    head: &CheckpointHead,
) -> Result<u64, String> {
    cancellation::check()?;
    let mut retained = vec![head.checkpoint.key.as_str()];
    if let Some(previous) = head.previous_checkpoint.as_ref() {
        retained.push(previous.key.as_str());
    }
    let mut deleted = 0u64;
    for object in store.list("v1/checkpoints/", None)? {
        cancellation::check()?;
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
    /// `None` when the store does not report ETags (some S3-compatible
    /// gateways). Compaction then degrades to an unconditional put instead of
    /// failing; see the put condition in `maybe_compact`.
    etag: Option<String>,
}

fn read_checkpoint_head(
    store: &impl ObjectStore,
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<Option<StoredCheckpointHead>, String> {
    cancellation::check()?;
    let Some(downloaded) = store.get(CHECKPOINT_HEAD_KEY)? else {
        return Ok(None);
    };
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        downloaded.bytes.len() as u64,
        "downloaded byte count",
    )?;
    // An absent ETag must not fail the run: several S3-compatible stores omit
    // the header. Compaction degrades to an unconditional put (see
    // maybe_compact) and re-verifies the pointer afterwards.
    let etag = downloaded.etag;
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
    cancellation::check()?;
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

fn checkpoint_compaction_due(
    database: &impl SyncRepository,
    remote_scope: &str,
    local_device_id: &str,
    local_state: &SyncRemoteState,
    baseline: &[DeviceCursor],
    vector: &[DeviceCursor],
) -> Result<bool, String> {
    cancellation::check()?;
    if baseline.is_empty() {
        return Ok(true);
    }
    let previous = baseline
        .iter()
        .map(|cursor| (cursor.device_id.as_str(), cursor))
        .collect::<BTreeMap<_, _>>();
    if previous
        .keys()
        .any(|device_id| !vector.iter().any(|cursor| cursor.device_id == *device_id))
    {
        return Ok(true);
    }
    let mut delta = 0u64;
    for cursor in vector {
        if let Some(old) = previous.get(cursor.device_id.as_str()) {
            if old.epoch != cursor.epoch || old.sequence > cursor.sequence {
                return Ok(true);
            }
            delta = delta.saturating_add(cursor.sequence - old.sequence);
            continue;
        }

        let snapshot_records = if cursor.device_id == local_device_id {
            local_state
                .snapshot
                .as_ref()
                .filter(|_| {
                    local_state.epoch == cursor.epoch
                        && local_state.published_sequence == cursor.sequence
                        && local_state.last_segment_key == cursor.last_segment_key
                })
                .map(|snapshot| snapshot.record_count)
        } else {
            database
                .get_sync_head_cache(remote_scope, &cursor.device_id)?
                .filter(|cache| cache.matches_cursor(cursor))
                .map(|cache| cache.snapshot_record_count)
        };
        let Some(snapshot_records) = snapshot_records else {
            return Ok(true);
        };
        delta = delta.saturating_add(snapshot_records.max(cursor.sequence));
    }
    Ok(delta >= CHECKPOINT_SEQUENCE_DELTA_THRESHOLD)
}

fn validate_compaction_vector(
    current: &[DeviceCursor],
    candidate: &[DeviceCursor],
) -> Result<(), String> {
    cancellation::check()?;
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

pub(super) fn garbage_collect_covered_history(
    store: &impl ObjectStore,
    covered_vector: &[DeviceCursor],
) -> Result<u64, String> {
    cancellation::check()?;
    let mut deleted = 0u64;
    for cursor in covered_vector {
        cancellation::check()?;
        let snapshot_prefix = format!("v1/snapshots/{}/{}/", cursor.device_id, cursor.epoch);
        for object in store.list(&snapshot_prefix, None)? {
            cancellation::check()?;
            store.delete(&object.key)?;
            deleted = checked_add(deleted, 1, "deleted remote object count")?;
        }
        let prefix = segment_prefix(&cursor.device_id, &cursor.epoch)?;
        for object in store.list(&prefix, None)? {
            cancellation::check()?;
            // Tolerate non-canonical keys under the prefix (directory markers,
            // partial or foreign uploads) instead of aborting the whole
            // compaction pass; `prune_unreferenced_checkpoints` skips the same
            // way. An unparseable key is left untouched for a later pass.
            let Ok(segment) = parse_segment_key(&object.key) else {
                continue;
            };
            if segment.last_sequence <= cursor.sequence {
                store.delete(&object.key)?;
                deleted = checked_add(deleted, 1, "deleted remote object count")?;
            }
        }
    }
    Ok(deleted)
}
