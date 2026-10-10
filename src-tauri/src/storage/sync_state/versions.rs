//! Version, identity, tag, kind and sequence helpers.

use super::*;

pub(super) fn validate_record_version(version: &RecordVersion) -> Result<(), StorageError> {
    let writer = Uuid::parse_str(&version.writer_device_id).ok();
    if version.modified_at_ms < 0
        || writer
            .as_ref()
            .is_none_or(|writer| writer.to_string() != version.writer_device_id)
    {
        return Err(StorageError::InvalidSyncState(
            "record version must contain a non-negative timestamp and writer UUID".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_cursor_identity(cursor: &DeviceCursor) -> Result<(), StorageError> {
    for (label, value) in [
        ("device", cursor.device_id.as_str()),
        ("epoch", cursor.epoch.as_str()),
    ] {
        let parsed = Uuid::parse_str(value).map_err(|_| {
            StorageError::InvalidSyncState(format!("cursor {label} id is not a UUID"))
        })?;
        if parsed.to_string() != value {
            return Err(StorageError::InvalidSyncState(format!(
                "cursor {label} id is not canonical lowercase"
            )));
        }
    }
    Ok(())
}

pub(super) fn winning_local_version(
    transaction: &Transaction<'_>,
    item_id: &str,
) -> Result<Option<RecordVersion>, StorageError> {
    let row_version = transaction
        .query_row(
            "SELECT COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
               FROM clipboard_items
              WHERE id = ?1",
            [item_id],
            |row| {
                Ok(RecordVersion {
                    modified_at_ms: row.get(0)?,
                    writer_device_id: row.get(1)?,
                })
            },
        )
        .optional()?;
    let tombstone_version = transaction
        .query_row(
            "SELECT modified_at_ms, writer_device_id
               FROM sync_tombstones
              WHERE item_id = ?1",
            [item_id],
            |row| {
                Ok(RecordVersion {
                    modified_at_ms: row.get(0)?,
                    writer_device_id: row.get(1)?,
                })
            },
        )
        .optional()?;
    Ok(match (row_version, tombstone_version) {
        (Some(row), Some(tombstone)) => Some(row.max(tombstone)),
        (Some(row), None) => Some(row),
        (None, Some(tombstone)) => Some(tombstone),
        (None, None) => None,
    })
}

pub(super) fn resolve_sync_item_id(
    transaction: &Transaction<'_>,
    remote_id: &str,
    kind: &str,
    content_hash: &str,
) -> Result<Option<String>, StorageError> {
    let exact = transaction
        .query_row(
            "SELECT id FROM clipboard_items WHERE id = ?1",
            [remote_id],
            |row| row.get(0),
        )
        .optional()?;
    if exact.is_some() {
        return Ok(exact);
    }
    let alias = transaction
        .query_row(
            "SELECT item_id FROM sync_item_aliases WHERE alias_id = ?1",
            [remote_id],
            |row| row.get(0),
        )
        .optional()?;
    if alias.is_some() {
        return Ok(alias);
    }
    if !matches!(kind, "text" | "link") {
        return Ok(None);
    }
    let matching = transaction
        .query_row(
            "SELECT id FROM clipboard_items WHERE kind = ?1 AND content_hash = ?2",
            params![kind, content_hash],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(item_id) = matching.as_deref() {
        transaction.execute(
            "INSERT INTO sync_item_aliases (alias_id, item_id)
             VALUES (?1, ?2)
             ON CONFLICT(alias_id) DO UPDATE SET item_id = excluded.item_id",
            params![remote_id, item_id],
        )?;
    }
    Ok(matching)
}

pub(super) fn replace_item_tags(
    transaction: &Transaction<'_>,
    item_id: &str,
    metadata_json: Option<&str>,
) -> Result<(), StorageError> {
    transaction.execute("DELETE FROM item_tags WHERE item_id = ?1", [item_id])?;
    let Some(metadata_json) = metadata_json else {
        return Ok(());
    };
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(metadata_json) else {
        return Ok(());
    };
    let Some(tags) = metadata.get("tags").and_then(|value| value.as_array()) else {
        return Ok(());
    };
    let mut unique = BTreeSet::new();
    for tag in tags.iter().filter_map(|value| value.as_str()) {
        let tag = tag.trim();
        if !tag.is_empty() && unique.insert(tag) {
            transaction.execute(
                "INSERT INTO item_tags (item_id, tag) VALUES (?1, ?2)",
                params![item_id, tag],
            )?;
        }
    }
    Ok(())
}

pub(super) fn kind_to_storage(kind: SyncItemKind) -> &'static str {
    match kind {
        SyncItemKind::Text => "text",
        SyncItemKind::Link => "link",
        SyncItemKind::Image => "image",
        SyncItemKind::File => "file",
    }
}

pub(super) fn kind_from_storage(kind: &str) -> Result<SyncItemKind, StorageError> {
    match kind {
        "text" => Ok(SyncItemKind::Text),
        "link" => Ok(SyncItemKind::Link),
        "image" => Ok(SyncItemKind::Image),
        "file" => Ok(SyncItemKind::File),
        _ => Err(StorageError::InvalidClipboardKind(kind.to_string())),
    }
}

pub(super) fn current_sequence(connection: &rusqlite::Connection) -> Result<u64, StorageError> {
    let sequence: Option<i64> = connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'sync_outbox'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let sequence = sequence.unwrap_or(0);
    u64::try_from(sequence).map_err(|_| StorageError::InvalidStoredValue {
        field: "sync_outbox.sequence",
        value: sequence,
    })
}
