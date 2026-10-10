//! `device` sync persistence (moved verbatim from `sync_state.rs`).

use super::*;

impl Database {
    pub fn set_sync_device_id(&self, device_id: &str) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('device_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [device_id],
            )?;
            Ok(())
        })
    }

    pub fn get_sync_device_id(&self) -> Result<String, StorageError> {
        self.with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT value FROM sync_metadata WHERE key = 'device_id'",
                [],
                |row| row.get(0),
            )?)
        })
    }

    /// Ensures one stable UUID identity for the current database. Invalid
    /// values are replaced directly; no historical identity alias is kept.
    pub fn ensure_sync_device_id(&self) -> Result<String, StorageError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'device_id'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing) = existing.as_deref() {
                if Uuid::parse_str(existing).is_ok() {
                    transaction.commit()?;
                    return Ok(existing.to_string());
                }
                // This machine will appear as a new sync peer to existing
                // devices; make the identity rotation visible in logs.
                crate::log_error!(
                    "[sync] replacing non-UUID device id ({existing:?}) with a fresh identity; \
                     existing peers will treat this device as new"
                );
            }

            let device_id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('device_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [&device_id],
            )?;
            transaction.commit()?;
            Ok(device_id)
        })
    }

    /// Enables the sole v1 replication state. Existing clipboard rows become
    /// the first local snapshot; no historical schema or sync state is read.
    pub fn initialize_sync(&self) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let current_enabled: Option<String> = transaction
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_enabled'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if current_enabled.as_deref() == Some("1") {
                transaction.commit()?;
                return Ok(false);
            }

            let device_id: String = transaction.query_row(
                "SELECT value FROM sync_metadata WHERE key = 'device_id'",
                [],
                |row| row.get(0),
            )?;
            transaction.execute(
                "INSERT INTO sync_metadata (key, value)
                 VALUES ('sync_suppress_changelog', '1')
                 ON CONFLICT(key) DO UPDATE SET value = '1'",
                [],
            )?;
            transaction.execute(
                "UPDATE clipboard_items
                    SET modified_at_ms = COALESCE(modified_at_ms, created_at_ms),
                        sync_writer_device_id = ?1",
                [&device_id],
            )?;
            transaction.execute_batch(
                "DELETE FROM sync_item_aliases;
                 DELETE FROM sync_item_resources;
                 DELETE FROM sync_outbox;
                 DELETE FROM sync_tombstones;
                 DELETE FROM sync_publication_state;
                 DELETE FROM sync_cursors;
                 DELETE FROM sync_checkpoint_cursors;
                 DELETE FROM sync_checkpoint_state;
                 DELETE FROM sync_resource_scopes;
                 DELETE FROM sqlite_sequence WHERE name = 'sync_outbox';",
            )?;
            transaction.execute(
                "DELETE FROM sync_metadata
                  WHERE substr(key, 1, length(?1)) = ?1",
                [SYNC_HEAD_CACHE_PREFIX],
            )?;
            transaction.execute(
                "INSERT INTO sync_tombstones
                    (item_id, kind, content_hash, deleted_at_ms,
                     modified_at_ms, writer_device_id)
                 SELECT id, kind, content_hash,
                        COALESCE(deleted_at_ms, modified_at_ms, created_at_ms),
                        COALESCE(modified_at_ms, created_at_ms),
                        sync_writer_device_id
                   FROM clipboard_items
                  WHERE deleted = 1",
                [],
            )?;
            transaction.execute(
                "DELETE FROM sync_metadata WHERE key = 'sync_suppress_changelog'",
                [],
            )?;
            transaction.execute(
                "INSERT INTO sync_metadata (key, value)
                 VALUES ('sync_enabled', '1')
                 ON CONFLICT(key) DO UPDATE SET value = '1'",
                [],
            )?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub fn is_sync_initialized(&self) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let enabled: Option<String> = connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_enabled'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(enabled.as_deref() == Some("1"))
        })
    }

    pub fn export_sync_snapshot(&self) -> Result<SyncSnapshot, StorageError> {
        self.with_connection(|connection| {
            let through_sequence = current_sequence(connection)?;
            let mutations = load_mutations(connection, None, None)?;
            Ok(SyncSnapshot {
                through_sequence,
                mutations,
            })
        })
    }

    pub fn export_sync_snapshot_for_scope(
        &self,
        remote_scope: &str,
    ) -> Result<SyncSnapshot, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let through_sequence = current_sequence(connection)?;
            let mutations = load_mutations(connection, Some(remote_scope), None)?;
            Ok(SyncSnapshot {
                through_sequence,
                mutations,
            })
        })
    }

    /// Visits a deterministic, point-in-time snapshot in bounded mutation
    /// batches. The SQLite read transaction stays open only while rows are
    /// exported; callers must not perform network I/O from `visit`.
    pub fn visit_sync_snapshot_for_scope(
        &self,
        remote_scope: &str,
        batch_size: usize,
        mut visit: impl FnMut(MutationBatch) -> Result<(), StorageError>,
    ) -> Result<SyncSnapshotExport, StorageError> {
        validate_remote_scope(remote_scope)?;
        if batch_size == 0 {
            return Err(StorageError::InvalidSyncState(
                "sync snapshot export batch size must be greater than zero".to_string(),
            ));
        }
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let through_sequence = current_sequence(&transaction)?;
            let mut record_count = 0u64;
            let mut after_item_id = None::<String>;
            loop {
                let batch = load_snapshot_upsert_batch(
                    &transaction,
                    remote_scope,
                    after_item_id.as_deref(),
                    batch_size,
                )?;
                if batch.is_empty() {
                    break;
                }
                after_item_id = batch.upserts.last().map(|item| item.item.id.clone());
                record_count = record_count.checked_add(batch.len() as u64).ok_or(
                    StorageError::ValueOutOfRange {
                        field: "sync snapshot record count",
                    },
                )?;
                visit(batch)?;
            }

            let mut after_tombstone_id = None::<String>;
            loop {
                let batch = load_snapshot_tombstone_batch(
                    &transaction,
                    after_tombstone_id.as_deref(),
                    batch_size,
                )?;
                if batch.is_empty() {
                    break;
                }
                after_tombstone_id = batch
                    .tombstones
                    .last()
                    .map(|tombstone| tombstone.item_id.clone());
                record_count = record_count.checked_add(batch.len() as u64).ok_or(
                    StorageError::ValueOutOfRange {
                        field: "sync snapshot record count",
                    },
                )?;
                visit(batch)?;
            }

            transaction.commit()?;
            Ok(SyncSnapshotExport {
                through_sequence,
                record_count,
            })
        })
    }
}
