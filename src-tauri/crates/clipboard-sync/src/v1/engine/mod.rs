//! Provider-neutral v1 replication engine, split by concern.
//!
//! This module keeps the engine entry points, the shared result/option types
//! and the small helpers every phase needs (`checked_add`, `current_time_ms`,
//! `TemporarySyncFile`, head-cache helpers). `transfer.rs` owns the
//! object-store primitives, `checkpoint.rs` the checkpoint/compaction/GC
//! machinery, `publish.rs` the local device head/bootstrap/segment publication,
//! and `pull.rs` the remote device/segment/snapshot pull paths.

use std::{collections::BTreeMap, fs, path::PathBuf};

use crate::cancellation::{self, CancellationToken};
use sha2::{Digest, Sha256};

use super::wire::envelope_is_encrypted;
use super::{
    cleanup_obsolete_objects, collect_mutation_resource_refs, decode_checkpoint_head,
    decode_device_head, decode_segment, defer_mutation_resources, encode_device_head,
    encode_segment, head_object_key, large_pack_chunk_raw_budget_bytes,
    mutation_batch_encoded_size, open_checkpoint_pack, open_snapshot_pack, parse_checkpoint_key,
    parse_head_key, parse_segment_key, prepare_mutation_resources, segment_object_key,
    segment_prefix, snapshot_object_key, CheckpointHead, CheckpointPackHeader, DeviceCursor,
    DeviceHead, EncodedFile, EncodedObject, LargePackKind, LargePackWriter, MutationBatch,
    ObjectInfo, ObjectRef, ObjectStore, PutCondition, PutOutcome, ResourceLimits, Segment,
    SessionKey, SnapshotPackHeader, SyncEnginePaths, SyncHeadCache, SyncOutboxBatch,
    SyncRemoteState, SyncRepository, SyncSnapshotExport, CHECKPOINT_HEAD_KEY, HEADS_PREFIX,
};

const CHECKPOINT_SEQUENCE_DELTA_THRESHOLD: u64 = 50_000;
const LARGE_PACK_BATCH_ENTRIES: usize = 2048;
const MAX_LARGE_PACK_STORED_BYTES: u64 = 1024 * 1024 * 1024;

mod checkpoint;
mod publish;
mod pull;
mod transfer;

use checkpoint::*;
use publish::*;
use pull::*;
use transfer::*;

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
    pub skipped_resources: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPhase {
    Preparing,
    Discovering,
    Uploading,
    Downloading,
    Compacting,
}

/// Runs one complete S3/object-store synchronization pass. Publication is
/// upload-first; immutable packs and resources become visible through the
/// device head only after their writes succeed. Pull cursors advance in the
/// same SQLite transaction as the corresponding mutation batch.
#[allow(clippy::too_many_arguments)]
pub fn sync_database_cancellable(
    store: &impl ObjectStore,
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    options: SyncEngineOptions,
    cancellation: &CancellationToken,
) -> Result<SyncEngineResult, String> {
    cancellation.run(|| sync_database(store, database, paths, remote_scope, session_key, options))
}

pub fn sync_database(
    store: &impl ObjectStore,
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    options: SyncEngineOptions,
) -> Result<SyncEngineResult, String> {
    sync_database_with_progress(
        store,
        database,
        paths,
        remote_scope,
        session_key,
        options,
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub fn sync_database_with_progress(
    store: &impl ObjectStore,
    database: &impl SyncRepository,
    paths: &SyncEnginePaths,
    remote_scope: &str,
    session_key: Option<&SessionKey>,
    options: SyncEngineOptions,
    mut progress: impl FnMut(SyncPhase),
) -> Result<SyncEngineResult, String> {
    progress(SyncPhase::Preparing);
    cancellation::check()?;
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

    progress(SyncPhase::Discovering);
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

    progress(SyncPhase::Uploading);
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

    state = adopt_orphan_segments(
        store,
        database,
        remote_scope,
        &device_id,
        state,
        session_key,
        &mut result,
    )?;

    while let Some(batch) =
        database.get_sync_outbox_batch_for_scope(remote_scope, options.segment_max_entries)?
    {
        cancellation::check()?;
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

    progress(SyncPhase::Downloading);
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
    progress(SyncPhase::Compacting);
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
    cancellation::check()?;
    Ok(result)
}

fn validate_remote_access_before_first_publish(
    store: &impl ObjectStore,
    state: &SyncRemoteState,
    heads: &[ObjectInfo],
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    cancellation::check()?;
    if state.initialized {
        return Ok(());
    }

    let mut canonical_pointers = 0u64;
    let mut valid_pointers = 0u64;
    let mut last_error = None::<String>;
    for info in heads {
        cancellation::check()?;
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

fn remove_obsolete_local_manifest(
    temporary_directory: &std::path::Path,
    remote_scope: &str,
) -> Result<(), String> {
    cancellation::check()?;
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
    cancellation::check()?;
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
