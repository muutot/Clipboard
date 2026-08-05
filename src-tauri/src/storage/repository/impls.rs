use std::collections::HashMap;

use rusqlite::{params, params_from_iter, OptionalExtension};

use super::{
    current_time_ms, delete_kind_records, kind_to_storage, query_kind_storage_stats, unique_ids,
    ClipboardRepository, KindDeleteResult, KindDeleteScope, KindStorageStats,
    StorageFileReferences, StoredClipboardItem, TextItemUpdate, ITEM_COLUMNS,
    ITEM_LOOKUP_CHUNK_SIZE,
};
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::storage::{Database, StorageError};

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
                    html_content,
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
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
                 )
                 ON CONFLICT DO UPDATE SET
                    title = excluded.title,
                    text_content = excluded.text_content,
                    html_content = COALESCE(
                        excluded.html_content,
                        clipboard_items.html_content
                    ),
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
                    item.html_content,
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

    fn set_tags(&self, id: &str, tags: &[String]) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let existing: Option<Option<String>> = connection
                .query_row(
                    "SELECT metadata_json FROM clipboard_items WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(metadata_json) = existing else {
                return Ok(false);
            };

            let mut object: serde_json::Value = metadata_json
                .as_deref()
                .and_then(|json| serde_json::from_str(json).ok())
                .filter(serde_json::Value::is_object)
                .unwrap_or_else(|| serde_json::Value::Object(Default::default()));

            let mut seen = std::collections::HashSet::new();
            let dedup: Vec<String> = tags
                .iter()
                .map(|tag| tag.trim().to_owned())
                .filter(|tag| !tag.is_empty() && seen.insert(tag.clone()))
                .collect();

            if let serde_json::Value::Object(map) = &mut object {
                if dedup.is_empty() {
                    map.remove("tags");
                } else {
                    map.insert(
                        "tags".to_owned(),
                        serde_json::Value::Array(
                            dedup.into_iter().map(serde_json::Value::String).collect(),
                        ),
                    );
                }
            }
            let updated = object.to_string();

            Ok(connection.execute(
                "UPDATE clipboard_items SET metadata_json = ?2 WHERE id = ?1 AND deleted = 0",
                params![id, updated],
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
            // The `clipboard_items_search_update` trigger already enqueues the
            // upsert event for the restored row, so no explicit outbox insert
            // is needed here.
            let affected = connection.execute(
                "UPDATE clipboard_items SET deleted = 0, deleted_at_ms = NULL WHERE id = ?1 AND deleted = 1",
                [id],
            )?;
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
