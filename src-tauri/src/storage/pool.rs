use rusqlite::OptionalExtension;

use super::{Database, StorageError, StorageFileReferences};
use crate::domain::ClipboardItem;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bincode::Encode, bincode::Decode)]
pub struct SyncChangeLogEntry {
    pub sequence: i64,
    pub item_id: String,
    pub operation: String,
    pub kind: String,
    pub title: String,
    pub content_hash: String,
    pub resource_path: Option<String>,
    pub preview_path: Option<String>,
    pub icon_path: Option<String>,
    pub text_content: Option<String>,
    pub html_content: Option<String>,
    pub rtf_content: Option<String>,
    pub metadata_json: Option<String>,
    #[serde(default)]
    pub is_favorite: bool,
    pub source_app: Option<String>,
    #[serde(default)]
    pub size_bytes: i64,
    pub last_used_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub modified_at_ms: i64,
    pub device_id: String,
}

impl Database {
    pub fn set_preview_path(
        &self,
        item_id: &str,
        preview_path: &str,
    ) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let affected = connection.execute(
                "UPDATE clipboard_items SET preview_path = ?2 WHERE id = ?1",
                rusqlite::params![item_id, preview_path],
            )?;
            Ok(affected > 0)
        })
    }

    pub fn checkpoint(&self) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
            Ok(())
        })
    }

    pub fn list_item_ids_since(&self, since_ms: i64) -> Result<Vec<String>, StorageError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id FROM clipboard_items WHERE created_at_ms > ?1 ORDER BY created_at_ms ASC",
            )?;
            let rows = statement.query_map([since_ms], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
        })
    }

    pub fn list_storage_file_references_for_items(
        &self,
        item_ids: &[String],
    ) -> Result<StorageFileReferences, StorageError> {
        self.with_connection(|connection| {
            let mut paths = StorageFileReferences::default();
            if item_ids.is_empty() {
                return Ok(paths);
            }

            let placeholders = item_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT resource_path, preview_path, icon_path
                 FROM clipboard_items
                 WHERE id IN ({placeholders})
                   AND (resource_path IS NOT NULL OR preview_path IS NOT NULL OR icon_path IS NOT NULL)",
            );
            let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
            for id in item_ids {
                params.push(id as &dyn rusqlite::ToSql);
            }
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(rusqlite::params_from_iter(params.iter()), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?;
            for row in rows {
                let (resource, preview, icon) = row?;
                if let Some(p) = resource {
                    if !p.trim().is_empty() {
                        paths.resource_paths.push(p);
                    }
                }
                if let Some(p) = preview {
                    if !p.trim().is_empty() {
                        paths.preview_paths.push(p);
                    }
                }
                if let Some(p) = icon {
                    if !p.trim().is_empty() {
                        paths.icon_paths.push(p);
                    }
                }
            }

            let sql_files = format!(
                "SELECT text_content FROM clipboard_items
                 WHERE id IN ({placeholders}) AND kind = 'file' AND text_content IS NOT NULL",
            );
            let mut statement_files = connection.prepare(&sql_files)?;
            let file_rows = statement_files.query_map(rusqlite::params_from_iter(params.iter()), |row| {
                row.get::<_, String>(0)
            })?;
            for stored_paths in file_rows {
                if let Ok(stored_paths) = serde_json::from_str::<Vec<String>>(&stored_paths?) {
                    paths.resource_paths.extend(
                        stored_paths.into_iter().filter(|p| !p.trim().is_empty()),
                    );
                }
            }

            Ok(paths)
        })
    }

    /// Returns all unsynced changelog entries up to `limit` rows, ordered by sequence.
    pub fn get_unsynced_changelog(
        &self,
        limit: usize,
    ) -> Result<Vec<SyncChangeLogEntry>, StorageError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT sequence, item_id, operation, kind, title, content_hash,
                        resource_path, preview_path, icon_path, created_at_ms,
                        modified_at_ms, device_id,
                        text_content, html_content, rtf_content, metadata_json,
                        is_favorite, source_app, size_bytes, last_used_at_ms
                 FROM sync_changelog
                 WHERE synced = 0
                 ORDER BY sequence ASC
                 LIMIT ?1",
            )?;
            let rows = statement.query_map([limit as i64], |row| {
                Ok(SyncChangeLogEntry {
                    sequence: row.get(0)?,
                    item_id: row.get(1)?,
                    operation: row.get(2)?,
                    kind: row.get(3)?,
                    title: row.get(4)?,
                    content_hash: row.get(5)?,
                    resource_path: row.get(6)?,
                    preview_path: row.get(7)?,
                    icon_path: row.get(8)?,
                    created_at_ms: row.get(9)?,
                    modified_at_ms: row.get(10)?,
                    device_id: row.get(11)?,
                    text_content: row.get(12)?,
                    html_content: row.get(13)?,
                    rtf_content: row.get(14)?,
                    metadata_json: row.get(15)?,
                    is_favorite: row.get(16)?,
                    source_app: row.get(17)?,
                    size_bytes: row.get(18)?,
                    last_used_at_ms: row.get(19)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
        })
    }

    /// Marks changelog entries as synced (up to and including `max_sequence`).
    pub fn mark_changelog_synced(&self, max_sequence: i64) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let affected = connection.execute(
                "UPDATE sync_changelog SET synced = 1 WHERE sequence <= ?1 AND synced = 0",
                [max_sequence],
            )?;
            Ok(affected as u64)
        })
    }

    /// Returns the count of unsynced changelog entries.
    pub fn count_unsynced_changelog(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM sync_changelog WHERE synced = 0",
                [],
                |row| row.get(0),
            )?;
            Ok(count as u64)
        })
    }

    /// Sets the device identifier in sync_metadata.
    /// Called once at startup. All trigger-generated changelog entries
    /// will use this value.
    pub fn set_sync_device_id(&self, device_id: &str) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('device_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [device_id],
            )?;
            Ok(())
        })
    }

    /// Gets the device identifier from sync_metadata.
    pub fn get_sync_device_id(&self) -> Result<String, StorageError> {
        self.with_connection(|connection| {
            let id: String = connection.query_row(
                "SELECT value FROM sync_metadata WHERE key = 'device_id'",
                [],
                |row| row.get(0),
            )?;
            Ok(id)
        })
    }

    /// Highest `modified_ms` among remote oplog files already applied. Files
    /// whose mtime is strictly lower than this watermark are skipped on the next
    /// sync, so previously-applied oplogs are not downloaded/parsed again.
    pub fn get_sync_applied_oplog_watermark(&self) -> Result<Option<i64>, StorageError> {
        self.with_connection(|connection| {
            let value: Option<String> = connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_applied_oplog_watermark_ms'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(value.and_then(|v| v.parse::<i64>().ok()))
        })
    }

    /// Persists the applied-remote-oplog watermark.
    pub fn set_sync_applied_oplog_watermark(&self, modified_ms: i64) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('sync_applied_oplog_watermark_ms', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [modified_ms.to_string()],
            )?;
            Ok(())
        })
    }

    /// Number of oplog files observed on the remote during the last sync. Used
    /// by the UI to suggest a manual compaction before the retention cleanup
    /// starts discarding old oplogs.
    pub fn get_sync_remote_oplog_count(&self) -> Result<Option<u64>, StorageError> {
        self.with_connection(|connection| {
            let value: Option<String> = connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_remote_oplog_count'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(value.and_then(|v| v.parse::<u64>().ok()))
        })
    }

    /// Persists the remote oplog count observed during the last sync.
    pub fn set_sync_remote_oplog_count(&self, count: u64) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('sync_remote_oplog_count', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [count.to_string()],
            )?;
            Ok(())
        })
    }

    /// Latest modified time of the newest remote baseline file observed during
    /// the last sync, used to suggest compaction when the snapshot is old.
    pub fn get_sync_remote_baseline_modified_ms(&self) -> Result<Option<i64>, StorageError> {
        self.with_connection(|connection| {
            let value: Option<String> = connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_remote_baseline_modified_ms'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(value.and_then(|v| v.parse::<i64>().ok()))
        })
    }

    /// Persists the latest remote baseline mtime observed during the last sync.
    pub fn set_sync_remote_baseline_modified_ms(
        &self,
        modified_ms: i64,
    ) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('sync_remote_baseline_modified_ms', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [modified_ms.to_string()],
            )?;
            Ok(())
        })
    }

    /// Sets the changelog-suppression flag so the sync_changelog triggers stay
    /// quiet while remote data is being applied. The flag is written and cleared
    /// inside the caller's transaction, so a rollback reverts it too (no stale
    /// suppression after a failed apply).
    fn set_changelog_suppressed(
        tx: &rusqlite::Transaction<'_>,
        suppressed: bool,
    ) -> Result<(), StorageError> {
        tx.execute(
            "INSERT INTO sync_metadata (key, value) VALUES ('sync_suppress_changelog', ?1)
             ON CONFLICT(key) DO UPDATE SET value = ?1",
            [if suppressed { "1" } else { "0" }],
        )?;
        Ok(())
    }

    /// Exports all active (non-deleted) items for baseline creation.
    pub fn export_active_items(&self) -> Result<Vec<ClipboardItem>, StorageError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, kind, title, text_content, html_content, rtf_content,
                        resource_path, preview_path, content_hash, source_app,
                        icon_path, size_bytes, created_at_ms, last_used_at_ms,
                        is_favorite, metadata_json, deleted, deleted_at_ms, modified_at_ms
                 FROM clipboard_items
                 WHERE deleted = 0
                 ORDER BY created_at_ms ASC",
            )?;
            let rows = statement.query_map([], |row| {
                use crate::domain::ClipboardKind;
                let kind_str: String = row.get(1)?;
                let kind = match kind_str.as_str() {
                    "text" => ClipboardKind::Text,
                    "link" => ClipboardKind::Link,
                    "image" => ClipboardKind::Image,
                    "file" => ClipboardKind::File,
                    _ => ClipboardKind::Text,
                };
                Ok(ClipboardItem {
                    id: row.get(0)?,
                    kind,
                    title: row.get(2)?,
                    text_content: row.get(3)?,
                    html_content: row.get(4)?,
                    rtf_content: row.get(5)?,
                    resource_path: row.get(6)?,
                    preview_path: row.get(7)?,
                    content_hash: row.get(8)?,
                    source_app: row.get(9)?,
                    icon_path: row.get(10)?,
                    size_bytes: row.get::<_, i64>(11)? as u64,
                    created_at_ms: row.get(12)?,
                    last_used_at_ms: row.get(13)?,
                    is_favorite: row.get(14)?,
                    metadata_json: row.get(15)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
        })
    }

    /// Imports baseline items (upsert by id).
    /// Used when restoring from a baseline on a new device.
    pub fn import_baseline_items(&self, items: &[ClipboardItem]) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let tx = connection.transaction()?;
            Self::set_changelog_suppressed(&tx, true)?;
            let mut imported = 0u64;
            for item in items {
                let kind_str = match item.kind {
                    crate::domain::ClipboardKind::Text => "text",
                    crate::domain::ClipboardKind::Link => "link",
                    crate::domain::ClipboardKind::Image => "image",
                    crate::domain::ClipboardKind::File => "file",
                };
                tx.execute(
                    "INSERT INTO clipboard_items
                     (id, kind, title, text_content, html_content, rtf_content,
                      resource_path, preview_path, content_hash, source_app,
                      icon_path, size_bytes, created_at_ms, last_used_at_ms,
                      is_favorite, metadata_json, deleted, modified_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                             ?11, ?12, ?13, ?14, ?15, ?16, 0, ?17)
                     ON CONFLICT(id) DO UPDATE SET
                        kind = excluded.kind,
                        title = excluded.title,
                        text_content = excluded.text_content,
                        html_content = excluded.html_content,
                        rtf_content = excluded.rtf_content,
                        resource_path = excluded.resource_path,
                        preview_path = excluded.preview_path,
                        content_hash = excluded.content_hash,
                        source_app = excluded.source_app,
                        icon_path = excluded.icon_path,
                        size_bytes = excluded.size_bytes,
                        last_used_at_ms = excluded.last_used_at_ms,
                        is_favorite = excluded.is_favorite,
                        metadata_json = excluded.metadata_json,
                        modified_at_ms = excluded.modified_at_ms
                     WHERE excluded.modified_at_ms >= clipboard_items.modified_at_ms",
                    rusqlite::params![
                        &item.id,
                        kind_str,
                        &item.title,
                        &item.text_content,
                        &item.html_content,
                        &item.rtf_content,
                        &item.resource_path,
                        &item.preview_path,
                        &item.content_hash,
                        &item.source_app,
                        &item.icon_path,
                        &(item.size_bytes as i64),
                        &item.created_at_ms,
                        &item.last_used_at_ms,
                        &item.is_favorite,
                        &item.metadata_json,
                        &item.created_at_ms,
                    ],
                )?;
                imported += 1;
            }
            Self::set_changelog_suppressed(&tx, false)?;
            tx.commit()?;
            Ok(imported)
        })
    }

    /// Applies a batch of remote oplog entries to the local database.
    /// Uses last-modified-wins conflict resolution.
    /// Returns the number of entries successfully applied.
    pub fn apply_remote_oplog(&self, entries: &[SyncChangeLogEntry]) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let tx = connection.transaction()?;
            Self::set_changelog_suppressed(&tx, true)?;
            let mut applied = 0u64;

            for entry in entries {
                let existing_modified: Option<i64> = tx
                    .query_row(
                        "SELECT modified_at_ms FROM clipboard_items WHERE id = ?1",
                        [&entry.item_id],
                        |row| row.get(0),
                    )
                    .ok();

                match entry.operation.as_str() {
                    "insert" => {
                        let _ = tx.execute(
                            "INSERT OR IGNORE INTO clipboard_items
                             (id, kind, title, text_content, html_content, rtf_content,
                              content_hash, resource_path, preview_path, icon_path,
                              metadata_json, is_favorite, source_app,
                              size_bytes, last_used_at_ms,
                              created_at_ms, modified_at_ms, deleted)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                                     ?11, ?12, ?13, ?14, ?15, ?16, ?17, 0)",
                            rusqlite::params![
                                &entry.item_id,
                                &entry.kind,
                                &entry.title,
                                &entry.text_content,
                                &entry.html_content,
                                &entry.rtf_content,
                                &entry.content_hash,
                                &entry.resource_path,
                                &entry.preview_path,
                                &entry.icon_path,
                                &entry.metadata_json,
                                &entry.is_favorite,
                                &entry.source_app,
                                &entry.size_bytes,
                                &entry.last_used_at_ms,
                                &entry.created_at_ms,
                                &entry.modified_at_ms,
                            ],
                        );
                        applied += 1;
                    }
                    "update" => {
                        if let Some(local_modified) = existing_modified {
                            if entry.modified_at_ms >= local_modified {
                                let _ = tx.execute(
                                    "UPDATE clipboard_items
                                     SET kind = ?2, title = ?3,
                                         text_content = ?4, html_content = ?5,
                                         rtf_content = ?6, content_hash = ?7,
                                         resource_path = ?8, preview_path = ?9,
                                         icon_path = ?10, metadata_json = ?11,
                                         is_favorite = ?12, source_app = ?13,
                                         size_bytes = ?14, last_used_at_ms = ?15,
                                         modified_at_ms = ?16,
                                         deleted = 0
                                     WHERE id = ?1",
                                    rusqlite::params![
                                        &entry.item_id,
                                        &entry.kind,
                                        &entry.title,
                                        &entry.text_content,
                                        &entry.html_content,
                                        &entry.rtf_content,
                                        &entry.content_hash,
                                        &entry.resource_path,
                                        &entry.preview_path,
                                        &entry.icon_path,
                                        &entry.metadata_json,
                                        &entry.is_favorite,
                                        &entry.source_app,
                                        &entry.size_bytes,
                                        &entry.last_used_at_ms,
                                        &entry.modified_at_ms,
                                    ],
                                );
                                applied += 1;
                            }
                        } else {
                            let _ = tx.execute(
                                "INSERT OR IGNORE INTO clipboard_items
                                 (id, kind, title, text_content, html_content, rtf_content,
                                  content_hash, resource_path, preview_path, icon_path,
                                  metadata_json, is_favorite, source_app,
                                  size_bytes, last_used_at_ms,
                                  created_at_ms, modified_at_ms, deleted)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                                         ?11, ?12, ?13, ?14, ?15, ?16, ?17, 0)",
                                rusqlite::params![
                                    &entry.item_id,
                                    &entry.kind,
                                    &entry.title,
                                    &entry.text_content,
                                    &entry.html_content,
                                    &entry.rtf_content,
                                    &entry.content_hash,
                                    &entry.resource_path,
                                    &entry.preview_path,
                                    &entry.icon_path,
                                    &entry.metadata_json,
                                    &entry.is_favorite,
                                    &entry.source_app,
                                    &entry.size_bytes,
                                    &entry.last_used_at_ms,
                                    &entry.created_at_ms,
                                    &entry.modified_at_ms,
                                ],
                            );
                            applied += 1;
                        }
                    }
                    "delete" => {
                        let _ = tx.execute(
                            "UPDATE clipboard_items SET deleted = 1, deleted_at_ms = ?2
                             WHERE id = ?1 AND COALESCE(modified_at_ms, 0) <= ?3",
                            rusqlite::params![
                                &entry.item_id,
                                &entry.modified_at_ms,
                                &entry.modified_at_ms,
                            ],
                        );
                        applied += 1;
                    }
                    _ => {}
                }
            }

            Self::set_changelog_suppressed(&tx, false)?;
            tx.commit()?;
            Ok(applied)
        })
    }

    /// Purges old synced entries beyond the most recent N for housekeeping.
    pub fn purge_synced_changelog(&self, keep_recent: i64) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let affected = connection.execute(
                "DELETE FROM sync_changelog
                 WHERE synced = 1
                   AND sequence <= (SELECT COALESCE(MAX(sequence) - ?1, 0) FROM sync_changelog)",
                [keep_recent],
            )?;
            Ok(affected as u64)
        })
    }

    pub fn count_active_items(&self) -> Result<usize, StorageError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE deleted = 0",
                [],
                |row| row.get(0),
            )?;
            Ok(count as usize)
        })
    }

    pub fn list_storage_file_references(&self) -> Result<StorageFileReferences, StorageError> {
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
                    _ => {}
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

    pub fn repair(&self) -> Result<RepairResult, StorageError> {
        self.with_connection(|connection| {
            let integrity: String =
                connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;

            let is_ok = integrity == "ok";
            if !is_ok {
                let _ = connection.execute("PRAGMA quick_check", []);
            }

            let page_count: i64 =
                connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
            let freelist_count: i64 =
                connection.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;

            Ok(RepairResult {
                integrity_ok: is_ok,
                integrity_message: integrity,
                page_count,
                freelist_count,
            })
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairResult {
    pub integrity_ok: bool,
    pub integrity_message: String,
    pub page_count: i64,
    pub freelist_count: i64,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use crate::domain::ClipboardItem;
    use crate::domain::ClipboardKind;

    use super::Database;
    use crate::storage::ClipboardRepository;

    fn text_item(id: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: format!("record-{id}"),
            text_content: Some(format!("content-{id}")),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 12,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "clipboard-pool-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn set_preview_path_updates_database() {
        let db_path = temporary_path("preview-path");
        let database = Database::open(&db_path).unwrap();
        database.save_item(&text_item("item", 100)).unwrap();

        assert!(database
            .set_preview_path("item", "previews/thumb.jpg")
            .unwrap());
        let stored = database.get_item("item").unwrap().unwrap();
        assert_eq!(stored.preview_path.unwrap(), "previews/thumb.jpg");

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn repair_checks_integrity() {
        let db_path = temporary_path("repair");
        let database = Database::open(&db_path).unwrap();
        database.save_item(&text_item("item", 100)).unwrap();

        let result = database.repair().unwrap();
        assert!(result.integrity_ok);

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn unsynced_changelog_carries_text_content() {
        let db_path = temporary_path("oplog-content");
        let database = Database::open(&db_path).unwrap();
        database.set_sync_device_id("test-device").unwrap();
        database.save_item(&text_item("item", 100)).unwrap();

        let entries = database.get_unsynced_changelog(10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].operation, "insert");
        assert_eq!(entries[0].text_content.as_deref(), Some("content-item"));
        assert_eq!(entries[0].title, "record-item");
        assert!(!entries[0].is_favorite);

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn apply_remote_oplog_restores_text_content() {
        let db_path = temporary_path("oplog-apply");
        let source = Database::open(&db_path).unwrap();
        source.set_sync_device_id("test-device").unwrap();
        source.save_item(&text_item("item", 100)).unwrap();
        let entries = source.get_unsynced_changelog(10).unwrap();

        let target_path = temporary_path("oplog-apply-target");
        let target = Database::open(&target_path).unwrap();
        let applied = target.apply_remote_oplog(&entries).unwrap();
        assert_eq!(applied, 1);

        let stored = target.get_item("item").unwrap().unwrap();
        assert_eq!(stored.text_content.as_deref(), Some("content-item"));
        assert_eq!(stored.title, "record-item");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&target_path);
    }

    #[test]
    fn apply_remote_oplog_does_not_echo_back_to_changelog() {
        let db_path = temporary_path("oplog-echo-source");
        let source = Database::open(&db_path).unwrap();
        source.set_sync_device_id("remote-device").unwrap();
        source.save_item(&text_item("item", 100)).unwrap();
        let entries = source.get_unsynced_changelog(10).unwrap();

        let target_path = temporary_path("oplog-echo-target");
        let target = Database::open(&target_path).unwrap();
        target.set_sync_device_id("local-device").unwrap();
        let applied = target.apply_remote_oplog(&entries).unwrap();
        assert_eq!(applied, 1);

        // Received entries must not be re-queued as local unsynced changes.
        let echo = target.get_unsynced_changelog(10).unwrap();
        assert_eq!(
            echo.len(),
            0,
            "applied remote entries echoed into local changelog"
        );
        let stored = target.get_item("item").unwrap().unwrap();
        assert_eq!(stored.text_content.as_deref(), Some("content-item"));

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&target_path);
    }

    #[test]
    fn import_baseline_does_not_echo_back_to_changelog() {
        let db_path = temporary_path("baseline-echo");
        let source = Database::open(&db_path).unwrap();
        source.set_sync_device_id("remote-device").unwrap();
        source.save_item(&text_item("item", 100)).unwrap();
        let items = source.export_active_items().unwrap();

        let target_path = temporary_path("baseline-echo-target");
        let target = Database::open(&target_path).unwrap();
        target.set_sync_device_id("local-device").unwrap();
        let imported = target.import_baseline_items(&items).unwrap();
        assert_eq!(imported, 1);

        let echo = target.get_unsynced_changelog(10).unwrap();
        assert_eq!(
            echo.len(),
            0,
            "imported baseline entries echoed into local changelog"
        );

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&target_path);
    }

    #[test]
    fn local_saves_still_queued_after_remote_apply() {
        let db_path = temporary_path("post-apply-local");
        let source = Database::open(&db_path).unwrap();
        source.set_sync_device_id("remote-device").unwrap();
        source.save_item(&text_item("remote", 100)).unwrap();
        let entries = source.get_unsynced_changelog(10).unwrap();

        let target_path = temporary_path("post-apply-local-target");
        let target = Database::open(&target_path).unwrap();
        target.set_sync_device_id("local-device").unwrap();
        target.apply_remote_oplog(&entries).unwrap();
        target.save_item(&text_item("local", 200)).unwrap();

        // A genuine local mutation after apply must still be queued.
        let echo = target.get_unsynced_changelog(10).unwrap();
        assert_eq!(echo.len(), 1);
        assert_eq!(echo[0].item_id, "local");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&target_path);
    }

    #[test]
    fn import_baseline_does_not_overwrite_newer_local_entries() {
        // Same content hash on two devices yields the same deterministic id
        // (e.g. img_{hash}). The local device edited/favorited the item after
        // capture, so its modified_at_ms is newer than the baseline snapshot
        // (baseline stores modified_at_ms = created_at_ms). Importing the
        // baseline must not clobber the newer local edit.
        let source_path = temporary_path("baseline-lmw-source");
        let source = Database::open(&source_path).unwrap();
        source.set_sync_device_id("remote-device").unwrap();
        let mut item = text_item("img_abc", 100);
        item.title = "baseline-title".to_string(); // stale remote snapshot
        item.is_favorite = false;
        source.save_item(&item).unwrap();
        let items = source.export_active_items().unwrap();
        assert_eq!(items.len(), 1);

        let target_path = temporary_path("baseline-lmw-target");
        let target = Database::open(&target_path).unwrap();
        target.set_sync_device_id("local-device").unwrap();
        let local_item = text_item("img_abc", 100);
        target.save_item(&local_item).unwrap();

        // A genuine local edit after capture: favorited. The set_modified
        // trigger bumps modified_at_ms to now, far newer than baseline's
        // created_at_ms snapshot (100).
        target.set_favorite("img_abc", true).unwrap();
        assert!(target.get_item("img_abc").unwrap().unwrap().is_favorite);

        let imported = target.import_baseline_items(&items).unwrap();
        assert_eq!(imported, 1);

        let local = target.get_item("img_abc").unwrap().unwrap();
        assert!(
            local.is_favorite,
            "baseline must not overwrite a newer local edit"
        );
        assert_eq!(
            local.title, "record-img_abc",
            "local title survives stale baseline"
        );

        let _ = std::fs::remove_file(&source_path);
        let _ = std::fs::remove_file(&target_path);
    }

    #[test]
    fn sync_applied_oplog_watermark_round_trip() {
        let db_path = temporary_path("oplog-watermark");
        let db = Database::open(&db_path).unwrap();

        assert_eq!(db.get_sync_applied_oplog_watermark().unwrap(), None);

        db.set_sync_applied_oplog_watermark(123456).unwrap();
        assert_eq!(db.get_sync_applied_oplog_watermark().unwrap(), Some(123456));

        db.set_sync_applied_oplog_watermark(654321).unwrap();
        assert_eq!(db.get_sync_applied_oplog_watermark().unwrap(), Some(654321));

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn sync_remote_compaction_stats_round_trip() {
        let db_path = temporary_path("compaction-stats");
        let db = Database::open(&db_path).unwrap();

        assert_eq!(db.get_sync_remote_oplog_count().unwrap(), None);
        assert_eq!(db.get_sync_remote_baseline_modified_ms().unwrap(), None);

        db.set_sync_remote_oplog_count(11).unwrap();
        db.set_sync_remote_baseline_modified_ms(123456789).unwrap();
        assert_eq!(db.get_sync_remote_oplog_count().unwrap(), Some(11));
        assert_eq!(
            db.get_sync_remote_baseline_modified_ms().unwrap(),
            Some(123456789)
        );

        db.set_sync_remote_oplog_count(0).unwrap();
        assert_eq!(db.get_sync_remote_oplog_count().unwrap(), Some(0));

        let _ = std::fs::remove_file(&db_path);
    }
}
