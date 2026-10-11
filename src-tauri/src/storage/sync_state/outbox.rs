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

    /// Distinct item ids referenced by outbox rows within
    /// `(after_sequence, through_sequence]`, ordered ascending.
    ///
    /// A publish that crashed between `publish_head` and its local commit
    /// leaves exactly the gap rows behind, so this set equals the published
    /// segment's mutation ids. A database restored from an older backup can
    /// still fill the sequence range with unrelated newer rows, so the caller
    /// must compare identities, not just counts.
    pub fn sync_outbox_range_item_ids(
        &self,
        after_sequence: u64,
        through_sequence: u64,
    ) -> Result<Vec<String>, StorageError> {
        if through_sequence <= after_sequence {
            return Ok(Vec::new());
        }
        if through_sequence > i64::MAX as u64 || after_sequence > i64::MAX as u64 {
            return Err(StorageError::ValueOutOfRange {
                field: "sync_outbox.sequence",
            });
        }
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT DISTINCT item_id
                   FROM sync_outbox
                  WHERE sequence > ?1 AND sequence <= ?2
                  ORDER BY item_id ASC",
            )?;
            let rows = statement
                .query_map([after_sequence as i64, through_sequence as i64], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
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
