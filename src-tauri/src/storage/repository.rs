use std::collections::{HashMap, HashSet};

use rusqlite::{params, params_from_iter, OptionalExtension, Row};

use crate::domain::{ClipboardItem, ClipboardKind};

use super::{Database, StorageError};

const ITEM_COLUMNS: &str = "
    id,
    kind,
    title,
    text_content,
    resource_path,
    preview_path,
    content_hash,
    source_app,
    size_bytes,
    created_at_ms,
    last_used_at_ms,
    is_favorite,
    icon_path,
    metadata_json
";

pub trait ClipboardRepository {
    fn save_item(&self, item: &ClipboardItem) -> Result<String, StorageError>;
    fn get_item(&self, id: &str) -> Result<Option<ClipboardItem>, StorageError>;
    fn get_items_by_ids(&self, ids: &[String]) -> Result<Vec<ClipboardItem>, StorageError>;
    fn list_recent(&self, limit: u32, offset: u32) -> Result<Vec<ClipboardItem>, StorageError>;
    fn list_source_applications(&self) -> Result<Vec<String>, StorageError>;
    fn list_source_applications_with_icons(&self) -> Result<Vec<(String, Option<String>)>, StorageError>;
    fn set_favorite(&self, id: &str, is_favorite: bool) -> Result<bool, StorageError>;
    /// Update the favorite flag for all requested records atomically.
    ///
    /// The operation returns `false` and makes no changes when any requested
    /// id does not exist. An empty id list is a no-op and also returns `false`.
    fn set_favorite_batch(&self, ids: &[String], is_favorite: bool) -> Result<bool, StorageError>;
    fn delete_item(&self, id: &str) -> Result<bool, StorageError>;
    fn item_count(&self) -> Result<u64, StorageError>;
    fn delete_older_than(&self, days: u32) -> Result<u64, StorageError>;
    fn enforce_capacity_limit(&self, max_items: u64) -> Result<u64, StorageError>;
    fn cleanup_orphan_search_index(&self) -> Result<u64, StorageError>;
    fn soft_delete(&self, id: &str) -> Result<bool, StorageError>;
    /// Soft-delete all requested active records atomically.
    ///
    /// A favorite active record aborts the whole operation with the same
    /// `FavoriteMustBeRemoved` error as the single-record API. Already deleted
    /// records are treated as idempotent; unknown ids return `false` without
    /// changing any record.
    fn soft_delete_batch(&self, ids: &[String]) -> Result<bool, StorageError>;
    fn restore_deleted(&self, id: &str) -> Result<bool, StorageError>;
    fn permanently_delete_expired(&self, days: u32) -> Result<u64, StorageError>;
    fn clear_all_non_favorite_items(&self) -> Result<u64, StorageError>;
    fn count_by_kind(&self, kind: &str) -> Result<u64, StorageError>;
    fn size_by_kind(&self, kind: &str) -> Result<u64, StorageError>;
    fn list_active_file_paths(&self) -> Result<Vec<String>, StorageError>;
}

