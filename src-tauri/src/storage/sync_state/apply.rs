//! `apply` sync persistence (moved verbatim from `sync_state.rs`).

use super::*;

impl Database {
    pub fn apply_sync_snapshot(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        mutations: &MutationBatch,
    ) -> Result<u64, StorageError> {
        self.apply_sync_snapshot_with_resources(
            remote_scope,
            cursor,
            snapshot_sha256,
            mutations,
            &BTreeMap::new(),
        )
    }

    pub fn apply_sync_snapshot_with_resources(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        mutations: &MutationBatch,
        resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
    ) -> Result<u64, StorageError> {
        validate_remote_scope(remote_scope)?;
        validate_cursor_identity(cursor)?;
        if cursor.last_segment_key.is_some() {
            return Err(StorageError::InvalidSyncState(
                "snapshot cursor must not contain a segment key".to_string(),
            ));
        }
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some(existing) = load_cursor(&transaction, remote_scope, &cursor.device_id)? {
                if existing.epoch == cursor.epoch && existing.sequence > cursor.sequence {
                    return Err(StorageError::InvalidSyncState(
                        "snapshot cursor sequence regressed".to_string(),
                    ));
                }
            }
            set_changelog_suppressed(&transaction, true)?;
            let applied = apply_mutations(&transaction, remote_scope, mutations, resource_refs)?;
            set_changelog_suppressed(&transaction, false)?;
            upsert_cursor(&transaction, remote_scope, cursor, Some(snapshot_sha256))?;
            transaction.commit()?;
            Ok(applied)
        })
    }

    pub fn apply_sync_snapshot_batches<I>(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        batches: I,
    ) -> Result<u64, StorageError>
    where
        I: IntoIterator<
            Item = Result<(MutationBatch, BTreeMap<String, Vec<SyncResourceRef>>), StorageError>,
        >,
    {
        validate_remote_scope(remote_scope)?;
        validate_cursor_identity(cursor)?;
        if cursor.last_segment_key.is_some() {
            return Err(StorageError::InvalidSyncState(
                "snapshot cursor must not contain a segment key".to_string(),
            ));
        }
        // See `apply_sync_checkpoint_batches`: decode chunk by chunk inside the
        // transaction so residency is bounded by one chunk rather than by the
        // whole pack.

        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some(existing) = load_cursor(&transaction, remote_scope, &cursor.device_id)? {
                if existing.epoch == cursor.epoch && existing.sequence > cursor.sequence {
                    return Err(StorageError::InvalidSyncState(
                        "snapshot cursor sequence regressed".to_string(),
                    ));
                }
            }
            set_changelog_suppressed(&transaction, true)?;
            let applied = Self::fold_decoded_batches(batches, |(mutations, resource_refs)| {
                apply_mutations(&transaction, remote_scope, mutations, resource_refs)
            })?;
            set_changelog_suppressed(&transaction, false)?;
            upsert_cursor(&transaction, remote_scope, cursor, Some(snapshot_sha256))?;
            transaction.commit()?;
            Ok(applied)
        })
    }

    pub fn apply_sync_segment(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        mutations: &MutationBatch,
    ) -> Result<u64, StorageError> {
        self.apply_sync_segment_with_resources(remote_scope, cursor, mutations, &BTreeMap::new())
    }

    pub fn apply_sync_segment_with_resources(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        mutations: &MutationBatch,
        resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
    ) -> Result<u64, StorageError> {
        validate_remote_scope(remote_scope)?;
        validate_cursor_identity(cursor)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let existing =
                load_cursor(&transaction, remote_scope, &cursor.device_id)?.ok_or_else(|| {
                    StorageError::InvalidSyncState(
                        "segment received before its device snapshot".to_string(),
                    )
                })?;
            if existing.epoch != cursor.epoch {
                return Err(StorageError::InvalidSyncState(
                    "segment epoch does not match the applied snapshot".to_string(),
                ));
            }
            if cursor.sequence < existing.sequence {
                return Err(StorageError::InvalidSyncState(
                    "segment cursor sequence regressed".to_string(),
                ));
            }
            if cursor.sequence == existing.sequence {
                if cursor.last_segment_key == existing.last_segment_key {
                    transaction.commit()?;
                    return Ok(0);
                }
                return Err(StorageError::InvalidSyncState(
                    "equal segment sequence has a different object key".to_string(),
                ));
            }
            if cursor.last_segment_key.is_none() {
                return Err(StorageError::InvalidSyncState(
                    "segment cursor is missing its object key".to_string(),
                ));
            }
            set_changelog_suppressed(&transaction, true)?;
            let applied = apply_mutations(&transaction, remote_scope, mutations, resource_refs)?;
            set_changelog_suppressed(&transaction, false)?;
            upsert_cursor(&transaction, remote_scope, cursor, None)?;
            transaction.commit()?;
            Ok(applied)
        })
    }
}
