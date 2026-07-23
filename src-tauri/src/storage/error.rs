use std::{error::Error, fmt, path::PathBuf};

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Sqlite(rusqlite::Error),
    ConnectionPoisoned,
    FavoriteMustBeRemoved(String),
    DataDirectoryMustBeAbsolute(PathBuf),
    InvalidClipboardKind(String),
    InvalidOcrStatus(String),
    InvalidSearchOperation(String),
    InvalidStoredValue { field: &'static str, value: i64 },
    ValueOutOfRange { field: &'static str },
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON storage error: {error}"),
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::ConnectionPoisoned => formatter.write_str("database connection lock is poisoned"),
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
            Self::InvalidClipboardKind(kind) => {
                write!(formatter, "unknown clipboard item kind: {kind}")
            }
            Self::InvalidOcrStatus(status) => {
                write!(formatter, "unknown OCR status: {status}")
            }
            Self::InvalidSearchOperation(operation) => {
                write!(formatter, "unknown search outbox operation: {operation}")
            }
            Self::InvalidStoredValue { field, value } => {
                write!(formatter, "invalid stored value for {field}: {value}")
            }
            Self::ValueOutOfRange { field } => {
                write!(formatter, "value is out of range for {field}")
            }
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
