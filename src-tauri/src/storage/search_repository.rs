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
    fn acknowledge_search_outbox(&self, through_sequence: i64) -> Result<u64, StorageError>;
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
                        CASE
                            WHEN ocr_results.status = 'completed' THEN ocr_results.full_text
                            ELSE NULL
                        END,
                        clipboard_items.created_at_ms,
                        clipboard_items.is_favorite
                     FROM clipboard_items
                     LEFT JOIN ocr_results
                        ON ocr_results.item_id = clipboard_items.id
                     WHERE clipboard_items.id = ?1",
                    [item_id],
                    SearchDocument::from_row,
                )
                .optional()
                .map_err(Into::into)
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
}

impl SearchDocument {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let title = row.get::<_, String>(2)?;
        let text_content = row.get::<_, Option<String>>(3)?;
        let ocr_text = row.get::<_, Option<String>>(4)?;
        let content = [Some(title), text_content, ocr_text]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(Self {
            item_id: row.get(0)?,
            kind: row.get(1)?,
            content,
            created_at_ms: row.get(5)?,
            is_favorite: row.get(6)?,
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
            resource_path: (kind == ClipboardKind::Image)
                .then(|| "image/screenshot.png".to_owned()),
            preview_path: None,
            content_hash: content_hash.to_owned(),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 32,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
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
}
