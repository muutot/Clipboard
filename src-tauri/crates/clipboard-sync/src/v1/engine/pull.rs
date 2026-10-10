//! Remote device, segment and snapshot pull paths.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn pull_device_with_checkpoint_recovery(
    store: &impl ObjectStore,
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    cancellation::check()?;
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
            )
            .map_err(|recovery_error| {
                // Preserve the original failure: recovery diagnostics alone
                // ("checkpoint is required...") would misreport a transient
                // pull error as a missing-history problem.
                format!("{original_error}; forced checkpoint recovery failed: {recovery_error}")
            })?;
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
pub(super) fn pull_remote_devices(
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
    cancellation::check()?;
    for info in heads {
        cancellation::check()?;
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
            cancellation::check()?;
            result.failed_peers = checked_add(result.failed_peers, 1, "failed peer count")?;
            eprintln!("[sync] skipped remote head {:?}: {error}", info.key);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pull_device(
    store: &impl ObjectStore,
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    resource_limits: ResourceLimits,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    cancellation::check()?;
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
        cancellation::check()?;
        if info.key.as_str() > head_last_key {
            break;
        }
        // Tolerate non-canonical keys under the prefix (directory markers,
        // foreign tool objects) exactly like GC does: one stray object must not
        // abort the whole device pull forever.
        let Ok(parsed) = parse_segment_key(&info.key) else {
            continue;
        };
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
        cancellation::check()?;
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
    cancellation::check()?;
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
                    Some(Err(format!(
                        "snapshot payload does not match head for {}",
                        head.device_id
                    )))
                } else {
                    None
                }
            }
        }
    }));
    cancellation::check()?;
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
