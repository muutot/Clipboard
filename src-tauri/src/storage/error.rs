use std::{error::Error, fmt, path::PathBuf};

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Sqlite(rusqlite::Error),
    ConnectionPoisoned,
    DatabaseBackupFailed {
        database: PathBuf,
        reason: String,
    },
    DatabaseRecoveryFailed {
        database: PathBuf,
        reason: String,
    },
    DatabaseRecoveryUnavailable {
        database: PathBuf,
        reason: String,
    },
    FavoriteMustBeRemoved(String),
    DataDirectoryMustBeAbsolute(PathBuf),
    ResourceDirectoryMustBeAbsolute {
        field: &'static str,
        path: PathBuf,
    },
    ResourceDirectoriesMustBeDistinct,
    ResourceDirectoriesOverlap {
        first: PathBuf,
        second: PathBuf,
    },
    ResourceDirectoryReserved {
        field: &'static str,
        path: PathBuf,
        reserved: PathBuf,
    },
    ResourceDirectoryMustBeEmptyOrOwned {
        field: &'static str,
        path: PathBuf,
    },
    InvalidClipboardKind(String),
    InvalidOcrStatus(String),
    OcrRegenerationInProgress(String),
    InvalidSearchOperation(String),
    InvalidSyncOperation(String),
    InvalidSyncState(String),
    InvalidKeyboardAction(String),
    InvalidShortcut(String),
    ShortcutConflict {
        shortcut: String,
        first_action: String,
        second_action: String,
    },
    InvalidStoredValue {
        field: &'static str,
        value: i64,
    },
    ValueOutOfRange {
        field: &'static str,
    },
    KindDeleteStatsChanged {
        expected_count: u64,
        expected_size: u64,
        actual_count: u64,
        actual_size: u64,
    },
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON storage error: {error}"),
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::ConnectionPoisoned => formatter.write_str("database connection lock is poisoned"),
            Self::DatabaseBackupFailed { database, reason } => write!(
                formatter,
                "database backup failed for {}: {reason}",
                database.display()
            ),
            Self::DatabaseRecoveryFailed { database, reason } => write!(
                formatter,
                "database recovery failed for {}: {reason}",
                database.display()
            ),
            Self::DatabaseRecoveryUnavailable { database, reason } => write!(
                formatter,
                "database recovery unavailable for {}: {reason}",
                database.display()
            ),
            Self::FavoriteMustBeRemoved(id) => {
                write!(
                    formatter,
                    "favorite item must be unfavorited before deletion: {id}"
                )
            }
            Self::DataDirectoryMustBeAbsolute(path) => {
                write!(
                    formatter,
                    "data directory must be an absolute path: {}",
                    path.display()
                )
            }
            Self::ResourceDirectoryMustBeAbsolute { field, path } => {
                write!(
                    formatter,
                    "{field} must be an absolute path: {}",
                    path.display()
                )
            }
            Self::ResourceDirectoriesMustBeDistinct => {
                formatter.write_str("file and image storage directories must be different")
            }
            Self::ResourceDirectoriesOverlap { first, second } => write!(
                formatter,
                "file and image storage directories must not overlap: {} and {}",
                first.display(),
                second.display()
            ),
            Self::ResourceDirectoryReserved {
                field,
                path,
                reserved,
            } => write!(
                formatter,
                "{field} cannot overlap the application directory {}: {}",
                reserved.display(),
                path.display()
            ),
            Self::ResourceDirectoryMustBeEmptyOrOwned { field, path } => write!(
                formatter,
                "{field} must be an empty directory or an application-owned resource directory: {}",
                path.display()
            ),
            Self::InvalidClipboardKind(kind) => {
                write!(formatter, "unknown clipboard item kind: {kind}")
            }
            Self::InvalidOcrStatus(status) => {
                write!(formatter, "unknown OCR status: {status}")
            }
            Self::OcrRegenerationInProgress(item_id) => {
                write!(
                    formatter,
                    "OCR regeneration is already running for {item_id}"
                )
            }
            Self::InvalidSearchOperation(operation) => {
                write!(formatter, "unknown search outbox operation: {operation}")
            }
            Self::InvalidSyncOperation(operation) => {
                write!(formatter, "unknown sync operation: {operation}")
            }
            Self::InvalidSyncState(message) => {
                write!(formatter, "invalid sync state: {message}")
            }
            Self::InvalidKeyboardAction(action) => {
                write!(formatter, "invalid keyboard action name: {action}")
            }
            Self::InvalidShortcut(message) => formatter.write_str(message),
            Self::ShortcutConflict {
                shortcut,
                first_action,
                second_action,
            } => write!(
                formatter,
                "shortcut {shortcut} is assigned to both {first_action} and {second_action}"
            ),
            Self::InvalidStoredValue { field, value } => {
                write!(formatter, "invalid stored value for {field}: {value}")
            }
            Self::ValueOutOfRange { field } => {
                write!(formatter, "value is out of range for {field}")
            }
            Self::KindDeleteStatsChanged {
                expected_count,
                expected_size,
                actual_count,
                actual_size,
            } => write!(
                formatter,
                "storage data changed before deletion (confirmed {expected_count} items/{expected_size} bytes, current {actual_count} items/{actual_size} bytes)"
            ),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
