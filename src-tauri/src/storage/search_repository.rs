use rusqlite::{OptionalExtension, Row};

use super::{Database, StorageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOperation {
    Upsert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOutboxEvent {
    pub sequence: i64,
    pub item_id: String,
    pub operation: SearchOperation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    pub item_id: String,
    pub kind: String,
    pub content: String,
    pub created_at_ms: i64,
    pub is_favorite: bool,
}

pub trait SearchRepository {
    fn read_search_outbox(&self, limit: u32) -> Result<Vec<SearchOutboxEvent>, StorageError>;
    fn get_search_document(&self, item_id: &str) -> Result<Option<SearchDocument>, StorageError>;
    fn get_search_documents(
        &self,
        item_ids: &[impl AsRef<str>],
    ) -> Result<Vec<SearchDocument>, StorageError>;
    fn acknowledge_search_outbox(&self, through_sequence: i64) -> Result<u64, StorageError>;
    fn enqueue_full_search_rebuild(&self) -> Result<u64, StorageError>;
    /// Cheap probe used by the search command to skip the synchronizer loop
    /// entirely when no mutations are pending. Avoids the `LIMIT` scan and
    /// reader reload that `read_search_outbox` would trigger on every query.
    fn has_pending_outbox_events(&self) -> Result<bool, StorageError>;
}

impl SearchRepository for Database {
    fn read_search_outbox(&self, limit: u32) -> Result<Vec<SearchOutboxEvent>, StorageError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT sequence, item_id, operation
                 FROM search_outbox
                 ORDER BY sequence ASC
                 LIMIT ?1",
            )?;
            let events = statement
                .query_map([i64::from(limit.clamp(1, 1_000))], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            events
                .into_iter()
                .map(|(sequence, item_id, operation)| {
                    Ok(SearchOutboxEvent {
                        sequence,
                        item_id,
                        operation: operation_from_storage(&operation)?,
                    })
                })
                .collect()
        })
    }

    fn get_search_document(&self, item_id: &str) -> Result<Option<SearchDocument>, StorageError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT
                        clipboard_items.id,
                        clipboard_items.kind,
                        clipboard_items.title,
                        clipboard_items.text_content,
                        clipboard_items.source_app,
                        CASE
                            WHEN ocr_results.status = 'completed' THEN ocr_results.full_text
                            ELSE NULL
                        END,
                        clipboard_items.created_at_ms,
                        clipboard_items.is_favorite
                     FROM clipboard_items
                     LEFT JOIN ocr_results
                        ON ocr_results.item_id = clipboard_items.id
                     WHERE clipboard_items.id = ?1
                       AND clipboard_items.deleted = 0",
                    [item_id],
                    SearchDocument::from_row,
                )
                .optional()
                .map_err(Into::into)
        })
    }

    fn get_search_documents(
        &self,
        item_ids: &[impl AsRef<str>],
    ) -> Result<Vec<SearchDocument>, StorageError> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }

        self.with_connection(|connection| {
            let placeholders = item_ids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect::<Vec<_>>()
                .join(", ");

            let sql = format!(
                "SELECT
                    clipboard_items.id,
                    clipboard_items.kind,
                    clipboard_items.title,
                    clipboard_items.text_content,
                    clipboard_items.source_app,
                    CASE
                        WHEN ocr_results.status = 'completed' THEN ocr_results.full_text
                        ELSE NULL
                    END,
                    clipboard_items.created_at_ms,
                    clipboard_items.is_favorite
                 FROM clipboard_items
                 LEFT JOIN ocr_results
                    ON ocr_results.item_id = clipboard_items.id
                 WHERE clipboard_items.id IN ({placeholders})
                   AND clipboard_items.deleted = 0"
            );

            let mut statement = connection.prepare(&sql)?;
            let rows = statement
                .query_map(
                    rusqlite::params_from_iter(item_ids.iter().map(|id| id.as_ref())),
                    SearchDocument::from_row,
                )?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(rows)
        })
    }

    fn acknowledge_search_outbox(&self, through_sequence: i64) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let deleted = connection.execute(
                "DELETE FROM search_outbox WHERE sequence <= ?1",
                [through_sequence],
            )?;

            Ok(deleted as u64)
        })
    }

    fn enqueue_full_search_rebuild(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute("DELETE FROM search_outbox", [])?;
            let queued = transaction.execute(
                "INSERT INTO search_outbox (item_id, operation, created_at_ms)
                 SELECT id, 'upsert', created_at_ms
                 FROM clipboard_items
                 WHERE deleted = 0
                 ORDER BY id",
                [],
            )?;
            transaction.commit()?;

            Ok(queued as u64)
        })
    }

    fn has_pending_outbox_events(&self) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let exists: i64 = connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM search_outbox LIMIT 1)",
                [],
                |row| row.get(0),
            )?;
            Ok(exists != 0)
        })
    }
}

impl SearchDocument {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let title = row.get::<_, String>(2)?;
        let text_content = row.get::<_, Option<String>>(3)?;
        let source_app = row.get::<_, Option<String>>(4)?;
        let ocr_text = row.get::<_, Option<String>>(5)?;

