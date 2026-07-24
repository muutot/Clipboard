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

        Ok(Self {
            connection: Mutex::new(connection),
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
    use super::Database;

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
}
