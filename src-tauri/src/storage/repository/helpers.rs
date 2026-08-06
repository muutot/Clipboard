use std::collections::HashSet;

use rusqlite::{params, Row};

use crate::domain::{ClipboardItem, ClipboardKind};
use crate::storage::StorageError;

use super::{KindDeleteResult, KindDeleteScope, KindStorageStats};

pub(super) fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(super) fn unique_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.iter()
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

pub(super) fn query_kind_storage_stats(
    connection: &rusqlite::Connection,
    kind: ClipboardKind,
    scope: KindDeleteScope,
) -> Result<KindStorageStats, StorageError> {
    let (item_count, size_bytes): (i64, i64) = connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0)
         FROM clipboard_items
         WHERE kind = ?1
           AND (?2 OR is_favorite = 0)
           AND (?3 OR deleted = 0)",
        params![
            kind_to_storage(kind),
            scope.include_favorites,
            scope.include_deleted,
        ],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    Ok(KindStorageStats {
        item_count: u64::try_from(item_count).map_err(|_| StorageError::InvalidStoredValue {
            field: "clipboard_items.count",
            value: item_count,
        })?,
        size_bytes: u64::try_from(size_bytes).map_err(|_| StorageError::InvalidStoredValue {
            field: "clipboard_items.size",
            value: size_bytes,
        })?,
    })
}

pub(super) fn delete_kind_records(
    connection: &mut rusqlite::Connection,
    kind: ClipboardKind,
    scope: KindDeleteScope,
    expected: Option<KindStorageStats>,
) -> Result<KindDeleteResult, StorageError> {
    let transaction = connection.transaction()?;
    if let Some(expected) = expected {
        let current = query_kind_storage_stats(&transaction, kind, scope)?;
        if current != expected {
            return Err(StorageError::KindDeleteStatsChanged {
                expected_count: expected.item_count,
                expected_size: expected.size_bytes,
                actual_count: current.item_count,
                actual_size: current.size_bytes,
            });
        }
    }

    let mut statement = transaction.prepare(
        "DELETE FROM clipboard_items
         WHERE kind = ?1
           AND (?2 OR is_favorite = 0)
           AND (?3 OR deleted = 0)
         RETURNING id, size_bytes",
    )?;
    let deleted_rows = statement.query_map(
        params![
            kind_to_storage(kind),
            scope.include_favorites,
            scope.include_deleted,
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let mut stats = KindStorageStats::default();
    let mut deleted_ids = Vec::new();
    for deleted_row in deleted_rows {
        let (id, size_bytes) = deleted_row?;
        let size_bytes =
            u64::try_from(size_bytes).map_err(|_| StorageError::InvalidStoredValue {
                field: "clipboard_items.size",
                value: size_bytes,
            })?;
        stats.item_count =
            stats
                .item_count
                .checked_add(1)
                .ok_or(StorageError::ValueOutOfRange {
                    field: "clipboard_items.count",
                })?;
        stats.size_bytes =
            stats
                .size_bytes
                .checked_add(size_bytes)
                .ok_or(StorageError::ValueOutOfRange {
                    field: "clipboard_items.size",
                })?;
        deleted_ids.push(id);
    }
    drop(statement);
    transaction.commit()?;
    deleted_ids.sort_unstable();
    Ok(KindDeleteResult { stats, deleted_ids })
}

pub(super) struct StoredClipboardItem {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) title: String,
    pub(super) text_content: Option<String>,
    pub(super) html_content: Option<String>,
    pub(super) rtf_content: Option<String>,
    pub(super) resource_path: Option<String>,
    pub(super) preview_path: Option<String>,
    pub(super) content_hash: String,
    pub(super) source_app: Option<String>,
    pub(super) size_bytes: i64,
    pub(super) created_at_ms: i64,
    pub(super) last_used_at_ms: Option<i64>,
    pub(super) is_favorite: bool,
    pub(super) icon_path: Option<String>,
    pub(super) metadata_json: Option<String>,
}

impl StoredClipboardItem {
    pub(super) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind: row.get(1)?,
            title: row.get(2)?,
            text_content: row.get(3)?,
            html_content: row.get(4)?,
            rtf_content: row.get(5)?,
            resource_path: row.get(6)?,
            preview_path: row.get(7)?,
            content_hash: row.get(8)?,
            source_app: row.get(9)?,
            size_bytes: row.get(10)?,
            created_at_ms: row.get(11)?,
            last_used_at_ms: row.get(12)?,
            is_favorite: row.get(13)?,
            icon_path: row.get(14)?,
            metadata_json: row.get(15)?,
        })
    }
}

impl TryFrom<StoredClipboardItem> for ClipboardItem {
    type Error = StorageError;

    fn try_from(item: StoredClipboardItem) -> Result<Self, Self::Error> {
        Ok(Self {
            id: item.id,
            kind: kind_from_storage(&item.kind)?,
            title: item.title,
            text_content: item.text_content,
            html_content: item.html_content,
            rtf_content: item.rtf_content,
            resource_path: item.resource_path,
            preview_path: item.preview_path,
            content_hash: item.content_hash,
            source_app: item.source_app,
            size_bytes: u64::try_from(item.size_bytes).map_err(|_| {
                StorageError::InvalidStoredValue {
                    field: "size_bytes",
                    value: item.size_bytes,
                }
            })?,
            created_at_ms: item.created_at_ms,
            last_used_at_ms: item.last_used_at_ms,
            is_favorite: item.is_favorite,
            icon_path: item.icon_path,
            metadata_json: item.metadata_json,
        })
    }
}

pub(super) fn kind_to_storage(kind: ClipboardKind) -> &'static str {
    match kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image => "image",
        ClipboardKind::File => "file",
    }
}

pub(super) fn kind_from_storage(kind: &str) -> Result<ClipboardKind, StorageError> {
    match kind {
        "text" => Ok(ClipboardKind::Text),
        "link" => Ok(ClipboardKind::Link),
        "image" => Ok(ClipboardKind::Image),
        "file" => Ok(ClipboardKind::File),
        _ => Err(StorageError::InvalidClipboardKind(kind.to_owned())),
    }
}
