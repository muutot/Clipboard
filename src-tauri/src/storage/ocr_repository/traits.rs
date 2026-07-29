use crate::domain::OcrResult;
use crate::ocr::OcrInput;
use crate::storage::StorageError;

pub trait OcrRepository {
    fn enqueue_ocr(&self, item_id: &str) -> Result<bool, StorageError>;
    fn claim_next_ocr(&self) -> Result<Option<OcrInput>, StorageError>;
    fn mark_ocr_failed(&self, item_id: &str, error_message: &str) -> Result<bool, StorageError>;
    fn retry_ocr(&self, item_id: &str) -> Result<bool, StorageError>;
    fn regenerate_ocr(&self, item_id: &str) -> Result<bool, StorageError>;
    fn requeue_interrupted_ocr(&self) -> Result<u64, StorageError>;
    fn save_ocr_result(&self, result: &OcrResult) -> Result<(), StorageError>;
    fn get_ocr_result(&self, item_id: &str) -> Result<Option<OcrResult>, StorageError>;
    fn find_completed_ocr_by_hash(
        &self,
        image_hash: &str,
    ) -> Result<Option<OcrResult>, StorageError>;
    fn count_pending_ocr(&self) -> Result<u64, StorageError>;
    fn count_completed_ocr(&self) -> Result<u64, StorageError>;
    fn count_failed_ocr(&self) -> Result<u64, StorageError>;
}
