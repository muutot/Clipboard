mod database;
mod error;
mod maintenance;
mod migrations;
mod ocr_repository;
mod paths;
mod recovery;
mod repository;
mod schema;
mod search_repository;
mod sync_state;

pub use database::Database;
pub use error::StorageError;
pub use maintenance::RepairResult;
pub use ocr_repository::OcrRepository;
pub use paths::{StoragePaths, RESOURCE_ROOT_MARKER};
pub use recovery::{
    backup_path, discard_database_backups, discard_database_quarantine, quarantine_search_index,
    recover_database_if_needed, refresh_database_backup, reset_search_index,
    DatabaseRecoveryReport,
};
pub use repository::{
    ClipboardRepository, HistoryFilter, KindDeleteResult, KindDeleteScope, KindStorageStats,
    StorageFileReferences, TagInfo, TextItemUpdate,
};
pub use search_repository::{SearchDocument, SearchOperation, SearchOutboxEvent, SearchRepository};
pub use sync_state::{SyncOutboxBatch, SyncRemoteState, SyncSnapshot};
