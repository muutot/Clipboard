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
const ITEM_LOOKUP_CHUNK_SIZE: usize = 500;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StorageFileReferences {
    pub resource_paths: Vec<String>,
    pub preview_paths: Vec<String>,
    pub icon_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindDeleteScope {
    pub include_favorites: bool,
    pub include_deleted: bool,
}

impl KindDeleteScope {
    /// Includes active records, favorites, and records already in the recycle bin.
    pub const fn all() -> Self {
        Self {
            include_favorites: true,
            include_deleted: true,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KindStorageStats {
    pub item_count: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct KindDeleteResult {
    pub stats: KindStorageStats,
    pub deleted_ids: Vec<String>,
}

pub struct TextItemUpdate<'a> {
    pub id: &'a str,
    pub kind: ClipboardKind,
    pub title: &'a str,
    pub text_content: &'a str,
    pub content_hash: &'a str,
    pub size_bytes: u64,
    pub metadata_json: Option<&'a str>,
}

pub trait ClipboardRepository {
    fn save_item(&self, item: &ClipboardItem) -> Result<String, StorageError>;
    /// Atomically replaces the textual payload of an active text/link item.
    ///
    /// Keeping this operation in the repository ensures the content hash and
    /// byte count are updated together with the text.  SQLite's UNIQUE(kind,
    /// content_hash) constraint rejects an edit that would collide with a
    /// different history record without partially updating either row.
    fn update_text_item(&self, update: &TextItemUpdate<'_>) -> Result<bool, StorageError>;
    fn get_item(&self, id: &str) -> Result<Option<ClipboardItem>, StorageError>;
    fn get_items_by_ids(&self, ids: &[String]) -> Result<Vec<ClipboardItem>, StorageError>;
    fn list_recent(&self, limit: u32, offset: u32) -> Result<Vec<ClipboardItem>, StorageError>;
    /// Lists soft-deleted records for the recycle-bin view.
    fn list_deleted(&self, limit: u32, offset: u32) -> Result<Vec<ClipboardItem>, StorageError>;
    fn list_source_applications(&self) -> Result<Vec<String>, StorageError>;
    fn list_source_applications_with_icons(
        &self,
    ) -> Result<Vec<(String, Option<String>)>, StorageError>;
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
    /// Restores all requested soft-deleted records atomically.
    fn restore_deleted_batch(&self, ids: &[String]) -> Result<bool, StorageError>;
    /// Permanently removes one already soft-deleted record.
    fn permanently_delete(&self, id: &str) -> Result<bool, StorageError>;
    /// Permanently removes requested soft-deleted records atomically.
    fn permanently_delete_batch(&self, ids: &[String]) -> Result<bool, StorageError>;
    fn permanently_delete_expired(&self, days: u32) -> Result<u64, StorageError>;
    fn clear_all_non_favorite_items(&self) -> Result<u64, StorageError>;
    fn count_by_kind(&self, kind: &str) -> Result<u64, StorageError>;
    fn size_by_kind(&self, kind: &str) -> Result<u64, StorageError>;
    /// Returns the count and logical byte size for one kind and explicit scope.
    fn kind_storage_stats(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
    ) -> Result<KindStorageStats, StorageError>;
    /// Permanently deletes every record matching one kind and explicit scope.
    ///
    /// `include_favorites` controls whether favorite records may be removed and
    /// `include_deleted` controls whether records already in the recycle bin
    /// are included. `KindDeleteScope::all()` therefore deletes the complete
    /// category, including favorites and recycle-bin records. The returned
    /// statistics and sorted ids are derived from the rows actually deleted.
    ///
    /// SQLite's delete trigger queues a search-index delete for every removed
    /// row, while the OCR foreign key removes associated OCR data. Filesystem
    /// resources must subsequently be reclaimed by the ownership-aware orphan
    /// cleanup after this transaction commits.
    fn permanently_delete_by_kind(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
    ) -> Result<KindDeleteResult, StorageError>;
    /// Deletes a category only when its current statistics still match the
    /// values shown in the destructive confirmation dialog.
    fn permanently_delete_by_kind_if_stats_match(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
        expected: KindStorageStats,
    ) -> Result<KindDeleteResult, StorageError>;
    /// Returns every filesystem reference still owned by a database record.
    ///
    /// Soft-deleted records remain recoverable, so their resources must stay
    /// referenced until the record is permanently removed.
    fn list_storage_file_references(&self) -> Result<StorageFileReferences, StorageError>;
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

    fn update_text_item(&self, update: &TextItemUpdate<'_>) -> Result<bool, StorageError> {
        let size_bytes =
            i64::try_from(update.size_bytes).map_err(|_| StorageError::ValueOutOfRange {
                field: "size_bytes",
            })?;

        self.with_connection(|connection| {
            let changed = connection.execute(
                "UPDATE clipboard_items
                 SET kind = ?2,
                     title = ?3,
                     text_content = ?4,
                     content_hash = ?5,
                     size_bytes = ?6,
                     metadata_json = COALESCE(?7, metadata_json)
                 WHERE id = ?1
                   AND deleted = 0
                   AND kind IN ('text', 'link')",
                params![
                    update.id,
                    kind_to_storage(update.kind),
                    update.title,
                    update.text_content,
                    update.content_hash,
                    size_bytes,
                    update.metadata_json,
                ],
            )?;
            Ok(changed == 1)
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
            let mut items_by_id = HashMap::with_capacity(ids.len());
            for chunk in ids.chunks(ITEM_LOOKUP_CHUNK_SIZE) {
                let placeholders = (1..=chunk.len())
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
                    .query_map(
                        params_from_iter(chunk.iter()),
                        StoredClipboardItem::from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                for stored_item in stored_items {
                    let item = ClipboardItem::try_from(stored_item)?;
                    items_by_id.insert(item.id.clone(), item);
                }
            }

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
                    params![i64::from(limit), i64::from(offset)],
                    StoredClipboardItem::from_row,
                )?
                .collect::<Result<Vec<_>, _>>()?;

            stored_items.into_iter().map(TryInto::try_into).collect()
        })
    }

    fn list_deleted(&self, limit: u32, offset: u32) -> Result<Vec<ClipboardItem>, StorageError> {
        self.with_connection(|connection| {
            let sql = format!(
                "SELECT {ITEM_COLUMNS}
                 FROM clipboard_items
                 WHERE deleted = 1
                 ORDER BY COALESCE(deleted_at_ms, created_at_ms) DESC, created_at_ms DESC
                 LIMIT ?1 OFFSET ?2"
            );
            let mut statement = connection.prepare_cached(&sql)?;
            let stored_items = statement
                .query_map(
                    params![i64::from(limit), i64::from(offset)],
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

    fn list_source_applications_with_icons(
        &self,
    ) -> Result<Vec<(String, Option<String>)>, StorageError> {
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
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
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

            let placeholders: Vec<String> = ids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect();
            let where_clause = format!("id IN ({})", placeholders.join(", "));
            let count: i64 = transaction.query_row(
                &format!("SELECT COUNT(*) FROM clipboard_items WHERE {where_clause}"),
                params_from_iter(ids.iter().map(|id| id.as_str())),
                |row| row.get(0),
            )?;
            if (count as usize) < ids.len() {
                return Ok(false);
            }

            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = ids
                .iter()
                .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
                .collect();
            params.push(Box::new(is_favorite));
            let fav_pos = ids.len() + 1;
            let set_clause = format!("is_favorite = ?{fav_pos}");
            transaction.execute(
                &format!("UPDATE clipboard_items SET {set_clause} WHERE {where_clause}"),
                params_from_iter(params.iter().map(|p| p.as_ref())),
            )?;

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
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE deleted = 0",
                [],
                |row| row.get(0),
            )?;

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

    fn restore_deleted_batch(&self, ids: &[String]) -> Result<bool, StorageError> {
        let ids = unique_ids(ids);
        if ids.is_empty() {
            return Ok(false);
        }

        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            for id in &ids {
                let exists = transaction
                    .query_row(
                        "SELECT 1 FROM clipboard_items WHERE id = ?1 AND deleted = 1",
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
                    "UPDATE clipboard_items
                     SET deleted = 0, deleted_at_ms = NULL
                     WHERE id = ?1 AND deleted = 1",
                    [id],
                )?;
            }
            transaction.commit()?;
            Ok(true)
        })
    }

    fn permanently_delete(&self, id: &str) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "DELETE FROM clipboard_items WHERE id = ?1 AND deleted = 1",
                [id],
            )? > 0)
        })
    }

    fn permanently_delete_batch(&self, ids: &[String]) -> Result<bool, StorageError> {
        let ids = unique_ids(ids);
        if ids.is_empty() {
            return Ok(false);
        }

        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            for id in &ids {
                let exists = transaction
                    .query_row(
                        "SELECT 1 FROM clipboard_items WHERE id = ?1 AND deleted = 1",
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
                    "DELETE FROM clipboard_items WHERE id = ?1 AND deleted = 1",
                    [id],
                )?;
            }
            transaction.commit()?;
            Ok(true)
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

    fn kind_storage_stats(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
    ) -> Result<KindStorageStats, StorageError> {
        self.with_connection(|connection| query_kind_storage_stats(connection, kind, scope))
    }

    fn permanently_delete_by_kind(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
    ) -> Result<KindDeleteResult, StorageError> {
        self.with_connection(|connection| delete_kind_records(connection, kind, scope, None))
    }

    fn permanently_delete_by_kind_if_stats_match(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
        expected: KindStorageStats,
    ) -> Result<KindDeleteResult, StorageError> {
        self.with_connection(|connection| {
            delete_kind_records(connection, kind, scope, Some(expected))
        })
    }

    fn list_storage_file_references(&self) -> Result<StorageFileReferences, StorageError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT 'resource', resource_path
                 FROM clipboard_items
                 WHERE resource_path IS NOT NULL
                 UNION ALL
                 SELECT 'preview', preview_path
                 FROM clipboard_items
                 WHERE preview_path IS NOT NULL
                 UNION ALL
                 SELECT 'icon', icon_path
                 FROM clipboard_items
                 WHERE icon_path IS NOT NULL",
            )?;
            let references = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut paths = StorageFileReferences::default();
            for reference in references {
                let (kind, path) = reference?;
                match kind.as_str() {
                    "resource" => paths.resource_paths.push(path),
                    "preview" => paths.preview_paths.push(path),
                    "icon" => paths.icon_paths.push(path),
                    _ => unreachable!("storage reference query returned an unknown kind"),
                }
            }

            let mut file_paths = connection.prepare(
                "SELECT text_content
                 FROM clipboard_items
                 WHERE kind = 'file'
                   AND text_content IS NOT NULL",
            )?;
            let file_paths = file_paths.query_map([], |row| row.get::<_, String>(0))?;
            for stored_paths in file_paths {
                if let Ok(stored_paths) = serde_json::from_str::<Vec<String>>(&stored_paths?) {
                    paths.resource_paths.extend(
                        stored_paths
                            .into_iter()
                            .filter(|path| !path.trim().is_empty()),
                    );
                }
            }
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
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

fn query_kind_storage_stats(
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

fn delete_kind_records(
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
    use crate::domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus};

    use super::{
        ClipboardRepository, Database, KindDeleteResult, KindDeleteScope, KindStorageStats,
        TextItemUpdate,
    };
    use crate::storage::{OcrRepository, SearchOperation, SearchRepository, StorageError};

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
    fn batch_lookup_reads_more_than_one_query_chunk() {
        let database = Database::open_in_memory().unwrap();
        let ids = (0..=500)
            .map(|index| format!("item-{index:03}"))
            .collect::<Vec<_>>();
        for (index, id) in ids.iter().enumerate() {
            database
                .save_item(&text_item(id, &format!("hash-{index}"), index as i64))
                .unwrap();
        }

        let requested_ids = ids.into_iter().rev().collect::<Vec<_>>();
        let items = database.get_items_by_ids(&requested_ids).unwrap();

        assert_eq!(items.len(), 501);
        assert_eq!(
            items.into_iter().map(|item| item.id).collect::<Vec<_>>(),
            requested_ids
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
    fn storage_references_include_every_path_from_multi_file_records() {
        let database = Database::open_in_memory().unwrap();
        let item = ClipboardItem {
            id: "files".to_owned(),
            kind: ClipboardKind::File,
            title: "first.txt".to_owned(),
            text_content: Some(
                serde_json::to_string(&["C:\\managed\\first.txt", "C:\\managed\\second.txt"])
                    .unwrap(),
            ),
            resource_path: Some("C:\\managed\\first.txt".to_owned()),
            preview_path: None,
            content_hash: "files-hash".to_owned(),
            source_app: Some("Explorer".to_owned()),
            size_bytes: 20,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        };
        database.save_item(&item).unwrap();

        let references = database.list_storage_file_references().unwrap();

        assert!(references
            .resource_paths
            .contains(&"C:\\managed\\first.txt".to_owned()));
        assert!(references
            .resource_paths
            .contains(&"C:\\managed\\second.txt".to_owned()));
    }

    #[test]
    fn update_text_item_replaces_payload_and_preserves_record_metadata() {
        let database = Database::open_in_memory().unwrap();
        let mut original = text_item("editable", "old-hash", 100);
        original.last_used_at_ms = Some(150);
        original.is_favorite = true;
        original.icon_path = Some("icons/test.png".to_owned());
        original.metadata_json = Some(r#"{"custom":"value"}"#.to_owned());
        database.save_item(&original).unwrap();

        assert!(database
            .update_text_item(&TextItemUpdate {
                id: "editable",
                kind: ClipboardKind::Link,
                title: "updated title",
                text_content: "https://example.com",
                content_hash: "new-hash",
                size_bytes: 19,
                metadata_json: None,
            })
            .unwrap());

        let saved = database.get_item("editable").unwrap().unwrap();
        assert_eq!(saved.kind, ClipboardKind::Link);
        assert_eq!(saved.title, "updated title");
        assert_eq!(saved.text_content.as_deref(), Some("https://example.com"));
        assert_eq!(saved.content_hash, "new-hash");
        assert_eq!(saved.size_bytes, 19);
        assert_eq!(saved.source_app, original.source_app);
        assert_eq!(saved.created_at_ms, original.created_at_ms);
        assert_eq!(saved.last_used_at_ms, original.last_used_at_ms);
        assert_eq!(saved.is_favorite, original.is_favorite);
        assert_eq!(saved.icon_path, original.icon_path);
        assert_eq!(saved.metadata_json, original.metadata_json);
        assert_eq!(database.read_search_outbox(20).unwrap().len(), 2);
    }

    #[test]
    fn update_text_item_can_replace_metadata_without_dropping_the_record() {
        let database = Database::open_in_memory().unwrap();
        let mut original = text_item("metadata-edit", "old-metadata-hash", 100);
        original.metadata_json = Some(r#"{"width":120,"custom":"value"}"#.to_owned());
        database.save_item(&original).unwrap();

        assert!(database
            .update_text_item(&TextItemUpdate {
                id: "metadata-edit",
                kind: ClipboardKind::Text,
                title: "Custom heading",
                text_content: "body text",
                content_hash: "new-metadata-hash",
                size_bytes: 9,
                metadata_json: Some(r#"{"width":120,"custom":"value","customTitle":true}"#),
            })
            .unwrap());

        let saved = database.get_item("metadata-edit").unwrap().unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(saved.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(metadata["width"], 120);
        assert_eq!(metadata["custom"], "value");
        assert_eq!(metadata["customTitle"], true);
    }

    #[test]
    fn update_text_item_rejects_hash_collisions_without_partial_changes() {
        let database = Database::open_in_memory().unwrap();
        let original = text_item("original", "original-hash", 100);
        let existing = text_item("existing", "existing-hash", 200);
        database.save_item(&original).unwrap();
        database.save_item(&existing).unwrap();

        let result = database.update_text_item(&TextItemUpdate {
            id: "original",
            kind: ClipboardKind::Text,
            title: "colliding title",
            text_content: "colliding content",
            content_hash: "existing-hash",
            size_bytes: 17,
            metadata_json: None,
        });

        assert!(matches!(result, Err(StorageError::Sqlite(_))));
        assert_eq!(database.get_item("original").unwrap().unwrap(), original);
        assert_eq!(database.read_search_outbox(20).unwrap().len(), 2);
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
        database
            .save_item(&text_item("first", "hash-first", 100))
            .unwrap();
        database
            .save_item(&text_item("second", "hash-second", 200))
            .unwrap();

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
        database
            .save_item(&text_item("regular", "hash-regular", 100))
            .unwrap();
        database
            .save_item(&text_item("favorite", "hash-favorite", 200))
            .unwrap();
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
        assert!(!database
            .list_recent(10, 0)
            .unwrap()
            .iter()
            .any(|item| item.id == "regular"));
    }

    #[test]
    fn batch_lookup_excludes_soft_deleted_items() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("active", "hash-active", 100))
            .unwrap();
        database
            .save_item(&text_item("deleted", "hash-deleted", 200))
            .unwrap();
        database.soft_delete("deleted").unwrap();

        let ids = vec!["deleted".to_owned(), "active".to_owned()];
        let items = database.get_items_by_ids(&ids).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["active"]
        );
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

    #[test]
    fn kind_deletion_scope_controls_favorites_and_recycle_bin_records() {
        let database = Database::open_in_memory().unwrap();
        let mut active = text_item("active", "hash-active", 100);
        active.size_bytes = 10;
        let mut favorite = text_item("favorite", "hash-favorite", 200);
        favorite.size_bytes = 20;
        favorite.is_favorite = true;
        let mut recycled = text_item("recycled", "hash-recycled", 300);
        recycled.size_bytes = 30;
        let mut link = text_item("link", "hash-link", 400);
        link.kind = ClipboardKind::Link;
        link.size_bytes = 40;
        database.save_item(&active).unwrap();
        database.save_item(&favorite).unwrap();
        database.save_item(&recycled).unwrap();
        database.save_item(&link).unwrap();
        database.soft_delete("recycled").unwrap();

        assert_eq!(
            database
                .kind_storage_stats(
                    ClipboardKind::Text,
                    KindDeleteScope {
                        include_favorites: false,
                        include_deleted: false,
                    },
                )
                .unwrap(),
            KindStorageStats {
                item_count: 1,
                size_bytes: 10,
            }
        );
        assert_eq!(
            database
                .kind_storage_stats(
                    ClipboardKind::Text,
                    KindDeleteScope {
                        include_favorites: true,
                        include_deleted: false,
                    },
                )
                .unwrap(),
            KindStorageStats {
                item_count: 2,
                size_bytes: 30,
            }
        );
        assert_eq!(
            database
                .kind_storage_stats(
                    ClipboardKind::Text,
                    KindDeleteScope {
                        include_favorites: false,
                        include_deleted: true,
                    },
                )
                .unwrap(),
            KindStorageStats {
                item_count: 2,
                size_bytes: 40,
            }
        );
        assert_eq!(
            database
                .kind_storage_stats(ClipboardKind::Text, KindDeleteScope::all())
                .unwrap(),
            KindStorageStats {
                item_count: 3,
                size_bytes: 60,
            }
        );

        let deleted = database
            .permanently_delete_by_kind(
                ClipboardKind::Text,
                KindDeleteScope {
                    include_favorites: false,
                    include_deleted: true,
                },
            )
            .unwrap();

        assert_eq!(
            deleted,
            KindDeleteResult {
                stats: KindStorageStats {
                    item_count: 2,
                    size_bytes: 40,
                },
                deleted_ids: vec!["active".to_owned(), "recycled".to_owned()],
            }
        );
        assert!(database.get_item("active").unwrap().is_none());
        assert!(database.get_item("recycled").unwrap().is_none());
        assert!(database.get_item("favorite").unwrap().is_some());
        assert!(database.get_item("link").unwrap().is_some());
    }

    #[test]
    fn kind_deletion_cascades_ocr_queues_search_deletes_and_drops_references() {
        let database = Database::open_in_memory().unwrap();
        let image = ClipboardItem {
            id: "image".to_owned(),
            kind: ClipboardKind::Image,
            title: "captured image".to_owned(),
            text_content: None,
            resource_path: Some("C:\\managed\\image.png".to_owned()),
            preview_path: Some("C:\\managed\\preview.jpg".to_owned()),
            content_hash: "image-hash".to_owned(),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 25,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: true,
            icon_path: None,
            metadata_json: None,
        };
        database.save_item(&image).unwrap();
        database
            .save_ocr_result(&OcrResult {
                item_id: image.id.clone(),
                status: OcrStatus::Completed,
                engine: "test".to_owned(),
                model_version: "1".to_owned(),
                language: Some("en".to_owned()),
                full_text: "recognized".to_owned(),
                blocks: Vec::new(),
                image_hash: image.content_hash.clone(),
                created_at_ms: 100,
                completed_at_ms: Some(200),
                error_message: None,
            })
            .unwrap();
        database
            .with_connection(|connection| {
                connection.execute("DELETE FROM search_outbox", [])?;
                Ok(())
            })
            .unwrap();

        let deleted = database
            .permanently_delete_by_kind(ClipboardKind::Image, KindDeleteScope::all())
            .unwrap();

        assert_eq!(
            deleted,
            KindDeleteResult {
                stats: KindStorageStats {
                    item_count: 1,
                    size_bytes: 25,
                },
                deleted_ids: vec!["image".to_owned()],
            }
        );
        assert!(database.get_item("image").unwrap().is_none());
        assert!(database.get_ocr_result("image").unwrap().is_none());
        let events = database.read_search_outbox(20).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].item_id, "image");
        assert_eq!(events[0].operation, SearchOperation::Delete);
        assert_eq!(
            database.list_storage_file_references().unwrap(),
            Default::default()
        );
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
        database.save_item(&text_item("a", "hash-a", 100)).unwrap();
        database.save_item(&text_item("b", "hash-b", 200)).unwrap();
        database.save_item(&text_item("c", "hash-c", 300)).unwrap();

        let items = database.list_recent(50, 0).unwrap();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn pagination_beyond_bounds_returns_empty() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&text_item("a", "hash-a", 100)).unwrap();

        let items = database.list_recent(100, 1000).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn pagination_returns_partial_page_at_end() {
        let database = Database::open_in_memory().unwrap();
        for i in 0..5 {
            database
                .save_item(&text_item(
                    &format!("item-{i}"),
                    &format!("hash-{i}"),
                    i * 100,
                ))
                .unwrap();
        }

        let items = database.list_recent(3, 3).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn pagination_limit_is_respected() {
        let database = Database::open_in_memory().unwrap();
        for i in 0..600 {
            database
                .save_item(&text_item(
                    &format!("item-{i}"),
                    &format!("hash-{i}"),
                    i as i64 * 100,
                ))
                .unwrap();
        }

        let items = database.list_recent(100, 0).unwrap();
        assert_eq!(items.len(), 100);
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

    #[test]
    fn deleted_records_are_listed_in_recency_order() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&text_item("older", "hash-1", 100))
            .unwrap();
        database
            .save_item(&text_item("newer", "hash-2", 200))
            .unwrap();
        database.soft_delete("older").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        database.soft_delete("newer").unwrap();

        let deleted = database.list_deleted(20, 0).unwrap();
        assert_eq!(
            deleted
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["newer", "older"]
        );
        assert!(database.list_recent(20, 0).unwrap().is_empty());
    }

    #[test]
    fn batch_restore_and_permanent_delete_are_atomic() {
        let database = Database::open_in_memory().unwrap();
        for id in ["one", "two", "three"] {
            database
                .save_item(&text_item(id, &format!("hash-{id}"), 100))
                .unwrap();
            database.soft_delete(id).unwrap();
        }

        assert!(!database
            .restore_deleted_batch(&["one".to_owned(), "missing".to_owned()])
            .unwrap());
        assert_eq!(database.list_deleted(20, 0).unwrap().len(), 3);

        assert!(database
            .restore_deleted_batch(&["one".to_owned(), "two".to_owned()])
            .unwrap());
        assert_eq!(database.list_deleted(20, 0).unwrap().len(), 1);

        assert!(!database
            .permanently_delete_batch(&["three".to_owned(), "missing".to_owned()])
            .unwrap());
        assert!(database.get_item("three").unwrap().is_some());
        assert!(database.permanently_delete("three").unwrap());
        assert!(database.get_item("three").unwrap().is_none());
    }

    // ── Task 4: Concurrent read/write with multiple connections ──

    #[test]
    fn concurrent_read_does_not_block_writes() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&text_item("a", "hash-a", 100)).unwrap();

        let read_result = database.get_item("a");
        database.save_item(&text_item("b", "hash-b", 200)).unwrap();

        assert!(read_result.is_ok());
        assert_eq!(database.item_count().unwrap(), 2);
    }
}
