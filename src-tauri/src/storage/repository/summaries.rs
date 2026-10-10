//! List reads never allocate rich-text bodies. Full records remain the storage/export contract.
use super::{history_predicates, StoredClipboardItem, ITEM_LOOKUP_CHUNK_SIZE};
use crate::domain::ClipboardItem;
use crate::storage::{Database, HistoryFilter, StorageError};
use rusqlite::{params_from_iter, types::ToSql};
use std::collections::HashMap;

// Preserve file path JSON and metadata used by cards; bound text/link previews in SQLite.
// Empty HTML/RTF sentinels retain format availability without loading either payload.
const SUMMARY_COLUMNS: &str = "
 id, kind, title,
 CASE WHEN kind IN ('text', 'link') THEN substr(text_content, 1, 2048) ELSE text_content END,
 CASE WHEN html_content IS NULL THEN NULL ELSE '' END,
 CASE WHEN rtf_content IS NULL THEN NULL ELSE '' END,
 resource_path, preview_path, content_hash, source_app, size_bytes, created_at_ms,
 last_used_at_ms, is_favorite, icon_path, metadata_json";

impl Database {
    pub fn list_summaries(
        &self,
        limit: u32,
        offset: u32,
        filter: &HistoryFilter,
        deleted: bool,
    ) -> Result<Vec<ClipboardItem>, StorageError> {
        self.with_connection(|connection| {
            let (mut conditions, mut args) = if deleted {
                (vec!["deleted = 1".into()], Vec::<Box<dyn ToSql>>::new())
            } else { history_predicates(filter) };
            let cursor = if deleted { None } else { filter.cursor.as_ref() };
            if let Some(cursor) = cursor {
                conditions.push("(last_used_at_ms, id) < (?, ?)".into());
                args.push(Box::new(cursor.last_used_at_ms));
                args.push(Box::new(cursor.id.clone()));
            }
            args.push(Box::new(i64::from(limit)));
            let limit_clause = if cursor.is_some() { "LIMIT ?" } else {
                args.push(Box::new(i64::from(offset)));
                "LIMIT ? OFFSET ?"
            };
            let order = if deleted {
                "COALESCE(deleted_at_ms, created_at_ms) DESC, created_at_ms DESC, id DESC"
            } else { "last_used_at_ms DESC, id DESC" };
            let sql = format!("SELECT {SUMMARY_COLUMNS} FROM clipboard_items WHERE {} ORDER BY {order} {limit_clause}", conditions.join(" AND "));
            let mut statement = connection.prepare_cached(&sql)?;
            let rows = statement.query_map(params_from_iter(args), StoredClipboardItem::from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter().map(TryInto::try_into).collect()
        })
    }

    pub fn get_summary_items_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<ClipboardItem>, StorageError> {
        self.with_connection(|connection| {
            let mut items = HashMap::with_capacity(ids.len());
            for chunk in ids.chunks(ITEM_LOOKUP_CHUNK_SIZE) {
                let placeholders = vec!["?"; chunk.len()].join(",");
                let sql = format!("SELECT {SUMMARY_COLUMNS} FROM clipboard_items WHERE deleted = 0 AND id IN ({placeholders})");
                let mut statement = connection.prepare_cached(&sql)?;
                let rows = statement.query_map(params_from_iter(chunk), StoredClipboardItem::from_row)?;
                for row in rows {
                    let item = ClipboardItem::try_from(row?)?;
                    items.insert(item.id.clone(), item);
                }
            }
            Ok(ids.iter().filter_map(|id| items.remove(id)).collect())
        })
    }
}
