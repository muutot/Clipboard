use std::{fs, path::Path};

use crate::sync::v1::{
    DeviceCursor, DeviceHead, MutationBatch, ObjectRef, SyncHeadCache, SyncIncomingBatch,
    SyncOutboxBatch, SyncRemoteState, SyncRepository, SyncResourceReferences, SyncSnapshotExport,
};

use super::{Database, StorageError};

struct TemporaryDatabaseSnapshot {
    path: std::path::PathBuf,
}

impl TemporaryDatabaseSnapshot {
    fn create(database: &Database, directory: &Path) -> Result<Self, StorageError> {
        fs::create_dir_all(directory)?;
        for _ in 0..16 {
            let path = directory.join(format!(
                ".sync-database-snapshot-{}-{:016x}.sqlite3",
                std::process::id(),
                rand::random::<u64>()
            ));
            match fs::symlink_metadata(&path) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    database.snapshot_into(&path)?;
                    return Ok(Self { path });
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(StorageError::InvalidSyncState(
            "failed to allocate a unique sync database snapshot".to_string(),
        ))
    }
}

impl Drop for TemporaryDatabaseSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(format!("{}-wal", self.path.display()));
        let _ = fs::remove_file(format!("{}-shm", self.path.display()));
    }
}

fn sync_error(error: String) -> StorageError {
    StorageError::InvalidSyncState(error)
}

impl SyncRepository for Database {
    fn initialize_sync(&self) -> Result<bool, String> {
        Database::initialize_sync(self).map_err(|error| error.to_string())
    }

    fn get_sync_device_id(&self) -> Result<String, String> {
        Database::get_sync_device_id(self).map_err(|error| error.to_string())
    }

    fn get_or_create_sync_remote_state(
        &self,
        remote_scope: &str,
    ) -> Result<SyncRemoteState, String> {
        Database::get_or_create_sync_remote_state(self, remote_scope)
            .map_err(|error| error.to_string())
    }

    fn reset_sync_remote_state(&self, remote_scope: &str) -> Result<SyncRemoteState, String> {
        Database::reset_sync_remote_state(self, remote_scope).map_err(|error| error.to_string())
    }

    fn mark_sync_remote_prepared(&self, remote_scope: &str) -> Result<(), String> {
        Database::mark_sync_remote_prepared(self, remote_scope).map_err(|error| error.to_string())
    }

    fn get_sync_outbox_batch_for_scope(
        &self,
        remote_scope: &str,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, String> {
        Database::get_sync_outbox_batch_for_scope(self, remote_scope, limit)
            .map_err(|error| error.to_string())
    }

    fn commit_sync_bootstrap_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        snapshot: &ObjectRef,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, String> {
        Database::commit_sync_bootstrap_published(
            self,
            remote_scope,
            expected_epoch,
            snapshot,
            through_sequence,
        )
        .map_err(|error| error.to_string())
    }

    fn commit_sync_segment_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        last_segment_key: &str,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, String> {
        Database::commit_sync_segment_published(
            self,
            remote_scope,
            expected_epoch,
            last_segment_key,
            through_sequence,
        )
        .map_err(|error| error.to_string())
    }

    fn get_sync_cursor(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<DeviceCursor>, String> {
        Database::get_sync_cursor(self, remote_scope, device_id).map_err(|error| error.to_string())
    }

    fn list_sync_cursors(&self, remote_scope: &str) -> Result<Vec<DeviceCursor>, String> {
        Database::list_sync_cursors(self, remote_scope).map_err(|error| error.to_string())
    }

    fn get_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<SyncHeadCache>, String> {
        Database::get_sync_head_cache(self, remote_scope, device_id)
            .map_err(|error| error.to_string())
    }

    fn record_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
        etag: &str,
        stored_size_bytes: u64,
        modified_ms: Option<i64>,
        head: &DeviceHead,
    ) -> Result<(), String> {
        Database::record_sync_head_cache(
            self,
            remote_scope,
            device_id,
            etag,
            stored_size_bytes,
            modified_ms,
            head,
        )
        .map_err(|error| error.to_string())
    }

    fn get_sync_checkpoint_state(
        &self,
        remote_scope: &str,
    ) -> Result<Option<(u64, String)>, String> {
        Database::get_sync_checkpoint_state(self, remote_scope).map_err(|error| error.to_string())
    }

    fn get_sync_checkpoint_cursors(&self, remote_scope: &str) -> Result<Vec<DeviceCursor>, String> {
        Database::get_sync_checkpoint_cursors(self, remote_scope).map_err(|error| error.to_string())
    }

    fn record_sync_checkpoint_published(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
    ) -> Result<(), String> {
        Database::record_sync_checkpoint_published(
            self,
            remote_scope,
            generation,
            checkpoint_sha256,
            cursors,
        )
        .map_err(|error| error.to_string())
    }

    fn record_sync_resource_refs(
        &self,
        remote_scope: &str,
        mutations: &MutationBatch,
        resource_refs: &SyncResourceReferences,
    ) -> Result<(), String> {
        Database::record_sync_resource_refs(self, remote_scope, mutations, resource_refs)
            .map_err(|error| error.to_string())
    }

    fn visit_sync_snapshot_for_scope(
        &self,
        remote_scope: &str,
        batch_size: usize,
        temporary_directory: &Path,
        visit: &mut dyn FnMut(MutationBatch) -> Result<(), String>,
    ) -> Result<SyncSnapshotExport, String> {
        let snapshot = TemporaryDatabaseSnapshot::create(self, temporary_directory)
            .map_err(|error| error.to_string())?;
        let snapshot_database =
            Database::open(&snapshot.path).map_err(|error| error.to_string())?;
        Database::visit_sync_snapshot_for_scope(
            &snapshot_database,
            remote_scope,
            batch_size,
            |mutations| visit(mutations).map_err(sync_error),
        )
        .map_err(|error| error.to_string())
    }

    fn apply_sync_checkpoint_batches(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        batches: &mut dyn Iterator<Item = SyncIncomingBatch>,
    ) -> Result<u64, String> {
        Database::apply_sync_checkpoint_batches(
            self,
            remote_scope,
            generation,
            checkpoint_sha256,
            cursors,
            batches.map(|batch| batch.map_err(sync_error)),
        )
        .map_err(|error| error.to_string())
    }

    fn apply_sync_snapshot_batches(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        batches: &mut dyn Iterator<Item = SyncIncomingBatch>,
    ) -> Result<u64, String> {
        Database::apply_sync_snapshot_batches(
            self,
            remote_scope,
            cursor,
            snapshot_sha256,
            batches.map(|batch| batch.map_err(sync_error)),
        )
        .map_err(|error| error.to_string())
    }

    fn apply_sync_segment_with_resources(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        mutations: &MutationBatch,
        resource_refs: &SyncResourceReferences,
    ) -> Result<u64, String> {
        Database::apply_sync_segment_with_resources(
            self,
            remote_scope,
            cursor,
            mutations,
            resource_refs,
        )
        .map_err(|error| error.to_string())
    }
}
