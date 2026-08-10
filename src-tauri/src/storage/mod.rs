mod database;
mod error;
mod migrations;
mod ocr_repository;
mod paths;
mod pool;
mod recovery;
mod repository;
mod search_repository;
mod sync_v3;

pub use database::Database;
pub use error::StorageError;
pub use ocr_repository::OcrRepository;
pub use paths::{StoragePaths, RESOURCE_ROOT_MARKER};
pub use pool::{RepairResult, SyncChangeLogEntry};
pub use recovery::{
    backup_path, quarantine_search_index, recover_database_if_needed, refresh_database_backup,
    DatabaseRecoveryReport,
};
pub use repository::{
    ClipboardRepository, HistoryFilter, KindDeleteResult, KindDeleteScope, KindStorageStats,
    StorageFileReferences, TagInfo, TextItemUpdate,
};
pub use search_repository::{SearchDocument, SearchOperation, SearchOutboxEvent, SearchRepository};
pub use sync_v3::{SyncV3OutboxBatch, SyncV3RemoteState, SyncV3Snapshot};
