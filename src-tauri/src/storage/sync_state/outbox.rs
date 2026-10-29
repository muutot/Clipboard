//! `outbox` sync persistence (moved verbatim from `sync_state.rs`).

use super::*;

impl Database {
    pub fn get_sync_outbox_batch(
        &self,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, StorageError> {
        self.get_sync_outbox_batch_inner(None, limit)
    }

    pub fn get_sync_outbox_batch_for_scope(
        &self,
        remote_scope: &str,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.get_sync_outbox_batch_inner(Some(remote_scope), limit)
    }

    fn get_sync_outbox_batch_inner(
        &self,
        remote_scope: Option<&str>,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, StorageError> {
        if limit == 0 {
            return Ok(None);
        }
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT sequence, item_id
                   FROM sync_outbox
                  ORDER BY sequence ASC
                  LIMIT ?1",
            )?;
            let rows = statement
                .query_map([limit as i64], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let Some((first_sequence, _)) = rows.first() else {
                return Ok(None);
            };
            let first_sequence = *first_sequence;
            let last_sequence = rows.last().map(|row| row.0).unwrap_or(first_sequence);
            let item_ids = rows
                .into_iter()
                .map(|(_, item_id)| item_id)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let mutations = load_mutations(connection, remote_scope, Some(&item_ids))?;
            if mutations.len() != item_ids.len() {
                return Err(StorageError::InvalidSyncState(
                    "outbox references an item without a live row or tombstone".to_string(),
                ));
            }
            Ok(Some(SyncOutboxBatch {
                first_sequence: u64::try_from(first_sequence).map_err(|_| {
                    StorageError::InvalidStoredValue {
                        field: "sync_outbox.sequence",
                        value: first_sequence,
                    }
                })?,
                last_sequence: u64::try_from(last_sequence).map_err(|_| {
                    StorageError::InvalidStoredValue {
                        field: "sync_outbox.sequence",
                        value: last_sequence,
                    }
                })?,
                mutations,
            }))
        })
    }

    /// Verify pending IDs and their current canonical mutations in one WAL
    /// snapshot. A restored backup can reuse both sequence numbers and item IDs
    /// for different edits, so neither IDs nor range/count prove publication.
    pub fn sync_outbox_range_matches_mutations(
        &self,
        remote_scope: &str,
        after_sequence: u64,
        through_sequence: u64,
        expected: &MutationBatch,
    ) -> Result<bool, StorageError> {
        validate_remote_scope(remote_scope)?;
        if through_sequence <= after_sequence || expected.is_empty() {
            return Ok(false);
        }
        if through_sequence > i64::MAX as u64 || after_sequence > i64::MAX as u64 {
            return Err(StorageError::ValueOutOfRange {
                field: "sync_outbox.sequence",
            });
        }
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let rows = {
                let mut statement = transaction.prepare(
                    "SELECT DISTINCT item_id
                   FROM sync_outbox
                  WHERE sequence > ?1 AND sequence <= ?2
                  ORDER BY item_id ASC LIMIT ?3",
                )?;
                let limit = i64::try_from(expected.len())
                    .ok()
                    .and_then(|len| len.checked_add(1))
                    .ok_or(StorageError::ValueOutOfRange {
                        field: "sync mutation count",
                    })?;
                let rows = statement.query_map(
                    [after_sequence as i64, through_sequence as i64, limit],
                    |row| row.get::<_, String>(0),
                )?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            if rows.len() != expected.len() {
                return Ok(false);
            }
            let mut pending = load_mutations(&transaction, Some(remote_scope), Some(&rows))?;
            let mut expected = expected.clone();
            // Restore/export retains portable keys for already prepared local
            // resources. Deferred normalization removes local-only previews and
            // paths without I/O; compare resource refs separately so clearing
            // paths cannot make different objects appear equal.
            let Ok(pending_refs) = crate::sync::v1::defer_mutation_resources(&mut pending) else {
                return Ok(false);
            };
            let Ok(expected_refs) = crate::sync::v1::defer_mutation_resources(&mut expected) else {
                return Ok(false);
            };
            pending.upserts.sort_by(|a, b| a.item.id.cmp(&b.item.id));
            expected.upserts.sort_by(|a, b| a.item.id.cmp(&b.item.id));
            pending.tombstones.sort_by(|a, b| a.item_id.cmp(&b.item_id));
            expected
                .tombstones
                .sort_by(|a, b| a.item_id.cmp(&b.item_id));
            Ok(pending == expected && pending_refs == expected_refs)
        })
    }

    pub fn acknowledge_sync_outbox(&self, through_sequence: u64) -> Result<u64, StorageError> {
        let through_sequence =
            i64::try_from(through_sequence).map_err(|_| StorageError::ValueOutOfRange {
                field: "sync_outbox.sequence",
            })?;
        self.with_connection(|connection| {
            let deleted = connection.execute(
                "DELETE FROM sync_outbox WHERE sequence <= ?1",
                [through_sequence],
            )?;
            Ok(deleted as u64)
        })
    }

    pub fn count_sync_outbox(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 =
                connection.query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))?;
            u64::try_from(count).map_err(|_| StorageError::InvalidStoredValue {
                field: "sync_outbox.count",
                value: count,
            })
        })
    }
}
