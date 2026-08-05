mod database;
mod error;
mod migrations;
mod ocr_repository;
mod paths;
mod pool;
mod recovery;
mod repository;
mod search_repository;

pub use database::Database;
pub use error::StorageError;
pub use ocr_repository::OcrRepository;
pub use paths::{StoragePaths, RESOURCE_ROOT_MARKER};
pub use pool::{DatabasePool, PooledConnection, RepairResult};
pub use recovery::{
    backup_path, quarantine_search_index, recover_database_if_needed, refresh_database_backup,
    DatabaseRecoveryReport,
};
pub use repository::{
    ClipboardRepository, KindDeleteResult, KindDeleteScope, KindStorageStats,
    StorageFileReferences, TagInfo, TextItemUpdate,
};
pub use search_repository::{SearchDocument, SearchOperation, SearchOutboxEvent, SearchRepository};
