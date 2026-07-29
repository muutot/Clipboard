use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Row;

use crate::domain::{OcrResult, OcrStatus, OcrTextBlock};
use crate::storage::StorageError;

pub(super) struct StoredOcrResult {
    pub(super) item_id: String,
    pub(super) status: String,
    pub(super) engine: String,
    pub(super) model_version: String,
    pub(super) language: Option<String>,
    pub(super) full_text: String,
    pub(super) blocks_json: String,
    pub(super) image_hash: String,
    pub(super) created_at_ms: i64,
    pub(super) completed_at_ms: Option<i64>,
    pub(super) error_message: Option<String>,
}

impl StoredOcrResult {
    pub(super) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
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

pub(super) fn status_to_storage(status: OcrStatus) -> &'static str {
    match status {
        OcrStatus::Pending => "pending",
        OcrStatus::Processing => "processing",
        OcrStatus::Completed => "completed",
        OcrStatus::Failed => "failed",
    }
}

pub(super) fn status_from_storage(status: &str) -> Result<OcrStatus, StorageError> {
    match status {
        "pending" => Ok(OcrStatus::Pending),
        "processing" => Ok(OcrStatus::Processing),
        "completed" => Ok(OcrStatus::Completed),
        "failed" => Ok(OcrStatus::Failed),
        _ => Err(StorageError::InvalidOcrStatus(status.to_owned())),
    }
}

pub(super) fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
