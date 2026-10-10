//! `remote` sync persistence (moved verbatim from `sync_state.rs`).

use super::*;

impl Database {
    pub fn get_or_create_sync_remote_state(
        &self,
        remote_scope: &str,
    ) -> Result<SyncRemoteState, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some(state) = load_remote_state(&transaction, remote_scope)? {
                transaction.commit()?;
                return Ok(state);
            }
            let epoch = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO sync_publication_state (remote_scope, epoch, updated_at_ms)
                 VALUES (?1, ?2, ?3)",
                params![remote_scope, &epoch, current_time_ms()],
            )?;
            let state = load_remote_state(&transaction, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState(
                    "newly created remote state could not be read".to_string(),
                )
            })?;
            transaction.commit()?;
            Ok(state)
        })
    }

    pub fn reset_sync_remote_state(
        &self,
        remote_scope: &str,
    ) -> Result<SyncRemoteState, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let epoch = Uuid::new_v4().to_string();
            connection.execute(
                "INSERT INTO sync_publication_state
                    (remote_scope, epoch, updated_at_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(remote_scope) DO UPDATE SET
                    epoch = excluded.epoch,
                    snapshot_key = NULL,
                    snapshot_sha256 = NULL,
                    snapshot_size_bytes = 0,
                    snapshot_record_count = 0,
                    snapshot_sequence = 0,
                    published_sequence = 0,
                    last_segment_key = NULL,
                    initialized = 0,
                    updated_at_ms = excluded.updated_at_ms",
                params![remote_scope, &epoch, current_time_ms()],
            )?;
            load_remote_state(connection, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState("reset remote state is missing".to_string())
            })
        })
    }

    pub fn mark_sync_remote_prepared(&self, remote_scope: &str) -> Result<(), StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let affected = connection.execute(
                "UPDATE sync_publication_state
                    SET remote_prepared = 1, updated_at_ms = ?2
                  WHERE remote_scope = ?1",
                params![remote_scope, current_time_ms()],
            )?;
            if affected != 1 {
                return Err(StorageError::InvalidSyncState(
                    "cannot mark cleanup for an unknown remote scope".to_string(),
                ));
            }
            Ok(())
        })
    }

    pub fn commit_sync_bootstrap_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        snapshot: &ObjectRef,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, StorageError> {
        validate_remote_scope(remote_scope)?;
        let through_sequence = sequence_to_i64(through_sequence, "sync snapshot sequence")?;
        let snapshot_size = sequence_to_i64(snapshot.stored_size_bytes, "sync snapshot size")?;
        let snapshot_records =
            sequence_to_i64(snapshot.record_count, "sync snapshot record count")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if through_sequence
                > sequence_to_i64(current_sequence(&transaction)?, "sync current sequence")?
            {
                return Err(StorageError::InvalidSyncState(
                    "bootstrap publication exceeds the local outbox high-water".to_string(),
                ));
            }
            let affected = transaction.execute(
                "UPDATE sync_publication_state
                    SET snapshot_key = ?3,
                        snapshot_sha256 = ?4,
                        snapshot_size_bytes = ?5,
                        snapshot_record_count = ?6,
                        snapshot_sequence = ?7,
                        published_sequence = ?7,
                        last_segment_key = NULL,
                        initialized = 1,
                        updated_at_ms = ?8
                  WHERE remote_scope = ?1 AND epoch = ?2 AND remote_prepared = 1",
                params![
                    remote_scope,
                    expected_epoch,
                    &snapshot.key,
                    &snapshot.sha256,
                    snapshot_size,
                    snapshot_records,
                    through_sequence,
                    current_time_ms(),
                ],
            )?;
            if affected != 1 {
                return Err(StorageError::InvalidSyncState(
                    "remote epoch changed before bootstrap publication committed".to_string(),
                ));
            }
            transaction.execute(
                "DELETE FROM sync_outbox WHERE sequence <= ?1",
                [through_sequence],
            )?;
            let state = load_remote_state(&transaction, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState("published remote state is missing".to_string())
            })?;
            transaction.commit()?;
            Ok(state)
        })
    }

    pub fn commit_sync_segment_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        last_segment_key: &str,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, StorageError> {
        validate_remote_scope(remote_scope)?;
        let through_sequence = sequence_to_i64(through_sequence, "sync segment sequence")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let current = load_remote_state(&transaction, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState("remote publication state is missing".to_string())
            })?;
            if current.epoch != expected_epoch {
                return Err(StorageError::InvalidSyncState(
                    "remote epoch changed before segment publication committed".to_string(),
                ));
            }
            if !current.initialized || current.snapshot.is_none() {
                return Err(StorageError::InvalidSyncState(
                    "cannot publish a segment before the bootstrap snapshot".to_string(),
                ));
            }
            if through_sequence
                > sequence_to_i64(current_sequence(&transaction)?, "sync current sequence")?
            {
                return Err(StorageError::InvalidSyncState(
                    "segment publication exceeds the local outbox high-water".to_string(),
                ));
            }
            if through_sequence
                < sequence_to_i64(current.published_sequence, "sync published sequence")?
            {
                return Err(StorageError::InvalidSyncState(
                    "segment publication sequence regressed".to_string(),
                ));
            }
            if through_sequence
                == sequence_to_i64(current.published_sequence, "sync published sequence")?
                && current.last_segment_key.as_deref() != Some(last_segment_key)
            {
                return Err(StorageError::InvalidSyncState(
                    "equal publication sequence has a different segment key".to_string(),
                ));
            }
            transaction.execute(
                "UPDATE sync_publication_state
                    SET published_sequence = ?3,
                        last_segment_key = ?4,
                        updated_at_ms = ?5
                  WHERE remote_scope = ?1 AND epoch = ?2",
                params![
                    remote_scope,
                    expected_epoch,
                    through_sequence,
                    last_segment_key,
                    current_time_ms(),
                ],
            )?;
            transaction.execute(
                "DELETE FROM sync_outbox WHERE sequence <= ?1",
                [through_sequence],
            )?;
            let state = load_remote_state(&transaction, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState("published remote state is missing".to_string())
            })?;
            transaction.commit()?;
            Ok(state)
        })
    }
}