        let parts: [&str; 4] = [
            title.as_str(),
            text_content.as_deref().unwrap_or(""),
            ocr_text.as_deref().unwrap_or(""),
            source_app.as_deref().unwrap_or(""),
        ];

        let mut content = String::new();
        for part in parts {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(trimmed);
            }
        }

        Ok(Self {
            item_id: row.get(0)?,
            kind: row.get(1)?,
            content,
            created_at_ms: row.get(6)?,
            is_favorite: row.get(7)?,
        })
    }
}

fn operation_from_storage(operation: &str) -> Result<SearchOperation, StorageError> {
    match operation {
        "upsert" => Ok(SearchOperation::Upsert),
        "delete" => Ok(SearchOperation::Delete),
        _ => Err(StorageError::InvalidSearchOperation(operation.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus};

    use super::{Database, SearchOperation, SearchRepository};
    use crate::storage::{ClipboardRepository, OcrRepository};

    fn item(id: &str, kind: ClipboardKind, content_hash: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind,
            title: format!("title-{id}"),
            text_content: (kind == ClipboardKind::Text).then(|| "脸皮挺脏".to_owned()),
            html_content: None,
            resource_path: (kind == ClipboardKind::Image)
                .then(|| "image/screenshot.png".to_owned()),
            preview_path: None,
            content_hash: content_hash.to_owned(),
            source_app: None,
            size_bytes: 32,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    #[test]
    fn reads_outbox_events_and_current_search_document() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("text", ClipboardKind::Text, "text-hash"))
            .unwrap();

        let events = database.read_search_outbox(100).unwrap();
        let document = database.get_search_document("text").unwrap().unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].item_id, "text");
        assert_eq!(events[0].operation, SearchOperation::Upsert);
        assert_eq!(document.kind, "text");
        assert_eq!(document.content, "title-text\n脸皮挺脏");
    }

    #[test]
    fn includes_only_completed_ocr_text_in_the_search_document() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("image", ClipboardKind::Image, "image-hash"))
            .unwrap();
        database
            .save_ocr_result(&OcrResult {
                item_id: "image".to_owned(),
                status: OcrStatus::Completed,
                engine: "test".to_owned(),
                model_version: "1".to_owned(),
                language: Some("zh-CN".to_owned()),
                full_text: "图片里的文字".to_owned(),
                blocks: vec![],
                image_hash: "image-hash".to_owned(),
                created_at_ms: 100,
                completed_at_ms: Some(200),
                error_message: None,
            })
            .unwrap();

        let document = database.get_search_document("image").unwrap().unwrap();

        assert_eq!(document.content, "title-image\n图片里的文字");
    }

    #[test]
    fn indexes_source_application_alongside_content() {
        let database = Database::open_in_memory().unwrap();
        let mut source_item = item("source", ClipboardKind::Text, "source-hash");
        source_item.source_app = Some("Zen Browser".to_owned());
        database.save_item(&source_item).unwrap();

        let document = database.get_search_document("source").unwrap().unwrap();
        assert!(document.content.contains("Zen Browser"));
    }

    #[test]
    fn soft_deleted_items_have_no_current_search_document() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("soft-deleted", ClipboardKind::Text, "soft-hash"))
            .unwrap();
        database.soft_delete("soft-deleted").unwrap();

        assert!(database
            .get_search_document("soft-deleted")
            .unwrap()
            .is_none());
    }

    #[test]
    fn acknowledges_only_events_through_the_committed_sequence() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("first", ClipboardKind::Text, "first-hash"))
            .unwrap();
        database
            .save_item(&item("second", ClipboardKind::Text, "second-hash"))
            .unwrap();
        let events = database.read_search_outbox(100).unwrap();

        assert_eq!(
            database
                .acknowledge_search_outbox(events[0].sequence)
                .unwrap(),
            1
        );
        let remaining = database.read_search_outbox(100).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].item_id, "second");
    }

    #[test]
    fn deleted_items_have_no_current_search_document() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("text", ClipboardKind::Text, "text-hash"))
            .unwrap();
        database.delete_item("text").unwrap();

        assert!(database.get_search_document("text").unwrap().is_none());
        assert_eq!(
            database
                .read_search_outbox(100)
                .unwrap()
                .last()
                .unwrap()
                .operation,
            SearchOperation::Delete
        );
    }

    #[test]
    fn full_rebuild_replaces_pending_events_with_current_items() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("keep", ClipboardKind::Text, "keep-hash"))
            .unwrap();
        database
            .save_item(&item("delete", ClipboardKind::Text, "delete-hash"))
            .unwrap();
        database.delete_item("delete").unwrap();

        assert_eq!(database.enqueue_full_search_rebuild().unwrap(), 1);
        let events = database.read_search_outbox(100).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].item_id, "keep");
        assert_eq!(events[0].operation, SearchOperation::Upsert);
    }

    #[test]
    fn full_rebuild_skips_soft_deleted_items() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("keep", ClipboardKind::Text, "keep-hash"))
            .unwrap();
        database
            .save_item(&item("soft-delete", ClipboardKind::Text, "soft-hash"))
            .unwrap();
        database.soft_delete("soft-delete").unwrap();

        assert_eq!(database.enqueue_full_search_rebuild().unwrap(), 1);
        let events = database.read_search_outbox(100).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].item_id, "keep");
    }
}
