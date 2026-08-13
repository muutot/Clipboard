use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{DeviceCursor, DeviceHead, MutationBatch, ObjectRef, ResourceRoots, SyncResourceRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncEnginePaths {
    pub temporary_directory: PathBuf,
    pub resource_roots: ResourceRoots,
}

impl SyncEnginePaths {
    pub fn new(temporary_directory: PathBuf, resource_roots: ResourceRoots) -> Self {
        Self {
            temporary_directory,
            resource_roots,
        }
    }
}

pub type SyncResourceReferences = BTreeMap<String, Vec<SyncResourceRef>>;
pub type SyncIncomingBatch = Result<(MutationBatch, SyncResourceReferences), String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncHeadCache {
    pub etag: String,
    pub stored_size_bytes: u64,
    pub modified_ms: Option<i64>,
    pub epoch: String,
    pub snapshot_key: String,
    pub snapshot_sha256: String,
    pub snapshot_size_bytes: u64,
    pub snapshot_record_count: u64,
    pub published_sequence: u64,
    pub last_segment_key: Option<String>,
}

impl SyncHeadCache {
    pub fn matches_head(&self, head: &DeviceHead) -> bool {
        self.epoch == head.epoch
            && self.snapshot_key == head.snapshot.key
            && self.snapshot_sha256 == head.snapshot.sha256
            && self.snapshot_size_bytes == head.snapshot.stored_size_bytes
            && self.snapshot_record_count == head.snapshot.record_count
            && self.published_sequence == head.published_sequence
            && self.last_segment_key == head.last_segment_key
    }

    pub fn matches_cursor(&self, cursor: &DeviceCursor) -> bool {
        self.epoch == cursor.epoch
            && self.published_sequence == cursor.sequence
            && self.last_segment_key == cursor.last_segment_key
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSnapshot {
    pub through_sequence: u64,
    pub mutations: MutationBatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncSnapshotExport {
    pub through_sequence: u64,
    pub record_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutboxBatch {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub mutations: MutationBatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRemoteState {
    pub remote_scope: String,
    pub epoch: String,
    pub snapshot: Option<ObjectRef>,
    pub snapshot_sequence: u64,
    pub published_sequence: u64,
    pub last_segment_key: Option<String>,
    pub remote_prepared: bool,
    pub initialized: bool,
    pub updated_at_ms: i64,
}

impl SyncRemoteState {
    pub fn device_head(&self, device_id: &str) -> Result<DeviceHead, String> {
        let snapshot = self
            .snapshot
            .clone()
            .ok_or_else(|| "device head has no published snapshot".to_string())?;
        Ok(DeviceHead {
            device_id: device_id.to_string(),
            epoch: self.epoch.clone(),
            snapshot,
            published_sequence: self.published_sequence,
            last_segment_key: self.last_segment_key.clone(),
            updated_at_ms: self.updated_at_ms,
        })
    }
}

/// Persistence boundary required by the provider-neutral sync engine.
///
/// Implementations retain ownership of their transaction model. In particular,
/// streamed snapshot/checkpoint application must commit all batches and cursor
/// state atomically or roll the entire operation back.
pub trait SyncRepository {
    fn initialize_sync(&self) -> Result<bool, String>;
    fn get_sync_device_id(&self) -> Result<String, String>;
    fn get_or_create_sync_remote_state(
        &self,
        remote_scope: &str,
    ) -> Result<SyncRemoteState, String>;
    fn reset_sync_remote_state(&self, remote_scope: &str) -> Result<SyncRemoteState, String>;
    fn mark_sync_remote_prepared(&self, remote_scope: &str) -> Result<(), String>;
    fn get_sync_outbox_batch_for_scope(
        &self,
        remote_scope: &str,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, String>;
    fn commit_sync_bootstrap_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        snapshot: &ObjectRef,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, String>;
    fn commit_sync_segment_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        last_segment_key: &str,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, String>;
    fn get_sync_cursor(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<DeviceCursor>, String>;
    fn list_sync_cursors(&self, remote_scope: &str) -> Result<Vec<DeviceCursor>, String>;
    fn get_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<SyncHeadCache>, String>;
    fn record_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
        etag: &str,
        stored_size_bytes: u64,
        modified_ms: Option<i64>,
        head: &DeviceHead,
    ) -> Result<(), String>;
    fn get_sync_checkpoint_state(
        &self,
        remote_scope: &str,
    ) -> Result<Option<(u64, String)>, String>;
    fn get_sync_checkpoint_cursors(&self, remote_scope: &str) -> Result<Vec<DeviceCursor>, String>;
    fn record_sync_checkpoint_published(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
    ) -> Result<(), String>;
    fn record_sync_resource_refs(
        &self,
        remote_scope: &str,
        mutations: &MutationBatch,
        resource_refs: &SyncResourceReferences,
    ) -> Result<(), String>;
    fn visit_sync_snapshot_for_scope(
        &self,
        remote_scope: &str,
        batch_size: usize,
        temporary_directory: &Path,
        visit: &mut dyn FnMut(MutationBatch) -> Result<(), String>,
    ) -> Result<SyncSnapshotExport, String>;
    fn apply_sync_checkpoint_batches(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        batches: &mut dyn Iterator<Item = SyncIncomingBatch>,
    ) -> Result<u64, String>;
    fn apply_sync_snapshot_batches(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        batches: &mut dyn Iterator<Item = SyncIncomingBatch>,
    ) -> Result<u64, String>;
    fn apply_sync_segment_with_resources(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        mutations: &MutationBatch,
        resource_refs: &SyncResourceReferences,
    ) -> Result<u64, String>;
}
