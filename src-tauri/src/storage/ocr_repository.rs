use std::path::PathBuf;

use rusqlite::{params, OptionalExtension, Row, TransactionBehavior};

use crate::domain::{OcrResult, OcrStatus, OcrTextBlock};
use crate::ocr::OcrInput;

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
    fn enqueue_ocr(&self, item_id: &str) -> Result<bool, StorageError>;
    fn claim_next_ocr(&self) -> Result<Option<OcrInput>, StorageError>;
    fn retry_ocr(&self, item_id: &str) -> Result<bool, StorageError>;
    fn requeue_interrupted_ocr(&self) -> Result<u64, StorageError>;
    fn save_ocr_result(&self, result: &OcrResult) -> Result<(), StorageError>;
    fn get_ocr_result(&self, item_id: &str) -> Result<Option<OcrResult>, StorageError>;
    fn find_completed_ocr_by_hash(
        &self,
        image_hash: &str,
    ) -> Result<Option<OcrResult>, StorageError>;
}

impl OcrRepository for Database {
    fn enqueue_ocr(&self, item_id: &str) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "INSERT INTO ocr_results (
                    item_id,
                    status,
                    image_hash,
                    created_at_ms
                 )
                 SELECT id, 'pending', content_hash, created_at_ms
                 FROM clipboard_items
                 WHERE id = ?1
                   AND kind = 'image'
                   AND resource_path IS NOT NULL
                 ON CONFLICT(item_id) DO NOTHING",
                [item_id],
            )? > 0)
        })
    }

    fn claim_next_ocr(&self) -> Result<Option<OcrInput>, StorageError> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let pending = transaction
                .query_row(
                    "SELECT ocr_results.item_id, clipboard_items.resource_path, ocr_results.image_hash
                     FROM ocr_results
                     INNER JOIN clipboard_items
                        ON clipboard_items.id = ocr_results.item_id
                     WHERE ocr_results.status = 'pending'
                       AND clipboard_items.resource_path IS NOT NULL
                     ORDER BY ocr_results.created_at_ms ASC
                     LIMIT 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;

            let Some((item_id, image_path, image_hash)) = pending else {
                transaction.commit()?;
                return Ok(None);
            };

            let claimed = transaction.execute(
                "UPDATE ocr_results
                 SET status = 'processing', error_message = NULL
                 WHERE item_id = ?1 AND status = 'pending'",
                [&item_id],
            )?;
            transaction.commit()?;

            if claimed == 0 {
                return Ok(None);
            }

            Ok(Some(OcrInput {
                item_id,
                image_path: PathBuf::from(image_path),
                image_hash,
            }))
        })
    }

    fn retry_ocr(&self, item_id: &str) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "UPDATE ocr_results
                 SET status = 'pending', error_message = NULL, completed_at_ms = NULL
                 WHERE item_id = ?1 AND status = 'failed'",
                [item_id],
            )? > 0)
        })
    }

    fn requeue_interrupted_ocr(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count = connection.execute(
                "UPDATE ocr_results
                 SET status = 'pending'
                 WHERE status = 'processing'",
                [],
            )?;

            Ok(count as u64)
        })
    }

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

    #[test]
    fn claims_queued_images_in_creation_order() {
        let database = Database::open_in_memory().unwrap();
        let mut later = image_item("later", "later-hash");
        later.created_at_ms = 200;
        let mut earlier = image_item("earlier", "earlier-hash");
        earlier.created_at_ms = 100;
        database.save_item(&later).unwrap();
        database.save_item(&earlier).unwrap();
        assert!(database.enqueue_ocr("later").unwrap());
        assert!(database.enqueue_ocr("earlier").unwrap());

        let claimed = database.claim_next_ocr().unwrap().unwrap();

        assert_eq!(claimed.item_id, "earlier");
        assert_eq!(
            database.get_ocr_result("earlier").unwrap().unwrap().status,
            OcrStatus::Processing
        );
    }

    #[test]
    fn requeues_jobs_interrupted_by_shutdown() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&image_item("image", "image-hash"))
            .unwrap();
        database.enqueue_ocr("image").unwrap();
        database.claim_next_ocr().unwrap().unwrap();

        assert_eq!(database.requeue_interrupted_ocr().unwrap(), 1);
        assert_eq!(
            database.get_ocr_result("image").unwrap().unwrap().status,
            OcrStatus::Pending
        );
    }
}
