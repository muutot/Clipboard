use std::{fs, path::Path, time::Duration};

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
        for suffix in [
            "",
            "-wal",
            "-shm",
            ".resource-writers.lock",
            ".resource-generation",
        ] {
            let mut path = self.path.as_os_str().to_os_string();
            path.push(suffix);
            let _ = fs::remove_file(Path::new(&path));
        }
    }
}

/// Removes point-in-time sync snapshots left behind by a crash or hard kill.
/// A live snapshot is created and consumed quickly, so an hour-old file under
/// the data directory cannot belong to the current run.
pub(crate) fn cleanup_stale_sync_snapshots(
    directory: &Path,
    max_age: Duration,
) -> Result<u64, StorageError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut removed = 0u64;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if !name
            .to_string_lossy()
            .starts_with(".sync-database-snapshot-")
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(age) = metadata
            .modified()
            .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
        else {
            continue;
        };
        if age < max_age {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(removed)
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

    fn sync_outbox_range_matches_mutations(
        &self,
        remote_scope: &str,
        after_sequence: u64,
        through_sequence: u64,
        mutations: &MutationBatch,
    ) -> Result<bool, String> {
        Database::sync_outbox_range_matches_mutations(
            self,
            remote_scope,
            after_sequence,
            through_sequence,
            mutations,
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
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn temporary_snapshot_removes_its_resource_coordination_files() {
        let directory = std::env::temp_dir().join(format!(
            "clipboard-snapshot-sidecars-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&directory).unwrap();
        let database = Database::open_in_memory().unwrap();
        let snapshot = TemporaryDatabaseSnapshot::create(&database, &directory).unwrap();
        let snapshot_database = Database::open(&snapshot.path).unwrap();
        assert!(fs::read_dir(&directory).unwrap().count() >= 3);
        drop(snapshot_database);
        drop(snapshot);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn removes_only_stale_sync_snapshots() {
        let directory = std::env::temp_dir().join(format!(
            "clipboard-sync-snapshot-cleanup-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&directory).unwrap();
        let stale = directory.join(".sync-database-snapshot-1-0000000000000001.sqlite3");
        let stale_wal = directory.join(".sync-database-snapshot-1-0000000000000001.sqlite3-wal");
        let fresh = directory.join(".sync-database-snapshot-2-0000000000000002.sqlite3");
        let unrelated = directory.join("history.sqlite3");
        for path in [&stale, &stale_wal, &fresh, &unrelated] {
            fs::write(path, b"fixture").unwrap();
        }
        let old = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        for path in [&stale, &stale_wal] {
            let file = fs::OpenOptions::new().write(true).open(path).unwrap();
            file.set_modified(old).unwrap();
        }

        assert_eq!(
            cleanup_stale_sync_snapshots(&directory, Duration::from_secs(60 * 60)).unwrap(),
            2
        );
        assert!(!stale.exists());
        assert!(!stale_wal.exists());
        assert!(fresh.exists());
        assert!(unrelated.exists());

        fs::remove_dir_all(&directory).unwrap();
    }
}
