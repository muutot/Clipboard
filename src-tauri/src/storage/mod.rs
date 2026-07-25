mod database;
mod error;
mod migrations;
mod ocr_repository;
mod paths;
mod pool;
mod repository;
mod search_repository;

pub use database::Database;
pub use error::StorageError;
pub use ocr_repository::OcrRepository;
pub use paths::StoragePaths;
pub use pool::{DatabasePool, PooledConnection, RepairResult};
pub use repository::{ClipboardRepository, StorageFileReferences};
pub use search_repository::{SearchDocument, SearchOperation, SearchOutboxEvent, SearchRepository};