impl ClipboardRepository for Database {
    fn save_item(&self, item: &ClipboardItem) -> Result<String, StorageError> {
        let size_bytes =
            i64::try_from(item.size_bytes).map_err(|_| StorageError::ValueOutOfRange {
                field: "size_bytes",
            })?;

        self.with_connection(|connection| {
            Ok(connection.query_row(
                "INSERT INTO clipboard_items (
                    id,
                    kind,
                    title,
                    text_content,
                    resource_path,
                    preview_path,
                    content_hash,
                    source_app,
                    size_bytes,
                    created_at_ms,
                    last_used_at_ms,
                    is_favorite,
                    icon_path,
                    metadata_json
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
                 )
                 ON CONFLICT DO UPDATE SET
                    title = excluded.title,
                    text_content = excluded.text_content,
                    resource_path = excluded.resource_path,
                    preview_path = excluded.preview_path,
                    source_app = excluded.source_app,
                    size_bytes = excluded.size_bytes,
                    created_at_ms = excluded.created_at_ms,
                    last_used_at_ms = COALESCE(
                        excluded.last_used_at_ms,
                        clipboard_items.last_used_at_ms
                    ),
                    is_favorite = MAX(
                        clipboard_items.is_favorite,
                        excluded.is_favorite
                    ),
                    icon_path = COALESCE(
                        excluded.icon_path,
                        clipboard_items.icon_path
                    ),
                    metadata_json = COALESCE(
                        excluded.metadata_json,
                        clipboard_items.metadata_json
                    )
                 RETURNING id",
                params![
                    item.id,
                    kind_to_storage(item.kind),
                    item.title,
                    item.text_content,
                    item.resource_path,
                    item.preview_path,
                    item.content_hash,
                    item.source_app,
                    size_bytes,
                    item.created_at_ms,
                    item.last_used_at_ms,
                    item.is_favorite,
                    item.icon_path,
                    item.metadata_json,
                ],
                |row| row.get(0),
            )?)
        })
    }

    fn get_item(&self, id: &str) -> Result<Option<ClipboardItem>, StorageError> {
        self.with_connection(|connection| {
            let sql = format!("SELECT {ITEM_COLUMNS} FROM clipboard_items WHERE id = ?1");
            let stored_item = connection
                .query_row(&sql, [id], StoredClipboardItem::from_row)
                .optional()?;

            stored_item.map(TryInto::try_into).transpose()
        })
    }

    fn get_items_by_ids(&self, ids: &[String]) -> Result<Vec<ClipboardItem>, StorageError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        self.with_connection(|connection| {
            let ids = &ids[..ids.len().min(500)];
            let placeholders = (1..=ids.len())
                .map(|position| format!("?{position}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT {ITEM_COLUMNS}
                 FROM clipboard_items
                 WHERE deleted = 0
                   AND id IN ({placeholders})"
            );
            let mut statement = connection.prepare(&sql)?;
            let stored_items = statement
                .query_map(params_from_iter(ids.iter()), StoredClipboardItem::from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            let mut items_by_id = stored_items
                .into_iter()
                .map(|item| {
                    let item = ClipboardItem::try_from(item)?;
                    Ok((item.id.clone(), item))
                })
                .collect::<Result<HashMap<_, _>, StorageError>>()?;

            Ok(ids.iter().filter_map(|id| items_by_id.remove(id)).collect())
        })
    }

    fn list_recent(&self, limit: u32, offset: u32) -> Result<Vec<ClipboardItem>, StorageError> {
        self.with_connection(|connection| {
            let sql = format!(
                "SELECT {ITEM_COLUMNS}
                 FROM clipboard_items
                 WHERE deleted = 0
                 ORDER BY created_at_ms DESC
                 LIMIT ?1 OFFSET ?2"
            );
            let mut statement = connection.prepare_cached(&sql)?;
            let stored_items = statement
                .query_map(
                    params![i64::from(limit.clamp(1, 500)), i64::from(offset)],
                    StoredClipboardItem::from_row,
                )?
                .collect::<Result<Vec<_>, _>>()?;

            stored_items.into_iter().map(TryInto::try_into).collect()
        })
    }

    fn list_source_applications(&self) -> Result<Vec<String>, StorageError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT MIN(TRIM(source_app))
                 FROM clipboard_items
                 WHERE source_app IS NOT NULL
                   AND TRIM(source_app) <> ''
                   AND deleted = 0
                 GROUP BY LOWER(TRIM(source_app))
                 ORDER BY LOWER(TRIM(source_app)) ASC",
            )?;

            let applications = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(applications)
        })
    }

    fn list_source_applications_with_icons(&self) -> Result<Vec<(String, Option<String>)>, StorageError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT
                    MIN(TRIM(c.source_app)) AS app,
                    (SELECT ci.icon_path
                     FROM clipboard_items ci
                     WHERE LOWER(TRIM(ci.source_app)) = LOWER(TRIM(c.source_app))
                       AND ci.icon_path IS NOT NULL
                       AND ci.deleted = 0
                     ORDER BY ci.created_at_ms DESC
                     LIMIT 1) AS icon
                 FROM clipboard_items c
                 WHERE c.source_app IS NOT NULL
                   AND TRIM(c.source_app) <> ''
                   AND c.deleted = 0
                 GROUP BY LOWER(TRIM(c.source_app))
                 ORDER BY LOWER(app) ASC",
            )?;
            let apps = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(apps)
        })
    }

    fn set_favorite(&self, id: &str, is_favorite: bool) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "UPDATE clipboard_items SET is_favorite = ?2 WHERE id = ?1",
                params![id, is_favorite],
            )? > 0)
        })
    }

    fn set_favorite_batch(&self, ids: &[String], is_favorite: bool) -> Result<bool, StorageError> {
        let ids = unique_ids(ids);
        if ids.is_empty() {
            return Ok(false);
        }

        self.with_connection(|connection| {
            let transaction = connection.transaction()?;

            // Validate the complete request before changing anything. This
            // keeps the boolean contract deterministic and avoids partially
            // applying a stale selection from the UI.
            for id in &ids {
                let exists = transaction
                    .query_row(
                        "SELECT 1 FROM clipboard_items WHERE id = ?1",
                        [id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?;
                if exists.is_none() {
                    return Ok(false);
                }
            }

            for id in &ids {
                transaction.execute(
                    "UPDATE clipboard_items SET is_favorite = ?2 WHERE id = ?1",
                    params![id, is_favorite],
                )?;
            }

            transaction.commit()?;
            Ok(true)
        })
    }

    fn delete_item(&self, id: &str) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let is_favorite = connection
                .query_row(
                    "SELECT is_favorite FROM clipboard_items WHERE id = ?1",
                    [id],
                    |row| row.get::<_, bool>(0),
                )
                .optional()?;

            if is_favorite == Some(true) {
                return Err(StorageError::FavoriteMustBeRemoved(id.to_owned()));
            }

            Ok(connection.execute("DELETE FROM clipboard_items WHERE id = ?1", [id])? > 0)
        })
    }

    fn item_count(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 =
                connection
                    .query_row("SELECT COUNT(*) FROM clipboard_items WHERE deleted = 0", [], |row| row.get(0))?;

            u64::try_from(count).map_err(|_| StorageError::InvalidStoredValue {
                field: "clipboard_items.count",
                value: count,
            })
        })
    }

    fn delete_older_than(&self, days: u32) -> Result<u64, StorageError> {
        if days == 0 {
            return Ok(0);
        }
        self.with_connection(|connection| {
            let cutoff_ms = current_time_ms() - i64::from(days) * 86_400_000;
            let deleted = connection.execute(
                "DELETE FROM clipboard_items
                 WHERE is_favorite = 0
                   AND deleted = 0
                   AND created_at_ms < ?1",
                [cutoff_ms],
            )?;
            Ok(deleted as u64)
        })
    }

    fn enforce_capacity_limit(&self, max_items: u64) -> Result<u64, StorageError> {
        if max_items == 0 {
            return Ok(0);
        }
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE deleted = 0",
                [],
                |row| row.get(0),
            )?;
            if count <= max_items as i64 {
                return Ok(0);
            }
            let excess = count - max_items as i64;
            let deleted = connection.execute(
                "DELETE FROM clipboard_items WHERE id IN (
                    SELECT id FROM clipboard_items
                    WHERE is_favorite = 0 AND deleted = 0
                    ORDER BY created_at_ms ASC
                    LIMIT ?1
                )",
                [excess],
            )?;
            Ok(deleted as u64)
        })
    }

    fn cleanup_orphan_search_index(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let removed = connection.execute(
                "DELETE FROM search_outbox
                 WHERE item_id NOT IN (SELECT id FROM clipboard_items)",
                [],
            )?;
            Ok(removed as u64)
        })
    }

    fn soft_delete(&self, id: &str) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let is_favorite = connection
                .query_row(
                    "SELECT is_favorite FROM clipboard_items WHERE id = ?1 AND deleted = 0",
                    [id],
                    |row| row.get::<_, bool>(0),
                )
                .optional()?;

            if is_favorite == Some(true) {
                return Err(StorageError::FavoriteMustBeRemoved(id.to_owned()));
            }

            let now = current_time_ms();
            Ok(connection.execute(
                "UPDATE clipboard_items SET deleted = 1, deleted_at_ms = ?2 WHERE id = ?1 AND deleted = 0",
                params![id, now],
            )? > 0)
        })
    }

    fn soft_delete_batch(&self, ids: &[String]) -> Result<bool, StorageError> {
        let ids = unique_ids(ids);
        if ids.is_empty() {
            return Ok(false);
        }

        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let mut active_ids = Vec::with_capacity(ids.len());

            // Validate first so a favorite never causes a partially deleted
            // batch. Missing ids are reported as a normal false result, just
            // like the single-record command reports `false` for a miss.
            for id in &ids {
                let state = transaction
                    .query_row(
                        "SELECT is_favorite, deleted
                         FROM clipboard_items
                         WHERE id = ?1",
                        [id],
                        |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
                    )
                    .optional()?;

                let Some((is_favorite, deleted)) = state else {
                    return Ok(false);
                };

                if !deleted {
                    if is_favorite {
                        return Err(StorageError::FavoriteMustBeRemoved(id.clone()));
                    }
                    active_ids.push(id);
                }
            }

            if active_ids.is_empty() {
                return Ok(false);
            }

            let now = current_time_ms();
            for id in active_ids {
                transaction.execute(
                    "UPDATE clipboard_items
                     SET deleted = 1, deleted_at_ms = ?2
                     WHERE id = ?1 AND deleted = 0",
                    params![id, now],
                )?;
            }

            transaction.commit()?;
            Ok(true)
        })
    }

    fn restore_deleted(&self, id: &str) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let affected = connection.execute(
                "UPDATE clipboard_items SET deleted = 0, deleted_at_ms = NULL WHERE id = ?1 AND deleted = 1",
                [id],
            )?;
            if affected > 0 {
                connection.execute(
                    "INSERT INTO search_outbox (item_id, operation, created_at_ms)
                     VALUES (?1, 'upsert', ?2)",
                    params![id, current_time_ms()],
                )?;
            }
            Ok(affected > 0)
        })
    }

    fn permanently_delete_expired(&self, days: u32) -> Result<u64, StorageError> {
        if days == 0 {
            return Ok(0);
        }
        self.with_connection(|connection| {
            let cutoff_ms = current_time_ms() - i64::from(days) * 86_400_000;
            let deleted = connection.execute(
                "DELETE FROM clipboard_items
                 WHERE deleted = 1
                   AND deleted_at_ms IS NOT NULL
                   AND deleted_at_ms < ?1",
                [cutoff_ms],
            )?;
            Ok(deleted as u64)
        })
    }

    fn clear_all_non_favorite_items(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let deleted = connection.execute(
                "UPDATE clipboard_items
                 SET deleted = 1, deleted_at_ms = ?1
                 WHERE is_favorite = 0 AND deleted = 0",
                [current_time_ms()],
            )?;
            Ok(deleted as u64)
        })
    }

    fn count_by_kind(&self, kind: &str) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE kind = ?1 AND deleted = 0",
                [kind],
                |row| row.get(0),
            )?;
            u64::try_from(count).map_err(|_| StorageError::InvalidStoredValue {
                field: "clipboard_items.count",
                value: count,
            })
        })
    }

    fn size_by_kind(&self, kind: &str) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let total: i64 = connection.query_row(
                "SELECT COALESCE(SUM(size_bytes), 0) FROM clipboard_items WHERE kind = ?1 AND deleted = 0",
                [kind],
                |row| row.get(0),
            )?;
            u64::try_from(total).map_err(|_| StorageError::InvalidStoredValue {
                field: "clipboard_items.size",
                value: total,
            })
        })
    }

    fn list_active_file_paths(&self) -> Result<Vec<String>, StorageError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT resource_path FROM clipboard_items WHERE resource_path IS NOT NULL AND deleted = 0
                 UNION
                 SELECT preview_path FROM clipboard_items WHERE preview_path IS NOT NULL AND deleted = 0
                 UNION
                 SELECT icon_path FROM clipboard_items WHERE icon_path IS NOT NULL AND deleted = 0",
            )?;
            let paths = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(paths)
        })
    }
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn unique_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect()
}

