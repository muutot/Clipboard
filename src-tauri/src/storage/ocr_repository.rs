use rusqlite::{params, OptionalExtension, Row};

use crate::domain::{OcrResult, OcrStatus, OcrTextBlock};

use super::{Database, StorageError};

const OCR_COLUMNS: &str = "
    item_id,
    status,
    engine,
    model_version,
    language,
    full_text,
    blocks_json,
    image_hash,
    created_at_ms,
    completed_at_ms,
    error_message
";

pub trait OcrRepository {
    fn save_ocr_result(&self, result: &OcrResult) -> Result<(), StorageError>;
    fn get_ocr_result(&self, item_id: &str) -> Result<Option<OcrResult>, StorageError>;
    fn find_completed_ocr_by_hash(
        &self,
        image_hash: &str,
    ) -> Result<Option<OcrResult>, StorageError>;
}

impl OcrRepository for Database {
    fn save_ocr_result(&self, result: &OcrResult) -> Result<(), StorageError> {
        let blocks_json = serde_json::to_string(&result.blocks)?;

        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO ocr_results (
                    item_id,
                    status,
                    engine,
                    model_version,
                    language,
                    full_text,
                    blocks_json,
                    image_hash,
                    created_at_ms,
                    completed_at_ms,
                    error_message
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(item_id) DO UPDATE SET
                    status = excluded.status,
                    engine = excluded.engine,
                    model_version = excluded.model_version,
                    language = excluded.language,
                    full_text = excluded.full_text,
                    blocks_json = excluded.blocks_json,
                    image_hash = excluded.image_hash,
                    completed_at_ms = excluded.completed_at_ms,
                    error_message = excluded.error_message",
                params![
                    result.item_id,
                    status_to_storage(result.status),
                    result.engine,
                    result.model_version,
                    result.language,
                    result.full_text,
                    blocks_json,
                    result.image_hash,
                    result.created_at_ms,
                    result.completed_at_ms,
                    result.error_message,
                ],
            )?;

            Ok(())
        })
    }

    fn get_ocr_result(&self, item_id: &str) -> Result<Option<OcrResult>, StorageError> {
        self.with_connection(|connection| {
            let sql = format!("SELECT {OCR_COLUMNS} FROM ocr_results WHERE item_id = ?1");
            let stored_result = connection
                .query_row(&sql, [item_id], StoredOcrResult::from_row)
                .optional()?;

            stored_result.map(TryInto::try_into).transpose()
        })
    }

    fn find_completed_ocr_by_hash(
        &self,
        image_hash: &str,
    ) -> Result<Option<OcrResult>, StorageError> {
        self.with_connection(|connection| {
            let sql = format!(
                "SELECT {OCR_COLUMNS}
                 FROM ocr_results
                 WHERE image_hash = ?1 AND status = 'completed'
                 ORDER BY completed_at_ms DESC
                 LIMIT 1"
            );
            let stored_result = connection
                .query_row(&sql, [image_hash], StoredOcrResult::from_row)
                .optional()?;

            stored_result.map(TryInto::try_into).transpose()
        })
    }
}

struct StoredOcrResult {
    item_id: String,
    status: String,
    engine: String,
    model_version: String,
    language: Option<String>,
    full_text: String,
    blocks_json: String,
    image_hash: String,
    created_at_ms: i64,
    completed_at_ms: Option<i64>,
    error_message: Option<String>,
}

impl StoredOcrResult {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            item_id: row.get(0)?,
            status: row.get(1)?,
            engine: row.get(2)?,
            model_version: row.get(3)?,
            language: row.get(4)?,
            full_text: row.get(5)?,
            blocks_json: row.get(6)?,
            image_hash: row.get(7)?,
            created_at_ms: row.get(8)?,
            completed_at_ms: row.get(9)?,
            error_message: row.get(10)?,
        })
    }
}

impl TryFrom<StoredOcrResult> for OcrResult {
    type Error = StorageError;

    fn try_from(result: StoredOcrResult) -> Result<Self, Self::Error> {
        Ok(Self {
            item_id: result.item_id,
            status: status_from_storage(&result.status)?,
            engine: result.engine,
            model_version: result.model_version,
            language: result.language,
            full_text: result.full_text,
            blocks: serde_json::from_str::<Vec<OcrTextBlock>>(&result.blocks_json)?,
            image_hash: result.image_hash,
            created_at_ms: result.created_at_ms,
            completed_at_ms: result.completed_at_ms,
            error_message: result.error_message,
        })
    }
}

fn status_to_storage(status: OcrStatus) -> &'static str {
    match status {
        OcrStatus::Pending => "pending",
        OcrStatus::Processing => "processing",
        OcrStatus::Completed => "completed",
        OcrStatus::Failed => "failed",
    }
}

fn status_from_storage(status: &str) -> Result<OcrStatus, StorageError> {
    match status {
        "pending" => Ok(OcrStatus::Pending),
        "processing" => Ok(OcrStatus::Processing),
        "completed" => Ok(OcrStatus::Completed),
        "failed" => Ok(OcrStatus::Failed),
        _ => Err(StorageError::InvalidOcrStatus(status.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus, OcrTextBlock};

    use super::{Database, OcrRepository};
    use crate::storage::ClipboardRepository;

    fn image_item(id: &str, image_hash: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Image,
            title: "screenshot.png".to_owned(),
            text_content: None,
            resource_path: Some("images/screenshot.png".to_owned()),
            preview_path: Some("previews/screenshot.webp".to_owned()),
            content_hash: image_hash.to_owned(),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 1024,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
        }
    }

    fn completed_result(item_id: &str, image_hash: &str) -> OcrResult {
        OcrResult {
            item_id: item_id.to_owned(),
            status: OcrStatus::Completed,
            engine: "test-engine".to_owned(),
            model_version: "1".to_owned(),
            language: Some("zh-CN".to_owned()),
            full_text: "脸皮挺脏".to_owned(),
            blocks: vec![OcrTextBlock {
                text: "脸皮挺脏".to_owned(),
                confidence: 0.98,
                left: 10,
                top: 20,
                width: 100,
                height: 24,
            }],
            image_hash: image_hash.to_owned(),
            created_at_ms: 100,
            completed_at_ms: Some(200),
            error_message: None,
        }
    }

    #[test]
    fn stores_blocks_and_reuses_completed_results_by_hash() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&image_item("image", "image-hash"))
            .unwrap();
        database
            .save_ocr_result(&completed_result("image", "image-hash"))
            .unwrap();

        let stored = database.get_ocr_result("image").unwrap().unwrap();
        let reused = database
            .find_completed_ocr_by_hash("image-hash")
            .unwrap()
            .unwrap();

        assert_eq!(stored.full_text, "脸皮挺脏");
        assert_eq!(stored.blocks.len(), 1);
        assert_eq!(reused.item_id, "image");
    }

    #[test]
    fn ocr_updates_are_enqueued_for_search_indexing() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&image_item("image", "image-hash"))
            .unwrap();
        database
            .save_ocr_result(&completed_result("image", "image-hash"))
            .unwrap();

        database
            .with_connection(|connection| {
                let operations: i64 = connection.query_row(
                    "SELECT COUNT(*)
                     FROM search_outbox
                     WHERE item_id = 'image' AND operation = 'upsert'",
                    [],
                    |row| row.get(0),
                )?;

                assert_eq!(operations, 2);
                Ok(())
            })
            .unwrap();
    }
}
