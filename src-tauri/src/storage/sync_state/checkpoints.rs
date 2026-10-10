//! `checkpoints` sync persistence (moved verbatim from `sync_state.rs`).

use super::*;

impl Database {
    pub fn get_sync_cursor(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<DeviceCursor>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| load_cursor(connection, remote_scope, device_id))
    }

    pub fn list_sync_cursors(&self, remote_scope: &str) -> Result<Vec<DeviceCursor>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT device_id, epoch, sequence, last_segment_key
                   FROM sync_cursors
                  WHERE remote_scope = ?1
                  ORDER BY device_id",
            )?;
            let rows = statement
                .query_map([remote_scope], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(device_id, epoch, sequence, last_segment_key)| {
                    Ok(DeviceCursor {
                        device_id,
                        epoch,
                        sequence: stored_sequence(sequence, "sync cursor sequence")?,
                        last_segment_key,
                    })
                })
                .collect()
        })
    }

    pub(crate) fn get_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<SyncHeadCache>, StorageError> {
        let key = sync_head_cache_key(remote_scope, device_id)?;
        self.with_connection(|connection| {
            let value = connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = ?1",
                    [&key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok(value
                .and_then(|value| serde_json::from_str::<SyncHeadCache>(&value).ok())
                .filter(|cache| valid_sync_head_cache(cache, device_id)))
        })
    }

    pub(crate) fn record_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
        etag: &str,
        stored_size_bytes: u64,
        modified_ms: Option<i64>,
        head: &DeviceHead,
    ) -> Result<(), StorageError> {
        let key = sync_head_cache_key(remote_scope, device_id)?;
        if head.device_id != device_id || !valid_head_etag(etag) {
            return Err(StorageError::InvalidSyncState(
                "sync head cache identity or ETag is invalid".to_string(),
            ));
        }
        let cache = SyncHeadCache {
            etag: etag.to_string(),
            stored_size_bytes,
            modified_ms,
            epoch: head.epoch.clone(),
            snapshot_key: head.snapshot.key.clone(),
            snapshot_sha256: head.snapshot.sha256.clone(),
            snapshot_size_bytes: head.snapshot.stored_size_bytes,
            snapshot_record_count: head.snapshot.record_count,
            published_sequence: head.published_sequence,
            last_segment_key: head.last_segment_key.clone(),
        };
        if !valid_sync_head_cache(&cache, device_id) {
            return Err(StorageError::InvalidSyncState(
                "sync head cache payload is invalid".to_string(),
            ));
        }
        let value = serde_json::to_string(&cache)?;
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    pub fn get_sync_checkpoint_state(
        &self,
        remote_scope: &str,
    ) -> Result<Option<(u64, String)>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT generation, checkpoint_sha256
                       FROM sync_checkpoint_state
                      WHERE remote_scope = ?1",
                    [remote_scope],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
                .map(|(generation, sha256)| {
                    let generation = stored_sequence(generation, "sync checkpoint generation")?;
                    checkpoint_object_key(generation, &sha256)
                        .map_err(StorageError::InvalidSyncState)?;
                    Ok((generation, sha256))
                })
                .transpose()
        })
    }

    pub fn get_sync_checkpoint_cursors(
        &self,
        remote_scope: &str,
    ) -> Result<Vec<DeviceCursor>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| load_checkpoint_cursors(connection, remote_scope))
    }

    pub fn record_sync_checkpoint_published(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
    ) -> Result<(), StorageError> {
        validate_remote_scope(remote_scope)?;
        checkpoint_object_key(generation, checkpoint_sha256)
            .map_err(StorageError::InvalidSyncState)?;
        validate_checkpoint_cursors(cursors)?;
        let generation = sequence_to_i64(generation, "sync checkpoint generation")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some((stored_generation, stored_sha256)) = transaction
                .query_row(
                    "SELECT generation, checkpoint_sha256
                       FROM sync_checkpoint_state
                      WHERE remote_scope = ?1",
                    [remote_scope],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
            {
                if stored_generation > generation {
                    return Err(StorageError::InvalidSyncState(
                        "published checkpoint generation regressed".to_string(),
                    ));
                }
                if stored_generation == generation && stored_sha256 != checkpoint_sha256 {
                    return Err(StorageError::InvalidSyncState(
                        "equal published checkpoint generation has a different digest".to_string(),
                    ));
                }
            }
            transaction.execute(
                "INSERT INTO sync_checkpoint_state
                    (remote_scope, generation, checkpoint_sha256, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(remote_scope) DO UPDATE SET
                    generation = excluded.generation,
                    checkpoint_sha256 = excluded.checkpoint_sha256,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    remote_scope,
                    generation,
                    checkpoint_sha256,
                    current_time_ms(),
                ],
            )?;
            replace_checkpoint_cursors(&transaction, remote_scope, cursors)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn apply_sync_checkpoint(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        mutations: &MutationBatch,
    ) -> Result<u64, StorageError> {
        self.apply_sync_checkpoint_with_resources(
            remote_scope,
            generation,
            checkpoint_sha256,
            cursors,
            mutations,
            &BTreeMap::new(),
        )
    }

    pub fn apply_sync_checkpoint_with_resources(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        mutations: &MutationBatch,
        resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
    ) -> Result<u64, StorageError> {
        validate_remote_scope(remote_scope)?;
        checkpoint_object_key(generation, checkpoint_sha256)
            .map_err(StorageError::InvalidSyncState)?;
        let generation = sequence_to_i64(generation, "sync checkpoint generation")?;
        validate_checkpoint_cursors(cursors)?;

        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some((stored_generation, stored_sha256)) = transaction
                .query_row(
                    "SELECT generation, checkpoint_sha256
                       FROM sync_checkpoint_state
                      WHERE remote_scope = ?1",
                    [remote_scope],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
            {
                if stored_generation > generation {
                    return Err(StorageError::InvalidSyncState(
                        "checkpoint generation regressed".to_string(),
                    ));
                }
                if stored_generation == generation && stored_sha256 != checkpoint_sha256 {
                    return Err(StorageError::InvalidSyncState(
                        "equal checkpoint generation has a different digest".to_string(),
                    ));
                }
            }

            set_changelog_suppressed(&transaction, true)?;
            let applied = apply_mutations(&transaction, remote_scope, mutations, resource_refs)?;
            set_changelog_suppressed(&transaction, false)?;
            // A checkpoint vector is frozen at publish time, so a device
            // whose segments were pulled after the publish has an existing
            // cursor ahead of its vector entry. Replacing it would regress
            // the cursor and force a full re-pull (the same regression the
            // segment path rejects as "segment cursor sequence regressed").
            // Merge instead: keep any same-epoch cursor ahead of the vector
            // and preserve cursors for devices the vector does not mention.
            for cursor in cursors {
                match load_cursor(&transaction, remote_scope, &cursor.device_id)? {
                    Some(existing)
                        if existing.epoch == cursor.epoch
                            && existing.sequence > cursor.sequence =>
                    {
                        continue;
                    }
                    _ => upsert_cursor(&transaction, remote_scope, cursor, None)?,
                }
            }
            transaction.execute(
                "INSERT INTO sync_checkpoint_state
                    (remote_scope, generation, checkpoint_sha256, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(remote_scope) DO UPDATE SET
                    generation = excluded.generation,
                    checkpoint_sha256 = excluded.checkpoint_sha256,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    remote_scope,
                    generation,
                    checkpoint_sha256,
                    current_time_ms(),
                ],
            )?;
            replace_checkpoint_cursors(&transaction, remote_scope, cursors)?;
            transaction.commit()?;
            Ok(applied)
        })
    }

    /// Applies decoded pack batches one at a time.
    ///
    /// The iterator is consumed lazily and each item is released as soon as it has
    /// been applied, so peak residency is one batch rather than the whole pack. This
    /// is deliberately *not* a `collect()` into a `Vec` first: doing that made peak
    /// memory scale with the pack, up to the 1 GiB global uncompressed limit.
    ///
    /// Generic over the item type so the laziness itself is unit-testable with a
    /// drop-instrumented payload.
    pub(super) fn fold_decoded_batches<I, T, F>(
        batches: I,
        mut apply: F,
    ) -> Result<u64, StorageError>
    where
        I: IntoIterator<Item = Result<T, StorageError>>,
        F: FnMut(&T) -> Result<u64, StorageError>,
    {
        let mut applied = 0u64;
        for batch in batches {
            // A decode failure propagates and the caller drops the transaction, so
            // nothing partial is committed.
            applied =
                applied
                    .checked_add(apply(&batch?)?)
                    .ok_or(StorageError::ValueOutOfRange {
                        field: "applied sync mutation count",
                    })?;
        }
        Ok(applied)
    }

    pub fn apply_sync_checkpoint_batches<I>(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        batches: I,
    ) -> Result<u64, StorageError>
    where
        I: IntoIterator<
            Item = Result<(MutationBatch, BTreeMap<String, Vec<SyncResourceRef>>), StorageError>,
        >,
    {
        validate_remote_scope(remote_scope)?;
        checkpoint_object_key(generation, checkpoint_sha256)
            .map_err(StorageError::InvalidSyncState)?;
        let generation = sequence_to_i64(generation, "sync checkpoint generation")?;
        validate_checkpoint_cursors(cursors)?;

        // Decode chunk by chunk *inside* the transaction instead of collecting
        // the whole pack first.
        //
        // Collecting first made peak residency scale with the entire pack (up to
        // the 1 GiB global uncompressed limit), because every decoded
        // `MutationBatch` stayed alive while the transaction ran. Decoding here
        // bounds residency to one chunk (16 MiB packing target) and drops each
        // one after it is applied.
        //
        // The cost is that decompression of the already-downloaded local pack
        // file runs while the write transaction is open. That is the deliberate
        // trade: a longer write lock stalls concurrent capture writes, whereas
        // unbounded residency risks the OOM killer on a low-memory machine.
        // The pack's integrity is still verified by the caller's iterator, which
        // reports a mismatch at the end of the sequence; a failure here rolls
        // the transaction back, so nothing partial is committed.

        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some((stored_generation, stored_sha256)) = transaction
                .query_row(
                    "SELECT generation, checkpoint_sha256
                       FROM sync_checkpoint_state
                      WHERE remote_scope = ?1",
                    [remote_scope],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
            {
                if stored_generation > generation {
                    return Err(StorageError::InvalidSyncState(
                        "checkpoint generation regressed".to_string(),
                    ));
                }
                if stored_generation == generation && stored_sha256 != checkpoint_sha256 {
                    return Err(StorageError::InvalidSyncState(
                        "equal checkpoint generation has a different digest".to_string(),
                    ));
                }
            }

            set_changelog_suppressed(&transaction, true)?;
            let applied = Self::fold_decoded_batches(batches, |(mutations, resource_refs)| {
                apply_mutations(&transaction, remote_scope, mutations, resource_refs)
            })?;
            set_changelog_suppressed(&transaction, false)?;
            // A checkpoint vector is frozen at publish time, so a device
            // whose segments were pulled after the publish has an existing
            // cursor ahead of its vector entry. Replacing it would regress
            // the cursor and force a full re-pull (the same regression the
            // segment path rejects as "segment cursor sequence regressed").
            // Merge instead: keep any same-epoch cursor ahead of the vector
            // and preserve cursors for devices the vector does not mention.
            for cursor in cursors {
                match load_cursor(&transaction, remote_scope, &cursor.device_id)? {
                    Some(existing)
                        if existing.epoch == cursor.epoch
                            && existing.sequence > cursor.sequence =>
                    {
                        continue;
                    }
                    _ => upsert_cursor(&transaction, remote_scope, cursor, None)?,
                }
            }
            transaction.execute(
                "INSERT INTO sync_checkpoint_state
                    (remote_scope, generation, checkpoint_sha256, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(remote_scope) DO UPDATE SET
                    generation = excluded.generation,
                    checkpoint_sha256 = excluded.checkpoint_sha256,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    remote_scope,
                    generation,
                    checkpoint_sha256,
                    current_time_ms(),
                ],
            )?;
            replace_checkpoint_cursors(&transaction, remote_scope, cursors)?;
            transaction.commit()?;
            Ok(applied)
        })
    }
}