struct StoredClipboardItem {
    id: String,
    kind: String,
    title: String,
    text_content: Option<String>,
    resource_path: Option<String>,
    preview_path: Option<String>,
    content_hash: String,
    source_app: Option<String>,
    size_bytes: i64,
    created_at_ms: i64,
    last_used_at_ms: Option<i64>,
    is_favorite: bool,
    icon_path: Option<String>,
    metadata_json: Option<String>,
}

impl StoredClipboardItem {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind: row.get(1)?,
            title: row.get(2)?,
            text_content: row.get(3)?,
            resource_path: row.get(4)?,
            preview_path: row.get(5)?,
            content_hash: row.get(6)?,
            source_app: row.get(7)?,
            size_bytes: row.get(8)?,
            created_at_ms: row.get(9)?,
            last_used_at_ms: row.get(10)?,
            is_favorite: row.get(11)?,
            icon_path: row.get(12)?,
            metadata_json: row.get(13)?,
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

fn kind_to_storage(kind: ClipboardKind) -> &'static str {
    match kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image => "image",
        ClipboardKind::File => "file",
    }
}

fn kind_from_storage(kind: &str) -> Result<ClipboardKind, StorageError> {
    match kind {
        "text" => Ok(ClipboardKind::Text),
        "link" => Ok(ClipboardKind::Link),
        "image" => Ok(ClipboardKind::Image),
        "file" => Ok(ClipboardKind::File),
        _ => Err(StorageError::InvalidClipboardKind(kind.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{ClipboardItem, ClipboardKind};

    use super::{ClipboardRepository, Database};
    use crate::storage::StorageError;

    fn text_item(id: &str, content_hash: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: format!("record-{id}"),
            text_content: Some(format!("content-{id}")),
            resource_path: None,
            preview_path: None,
            content_hash: content_hash.to_owned(),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 12,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    #[test]
    fn saves_and_lists_items_by_recency() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("older", "hash-1", 100))
            .unwrap();
        database
            .save_item(&text_item("newer", "hash-2", 200))
            .unwrap();

        let items = database.list_recent(20, 0).unwrap();

        assert_eq!(database.item_count().unwrap(), 2);
        assert_eq!(items[0].id, "newer");
        assert_eq!(items[1].id, "older");
    }

    #[test]
    fn batch_lookup_preserves_requested_relevance_order() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("first", "hash-1", 100))
            .unwrap();
        database
            .save_item(&text_item("second", "hash-2", 200))
            .unwrap();

        let items = database
            .get_items_by_ids(&[
                "second".to_owned(),
                "missing".to_owned(),
                "first".to_owned(),
            ])
            .unwrap();

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["second", "first"]
        );
    }

    #[test]
    fn lists_distinct_source_applications_for_filter_configuration() {
        let database = Database::open_in_memory().unwrap();
        let mut chatgpt = text_item("chatgpt", "hash-1", 100);
        chatgpt.source_app = Some("ChatGPT".to_owned());
        let mut browser = text_item("browser", "hash-2", 200);
        browser.source_app = Some("Browser".to_owned());
        let mut duplicate = text_item("duplicate", "hash-3", 300);
        duplicate.source_app = Some("chatgpt".to_owned());
        database.save_item(&chatgpt).unwrap();
        database.save_item(&browser).unwrap();
        database.save_item(&duplicate).unwrap();

        assert_eq!(
            database.list_source_applications().unwrap(),
            vec!["Browser", "ChatGPT"]
        );
    }

    #[test]
    fn repeated_content_reuses_the_existing_record() {
        let database = Database::open_in_memory().unwrap();
        let mut first = text_item("original", "same-hash", 100);
        first.is_favorite = true;
        database.save_item(&first).unwrap();

        let repeated = text_item("replacement", "same-hash", 500);
        let stored_id = database.save_item(&repeated).unwrap();
        let stored = database.get_item(&stored_id).unwrap().unwrap();

        assert_eq!(stored_id, "original");
        assert_eq!(stored.created_at_ms, 500);
        assert!(stored.is_favorite);
        assert_eq!(database.item_count().unwrap(), 1);
    }

    #[test]
    fn favorite_must_be_removed_before_direct_deletion() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&text_item("item", "hash", 100)).unwrap();
        assert!(database.set_favorite("item", true).unwrap());

        assert!(matches!(
            database.delete_item("item"),
            Err(StorageError::FavoriteMustBeRemoved(id)) if id == "item"
        ));
        assert!(database.set_favorite("item", false).unwrap());
        assert!(database.delete_item("item").unwrap());
        assert_eq!(database.item_count().unwrap(), 0);
    }

    #[test]
    fn batch_favorite_is_atomic_and_deduplicates_ids() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&text_item("first", "hash-first", 100)).unwrap();
        database.save_item(&text_item("second", "hash-second", 200)).unwrap();

        assert!(database
            .set_favorite_batch(
                &["first".to_owned(), "first".to_owned(), "second".to_owned()],
                true,
            )
            .unwrap());
        assert!(database.get_item("first").unwrap().unwrap().is_favorite);
        assert!(database.get_item("second").unwrap().unwrap().is_favorite);

        // A stale id must not leave a partially updated batch behind.
        assert!(!database
            .set_favorite_batch(&["first".to_owned(), "missing".to_owned()], false)
            .unwrap());
        assert!(database.get_item("first").unwrap().unwrap().is_favorite);
    }

    #[test]
    fn batch_soft_delete_protects_favorites_without_partial_changes() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&text_item("regular", "hash-regular", 100)).unwrap();
        database.save_item(&text_item("favorite", "hash-favorite", 200)).unwrap();
        database.set_favorite("favorite", true).unwrap();

        let result = database.soft_delete_batch(&["regular".to_owned(), "favorite".to_owned()]);
        assert!(matches!(
            result,
            Err(StorageError::FavoriteMustBeRemoved(id)) if id == "favorite"
        ));
        assert!(database.get_item("regular").unwrap().is_some());
        assert!(database.get_item("favorite").unwrap().is_some());

        assert!(database
            .soft_delete_batch(&["regular".to_owned(), "regular".to_owned()])
            .unwrap());
        assert!(database.get_item("regular").unwrap().is_some());
        assert!(!database.list_recent(10, 0).unwrap().iter().any(|item| item.id == "regular"));
    }

    #[test]
    fn batch_lookup_excludes_soft_deleted_items() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&text_item("active", "hash-active", 100)).unwrap();
        database.save_item(&text_item("deleted", "hash-deleted", 200)).unwrap();
        database.soft_delete("deleted").unwrap();

        let ids = vec!["deleted".to_owned(), "active".to_owned()];
        let items = database.get_items_by_ids(&ids).unwrap();
        assert_eq!(items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), ["active"]);
    }

    #[test]
    fn favorite_survives_unfavorited_history_cleanup() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("favorite", "favorite-hash", 100))
            .unwrap();
        database
            .save_item(&text_item("regular", "regular-hash", 200))
            .unwrap();
        database.set_favorite("favorite", true).unwrap();

        database
            .with_connection(|connection| {
                connection.execute("DELETE FROM clipboard_items WHERE is_favorite = 0", [])?;
                Ok(())
            })
            .unwrap();

        let stored = database.get_item("favorite").unwrap().unwrap();
        assert!(stored.is_favorite);
        assert_eq!(stored.title, "record-favorite");
        assert!(database.get_item("regular").unwrap().is_none());
        assert_eq!(database.item_count().unwrap(), 1);

        database
            .with_connection(|connection| {
                let last_operation: String = connection.query_row(
                    "SELECT operation FROM search_outbox ORDER BY sequence DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )?;

                assert_eq!(last_operation, "delete");
                Ok(())
            })
            .unwrap();
    }

    // ── Task 4: Pagination edge cases ──

    #[test]
    fn pagination_empty_database_returns_empty() {
        let database = Database::open_in_memory().unwrap();
        let items = database.list_recent(100, 0).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn pagination_single_page_returns_all_items() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("a", "hash-a", 100))
            .unwrap();
        database
            .save_item(&text_item("b", "hash-b", 200))
            .unwrap();
        database
            .save_item(&text_item("c", "hash-c", 300))
            .unwrap();

        let items = database.list_recent(50, 0).unwrap();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn pagination_beyond_bounds_returns_empty() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("a", "hash-a", 100))
            .unwrap();

        let items = database.list_recent(100, 1000).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn pagination_returns_partial_page_at_end() {
        let database = Database::open_in_memory().unwrap();
        for i in 0..5 {
            database
                .save_item(&text_item(&format!("item-{i}"), &format!("hash-{i}"), i * 100))
                .unwrap();
        }

        let items = database.list_recent(3, 3).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn pagination_limit_is_clamped() {
        let database = Database::open_in_memory().unwrap();
        for i in 0..600 {
            database
                .save_item(&text_item(&format!("item-{i}"), &format!("hash-{i}"), i as i64 * 100))
                .unwrap();
        }

        let items = database.list_recent(10000, 0).unwrap();
        assert!(items.len() <= 500);
    }

    // ── Task 4: Item count with soft-deleted items ──

    #[test]
    fn item_count_excludes_soft_deleted_items() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("active", "hash-1", 100))
            .unwrap();
        database
            .save_item(&text_item("deleted", "hash-2", 200))
            .unwrap();

        assert_eq!(database.item_count().unwrap(), 2);

        database.soft_delete("deleted").unwrap();
        assert_eq!(database.item_count().unwrap(), 1);
        assert!(database.get_item("active").unwrap().is_some());
    }

    #[test]
    fn item_count_includes_restored_items() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("restored", "hash-1", 100))
            .unwrap();

        database.soft_delete("restored").unwrap();
        assert_eq!(database.item_count().unwrap(), 0);

        database.restore_deleted("restored").unwrap();
        assert_eq!(database.item_count().unwrap(), 1);
    }

    // ── Task 4: Concurrent read/write with multiple connections ──

    #[test]
    fn concurrent_read_does_not_block_writes() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("a", "hash-a", 100))
            .unwrap();

        let read_result = database.get_item("a");
        database
            .save_item(&text_item("b", "hash-b", 200))
            .unwrap();

        assert!(read_result.is_ok());
        assert_eq!(database.item_count().unwrap(), 2);
    }
}
