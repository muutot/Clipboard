//! Remote-state, cursor and changelog-suppression helpers.

use super::*;

pub(super) fn load_remote_state(
    connection: &rusqlite::Connection,
    remote_scope: &str,
) -> Result<Option<SyncRemoteState>, StorageError> {
    let stored = connection
        .query_row(
            "SELECT epoch, snapshot_key, snapshot_sha256,
                    snapshot_size_bytes, snapshot_record_count,
                    snapshot_sequence, published_sequence, last_segment_key,
                    remote_prepared, initialized, updated_at_ms
               FROM sync_publication_state
              WHERE remote_scope = ?1",
            [remote_scope],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, bool>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        epoch,
        snapshot_key,
        snapshot_sha256,
        snapshot_size_bytes,
        snapshot_record_count,
        snapshot_sequence,
        published_sequence,
        last_segment_key,
        remote_prepared,
        initialized,
        updated_at_ms,
    )) = stored
    else {
        return Ok(None);
    };
    let snapshot = match (snapshot_key, snapshot_sha256) {
        (Some(key), Some(sha256)) => Some(ObjectRef {
            key,
            sha256,
            stored_size_bytes: stored_sequence(snapshot_size_bytes, "sync snapshot size")?,
            record_count: stored_sequence(snapshot_record_count, "sync snapshot record count")?,
        }),
        (None, None) => None,
        _ => {
            return Err(StorageError::InvalidSyncState(
                "remote snapshot key/hash presence does not match".to_string(),
            ));
        }
    };
    Ok(Some(SyncRemoteState {
        remote_scope: remote_scope.to_string(),
        epoch,
        snapshot,
        snapshot_sequence: stored_sequence(snapshot_sequence, "sync snapshot sequence")?,
        published_sequence: stored_sequence(published_sequence, "sync published sequence")?,
        last_segment_key,
        remote_prepared,
        initialized,
        updated_at_ms,
    }))
}

pub(super) fn load_cursor(
    connection: &rusqlite::Connection,
    remote_scope: &str,
    device_id: &str,
) -> Result<Option<DeviceCursor>, StorageError> {
    let stored = connection
        .query_row(
            "SELECT epoch, sequence, last_segment_key
               FROM sync_cursors
              WHERE remote_scope = ?1 AND device_id = ?2",
            params![remote_scope, device_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    stored
        .map(|(epoch, sequence, last_segment_key)| {
            Ok(DeviceCursor {
                device_id: device_id.to_string(),
                epoch,
                sequence: stored_sequence(sequence, "sync cursor sequence")?,
                last_segment_key,
            })
        })
        .transpose()
}

pub(super) fn load_checkpoint_cursors(
    connection: &rusqlite::Connection,
    remote_scope: &str,
) -> Result<Vec<DeviceCursor>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT device_id, epoch, sequence, last_segment_key
           FROM sync_checkpoint_cursors
          WHERE remote_scope = ?1
          ORDER BY device_id",
    )?;
    let cursors = statement
        .query_map([remote_scope], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .map(|row| {
            let (device_id, epoch, sequence, last_segment_key) = row?;
            Ok(DeviceCursor {
                device_id,
                epoch,
                sequence: stored_sequence(sequence, "sync checkpoint cursor sequence")?,
                last_segment_key,
            })
        })
        .collect();
    cursors
}

pub(super) fn validate_checkpoint_cursors(cursors: &[DeviceCursor]) -> Result<(), StorageError> {
    let mut identities = BTreeSet::new();
    for cursor in cursors {
        validate_cursor_identity(cursor)?;
        if let Some(key) = cursor.last_segment_key.as_deref() {
            let parsed = parse_segment_key(key).map_err(StorageError::InvalidSyncState)?;
            if parsed.device_id != cursor.device_id
                || parsed.epoch != cursor.epoch
                || parsed.last_sequence != cursor.sequence
            {
                return Err(StorageError::InvalidSyncState(
                    "checkpoint cursor does not match its segment key".to_string(),
                ));
            }
        }
        if !identities.insert(cursor.device_id.as_str()) {
            return Err(StorageError::InvalidSyncState(
                "checkpoint contains duplicate device cursors".to_string(),
            ));
        }
    }
    Ok(())
}

pub(super) fn replace_checkpoint_cursors(
    transaction: &Transaction<'_>,
    remote_scope: &str,
    cursors: &[DeviceCursor],
) -> Result<(), StorageError> {
    transaction.execute(
        "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
        [remote_scope],
    )?;
    for cursor in cursors {
        transaction.execute(
            "INSERT INTO sync_checkpoint_cursors
                (remote_scope, device_id, epoch, sequence, last_segment_key)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                remote_scope,
                &cursor.device_id,
                &cursor.epoch,
                sequence_to_i64(cursor.sequence, "sync checkpoint cursor sequence")?,
                &cursor.last_segment_key,
            ],
        )?;
    }
    Ok(())
}

pub(super) fn upsert_cursor(
    transaction: &Transaction<'_>,
    remote_scope: &str,
    cursor: &DeviceCursor,
    snapshot_sha256: Option<&str>,
) -> Result<(), StorageError> {
    let sequence = sequence_to_i64(cursor.sequence, "sync cursor sequence")?;
    transaction.execute(
        "INSERT INTO sync_cursors
            (remote_scope, device_id, epoch, sequence, snapshot_sha256,
             last_segment_key, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(remote_scope, device_id) DO UPDATE SET
            epoch = excluded.epoch,
            sequence = excluded.sequence,
            snapshot_sha256 = COALESCE(excluded.snapshot_sha256, sync_cursors.snapshot_sha256),
            last_segment_key = excluded.last_segment_key,
            updated_at_ms = excluded.updated_at_ms",
        params![
            remote_scope,
            &cursor.device_id,
            &cursor.epoch,
            sequence,
            snapshot_sha256,
            &cursor.last_segment_key,
            current_time_ms(),
        ],
    )?;
    Ok(())
}

pub(super) fn set_changelog_suppressed(
    transaction: &Transaction<'_>,
    suppressed: bool,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO sync_metadata (key, value)
         VALUES ('sync_suppress_changelog', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [if suppressed { "1" } else { "0" }],
    )?;
    Ok(())
}
