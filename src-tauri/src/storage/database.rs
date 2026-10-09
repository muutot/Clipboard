use std::{fs, path::Path, sync::Mutex, time::Duration};

use rusqlite::{Connection, OpenFlags};

use super::{schema, StorageError};

pub struct Database {
    pub(super) connection: Mutex<Connection>,
    schema_was_reset: bool,
    migration_writer: Mutex<Option<Connection>>,
}

/// Holds SQLite's writer reservation across resource copy and config save.
/// On success the owning Database retains it until restart; failure releases it.
pub(crate) struct StorageMigrationGuard<'a> {
    writer: std::sync::MutexGuard<'a, Option<Connection>>,
    keep_until_restart: bool,
}

impl StorageMigrationGuard<'_> {
    pub(crate) fn keep_until_restart(mut self) {
        self.keep_until_restart = true;
    }
}

impl Drop for StorageMigrationGuard<'_> {
    fn drop(&mut self) {
        if !self.keep_until_restart {
            // Closing this connection rolls back its empty write transaction.
            self.writer.take();
        }
    }
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
            migration_writer: Mutex::new(None),
        };
        database.ensure_sync_device_id()?;
        Ok(database)
    }

    /// Changes only when another SQLite connection commits. Compare on the same connection.
    pub(crate) fn data_version(&self) -> Result<i64, StorageError> {
        self.with_connection(|connection| {
            Ok(connection.pragma_query_value(None, "data_version", |row| row.get(0))?)
        })
    }

    pub fn schema_was_reset(&self) -> bool {
        self.schema_was_reset
    }

    pub(crate) fn begin_storage_migration(
        &self,
    ) -> Result<StorageMigrationGuard<'_>, StorageError> {
        let mut writer = match self.migration_writer.try_lock() {
            Ok(writer) => writer,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(StorageError::StorageMigrationInProgress)
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(StorageError::ConnectionPoisoned)
            }
        };
        if writer.is_some() {
            return Err(StorageError::StorageMigrationInProgress);
        }
        let path = self.with_connection(|connection| {
            connection
                .path()
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .ok_or(StorageError::StorageMigrationRequiresFile)
        })?;
        let reservation = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        reservation.busy_timeout(Duration::from_secs(5))?;
        // Wait for the current writer, then block every SQLite connection,
        // including CLI/API connections, from committing after the snapshot.
        reservation.execute_batch("BEGIN IMMEDIATE")?;
        *writer = Some(reservation);
        Ok(StorageMigrationGuard {
            writer,
            keep_until_restart: false,
        })
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

    /// Long tasks use their own WAL connection, without rerunning schema setup.
    /// In-memory test databases cannot be reopened, so they retain the same connection.
    pub(crate) fn with_task_connection<T>(
        &self,
        action: impl FnOnce(&mut Connection) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let path = self.with_connection(|connection| {
            Ok(connection
                .path()
                .filter(|path| !path.is_empty())
                .map(str::to_owned))
        })?;
        let Some(path) = path else {
            return self.with_connection(action);
        };
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA temp_store = FILE; PRAGMA cache_size = -2000;",
        )?;
        action(&mut connection)
    }

    pub fn vacuum_into(&self, target_path: impl AsRef<Path>) -> Result<(), StorageError> {
        self.with_connection(|conn| {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
            vacuum_into_path(conn, target_path.as_ref())
        })
    }

    /// Creates a transactionally consistent copy without forcing a WAL
    /// checkpoint. Sync uses this for bounded export; storage migration also
    /// uses it while a separate connection holds the writer reservation.
    pub(crate) fn snapshot_into(&self, target_path: impl AsRef<Path>) -> Result<(), StorageError> {
        self.with_connection(|conn| vacuum_into_path(conn, target_path.as_ref()))
    }
}

fn vacuum_into_path(connection: &Connection, target_path: &Path) -> Result<(), StorageError> {
    // SQLite string literals only escape `'` as `''`; backslashes are literal.
    // Doubling them would create a file whose name contains doubled separators.
    let target = target_path.to_string_lossy().replace('\'', "''");
    connection.execute_batch(&format!("VACUUM INTO '{target}'"))?;
    Ok(())
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
    fn migration_reservation_drains_an_existing_transaction_before_snapshot() {
        use std::sync::{mpsc, Arc};
        use std::time::Duration;
        let root =
            std::env::temp_dir().join(format!("clipboard-migration-drain-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("source.sqlite3");
        let snapshot = root.join("snapshot.sqlite3");
        let database = Arc::new(Database::open(&path).unwrap());
        let writer = rusqlite::Connection::open(&path).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE; INSERT INTO sync_metadata(key, value) VALUES ('audit-pending', 'accepted')").unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let worker_database = database.clone();
        let snapshot_path = snapshot.clone();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let guard = worker_database.begin_storage_migration().unwrap();
            acquired_tx.send(()).unwrap();
            worker_database.snapshot_into(snapshot_path).unwrap();
            drop(guard);
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(acquired_rx.recv_timeout(Duration::from_millis(80)).is_err());
        writer.execute_batch("COMMIT").unwrap();
        acquired_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        worker.join().unwrap();
        let saved = rusqlite::Connection::open(snapshot).unwrap();
        let value: String = saved
            .query_row(
                "SELECT value FROM sync_metadata WHERE key = 'audit-pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "accepted");
        drop(saved);
        drop(writer);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
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
