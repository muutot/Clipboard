//! Local device head, bootstrap and segment publication.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn reconcile_local_device_head(
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
    cancellation::check()?;
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
    cancellation::check()?;
    let Some(cache) = database.get_sync_head_cache(remote_scope, device_id)? else {
        return Ok(false);
    };
    let head = state.device_head(device_id)?;
    Ok(cache_matches_listing(&cache, info) && cache.matches_head(&head))
}

pub(super) fn peer_head_cache_matches(
    database: &impl SyncRepository,
    remote_scope: &str,
    device_id: &str,
    info: &ObjectInfo,
) -> Result<bool, String> {
    cancellation::check()?;
    let Some(cache) = database.get_sync_head_cache(remote_scope, device_id)? else {
        return Ok(false);
    };
    let Some(cursor) = database.get_sync_cursor(remote_scope, device_id)? else {
        return Ok(false);
    };
    Ok(cache_matches_listing(&cache, info) && cache.matches_cursor(&cursor))
}

pub(super) fn record_head_cache(
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
        if let Err(error) = database.record_sync_head_cache(
            remote_scope,
            device_id,
            etag,
            stored_size_bytes,
            modified_ms,
            head,
        ) {
            // Best-effort cache: a persistent failure only costs repeated head
            // downloads, but it must not be invisible.
            eprintln!("[sync] failed to record head cache for {device_id}: {error}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn publish_bootstrap(
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
    cancellation::check()?;
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
    cancellation::check()?;
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
pub(super) fn adopt_orphan_segments(
    store: &impl ObjectStore,
    database: &impl SyncRepository,
    remote_scope: &str,
    device_id: &str,
    mut state: SyncRemoteState,
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<SyncRemoteState, String> {
    cancellation::check()?;
    if !state.initialized {
        return Ok(state);
    }
    let prefix = segment_prefix(device_id, &state.epoch)?;
    loop {
        cancellation::check()?;
        let listed = store.list(&prefix, None)?;
        // A retry that encoded a longer segment than an earlier orphan leaves
        // a partially overlapping leftover: once the shorter orphan is
        // adopted, such an object can never continue the contiguous chain
        // peers require, so remove it before looking for adoptable orphans.
        for info in &listed {
            cancellation::check()?;
            let Ok(parsed) = parse_segment_key(&info.key) else {
                continue;
            };
            if parsed.first_sequence <= state.published_sequence
                && parsed.last_sequence > state.published_sequence
            {
                store.delete(&info.key)?;
                result.deleted_remote_objects =
                    checked_add(result.deleted_remote_objects, 1, "deleted object count")?;
            }
        }
        let expected_first = state
            .published_sequence
            .checked_add(1)
            .ok_or_else(|| "sync published sequence overflowed".to_string())?;
        let candidate = listed
            .iter()
            .filter_map(|info| {
                parse_segment_key(&info.key)
                    .ok()
                    .map(|parsed| (parsed, info.size_bytes))
            })
            .filter(|(parsed, _)| parsed.first_sequence == expected_first)
            .min_by_key(|(parsed, _)| parsed.last_sequence);
        let Some((candidate, stored_size)) = candidate else {
            return Ok(state);
        };
        let key = segment_object_key(
            device_id,
            &state.epoch,
            candidate.first_sequence,
            candidate.last_sequence,
            &candidate.sha256,
        )?;
        // The orphan was uploaded by an interrupted publish that never
        // reached `publish_head`, so its mutations still match the outbox
        // rows (those are only deleted by the commit below). Publishing the
        // head for it and committing drains the outbox through the orphan
        // instead of re-encoding a new overlapping segment.
        let bytes = get_verified_object(store, &key, &candidate.sha256, stored_size, result)?;
        let segment = decode_segment(&bytes, session_key)?;
        if segment.device_id != device_id
            || segment.epoch != state.epoch
            || segment.first_sequence != expected_first
            || segment.last_sequence != candidate.last_sequence
            || segment.mutations.is_empty()
        {
            return Err(format!(
                "orphan segment {key:?} does not match its key layout"
            ));
        }
        let resource_refs = collect_mutation_resource_refs(&segment.mutations)?;
        cancellation::check()?;
        database.record_sync_resource_refs(remote_scope, &segment.mutations, &resource_refs)?;
        result.uploaded_entries = checked_add(
            result.uploaded_entries,
            segment.mutations.len() as u64,
            "uploaded entry count",
        )?;
        let mut next_state = state.clone();
        next_state.published_sequence = segment.last_sequence;
        next_state.last_segment_key = Some(key.clone());
        next_state.updated_at_ms = current_time_ms();
        let head = next_state.device_head(device_id)?;
        let published_head = publish_head(store, &head, session_key, result)?;
        cancellation::check()?;
        state = database.commit_sync_segment_published(
            remote_scope,
            &state.epoch,
            &key,
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
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn publish_segment(
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
    cancellation::check()?;
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
    result.uploaded_resources = checked_add(
        result.uploaded_resources,
        resources.transferred_resources,
        "uploaded resource count",
    )?;
    result.skipped_resources = checked_add(
        result.skipped_resources,
        resources.skipped_resources,
        "skipped resource count",
    )?;
    result.bytes_uploaded = checked_add(
        result.bytes_uploaded,
        resources.transferred_bytes,
        "uploaded byte count",
    )?;
    let resource_refs = collect_mutation_resource_refs(&segment.mutations)?;
    cancellation::check()?;
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
    cancellation::check()?;
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
