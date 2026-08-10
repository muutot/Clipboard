use std::{fs, path::Path, sync::Mutex, time::Duration};

use rusqlite::Connection;

use super::{migrations, StorageError};

pub struct Database {
    pub(super) connection: Mutex<Connection>,
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
        migrations::create_schema(&connection)?;

        let database = Self {
            connection: Mutex::new(connection),
        };
        database.ensure_sync_device_id()?;
        Ok(database)
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
    fn legacy_hostname_device_id_migrates_to_uuid_and_remains_an_alias() {
        let path = temporary_database_path("legacy-sync-id");
        let database = Database::open(&path).unwrap();
        database.set_sync_device_id("workstation-a").unwrap();
        drop(database);

        let migrated = Database::open(&path).unwrap();
        let ids = migrated.get_sync_device_ids().unwrap();
        assert_eq!(ids.len(), 2);
        assert!(Uuid::parse_str(&ids[0]).is_ok());
        assert_eq!(ids[1], "workstation-a");
        drop(migrated);
        remove_database_files(&path);
    }

    #[test]
    fn generic_fallback_device_ids_are_not_retained_as_aliases() {
        for fallback in ["unknown", "unknown-device", " UNKNOWN "] {
            let path = temporary_database_path("fallback-sync-id");
            let database = Database::open(&path).unwrap();
            database.set_sync_device_id(fallback).unwrap();
            drop(database);

            let migrated = Database::open(&path).unwrap();
            let ids = migrated.get_sync_device_ids().unwrap();
            assert_eq!(ids.len(), 1);
            assert!(Uuid::parse_str(&ids[0]).is_ok());
            drop(migrated);
            remove_database_files(&path);
        }
    }
}
