use std::path::PathBuf;

use rusqlite::{params, OptionalExtension, TransactionBehavior};

use crate::domain::OcrResult;
use crate::ocr::OcrInput;
use crate::storage::{Database, StorageError};

use super::helpers::*;
use super::traits::OcrRepository;

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

    fn mark_ocr_failed(&self, item_id: &str, error_message: &str) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "UPDATE ocr_results
                 SET status = 'failed',
                     error_message = ?2,
                     completed_at_ms = NULL
                 WHERE item_id = ?1 AND status = 'processing'",
                params![item_id, error_message],
            )? > 0)
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

    fn regenerate_ocr(&self, item_id: &str) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let image_hash = transaction
                .query_row(
                    "SELECT content_hash
                     FROM clipboard_items
                     WHERE id = ?1 AND kind = 'image' AND resource_path IS NOT NULL",
                    [item_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;

            let Some(image_hash) = image_hash else {
                transaction.commit()?;
                return Ok(false);
            };

            let processing_item = transaction
                .query_row(
                    "SELECT item_id
                     FROM ocr_results
                     WHERE image_hash = ?1 AND status = 'processing'
                     LIMIT 1",
                    [&image_hash],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(processing_item) = processing_item {
                return Err(StorageError::OcrRegenerationInProgress(processing_item));
            }

            let now = current_time_ms();
            transaction.execute(
                "UPDATE ocr_results
                 SET status = 'pending',
                     engine = '',
                     model_version = '',
                     language = NULL,
                     full_text = '',
                     blocks_json = '[]',
                     completed_at_ms = NULL,
                     error_message = NULL,
                     created_at_ms = ?2
                 WHERE image_hash = ?1",
                params![image_hash, now.saturating_add(1)],
            )?;
            transaction.execute(
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
                 )
                 SELECT id, 'pending', '', '', NULL, '', '[]', content_hash, ?2, NULL, NULL
                 FROM clipboard_items
                 WHERE id = ?1 AND kind = 'image' AND resource_path IS NOT NULL
                 ON CONFLICT(item_id) DO UPDATE SET
                    status = 'pending',
                    engine = '',
                    model_version = '',
                    language = NULL,
                    full_text = '',
                    blocks_json = '[]',
                    image_hash = excluded.image_hash,
                    created_at_ms = excluded.created_at_ms,
                    completed_at_ms = NULL,
                    error_message = NULL",
                params![item_id, now],
            )?;
            transaction.commit()?;
            Ok(true)
        })
    }

    fn requeue_interrupted_ocr(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count = connection.execute(
                "UPDATE ocr_results
                 SET status = 'pending',
                     error_message = NULL,
                     completed_at_ms = NULL
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

    fn count_pending_ocr(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM ocr_results WHERE status IN ('pending', 'processing')",
                [],
                |row| row.get(0),
            )?;
            Ok(count as u64)
        })
    }

    fn count_completed_ocr(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM ocr_results WHERE status = 'completed'",
                [],
                |row| row.get(0),
            )?;
            Ok(count as u64)
        })
    }

    fn count_failed_ocr(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM ocr_results WHERE status = 'failed'",
                [],
                |row| row.get(0),
            )?;
            Ok(count as u64)
        })
    }
}
