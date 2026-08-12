use std::{fs, path::Path, sync::Mutex, time::Duration};

use rusqlite::Connection;

use super::{schema, StorageError};

pub struct Database {
    pub(super) connection: Mutex<Connection>,
    schema_was_reset: bool,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        Self::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, StorageError> {
        configure_connection(&connection)?;
        let schema = schema::initialize(&connection)?;

        let database = Self {
            connection: Mutex::new(connection),
            schema_was_reset: schema.was_reset,
        };
        database.ensure_sync_device_id()?;
        Ok(database)
    }

    pub fn schema_was_reset(&self) -> bool {
        self.schema_was_reset
    }

    pub(crate) fn with_connection<T>(
        &self,
        action: impl FnOnce(&mut Connection) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| StorageError::ConnectionPoisoned)?;

        action(&mut connection)
    }

    pub fn vacuum_into(&self, target_path: impl AsRef<Path>) -> Result<(), StorageError> {
        self.with_connection(|conn| {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
            let target = target_path.as_ref().to_string_lossy().replace('\\', "\\\\");
            conn.execute_batch(&format!("VACUUM INTO '{}'", target.replace('\'', "''")))?;
            Ok(())
        })
    }
}

fn configure_connection(connection: &Connection) -> Result<(), StorageError> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -20000;
         PRAGMA mmap_size = 268435456;",
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::SystemTime};

    use uuid::Uuid;

    use super::Database;

    fn temporary_database_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "clipboard-database-{label}-{}-{unique}.db",
            std::process::id()
        ))
    }

    fn remove_database_files(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn initializes_schema_and_enables_foreign_keys() {
        let database = Database::open_in_memory().unwrap();

        database
            .with_connection(|connection| {
                let foreign_keys: i64 =
                    connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
                let table_count: i64 = connection.query_row(
                    "SELECT COUNT(*)
                     FROM sqlite_master
                     WHERE type = 'table'
                       AND name IN (
                         'clipboard_items',
                         'ocr_results',
                         'search_outbox'
                       )",
                    [],
                    |row| row.get(0),
                )?;

                assert_eq!(foreign_keys, 1);
                assert_eq!(table_count, 3);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn generated_sync_device_uuid_persists_across_reopen() {
        let path = temporary_database_path("stable-sync-id");
        let first = Database::open(&path).unwrap();
        let first_id = first.get_sync_device_id().unwrap();
        assert!(Uuid::parse_str(&first_id).is_ok());
        drop(first);

        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.get_sync_device_id().unwrap(), first_id);
        drop(reopened);
        remove_database_files(&path);
    }

    #[test]
    fn non_uuid_sync_identity_is_replaced_without_an_alias() {
        let path = temporary_database_path("invalid-sync-id");
        let database = Database::open(&path).unwrap();
        database.set_sync_device_id("workstation-a").unwrap();
        drop(database);

        let reopened = Database::open(&path).unwrap();
        assert!(Uuid::parse_str(&reopened.get_sync_device_id().unwrap()).is_ok());
        let metadata_count: i64 = reopened
            .with_connection(|connection| {
                Ok(connection
                    .query_row("SELECT COUNT(*) FROM sync_metadata", [], |row| row.get(0))?)
            })
            .unwrap();
        assert_eq!(metadata_count, 1);
        drop(reopened);
        remove_database_files(&path);
    }

    #[test]
    fn every_invalid_device_id_is_replaced_by_a_uuid() {
        for fallback in ["unknown", "unknown-device", " UNKNOWN "] {
            let path = temporary_database_path("fallback-sync-id");
            let database = Database::open(&path).unwrap();
            database.set_sync_device_id(fallback).unwrap();
            drop(database);

            let reopened = Database::open(&path).unwrap();
            assert!(Uuid::parse_str(&reopened.get_sync_device_id().unwrap()).is_ok());
            drop(reopened);
            remove_database_files(&path);
        }
    }
}
