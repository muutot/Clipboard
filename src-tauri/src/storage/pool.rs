use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OpenFlags};

use super::{Database, StorageError};

pub struct DatabasePool {
    write_connection: Arc<Database>,
    read_connections: Vec<Arc<Database>>,
    next_read: Mutex<usize>,
}

impl DatabasePool {
    pub fn open(path: impl AsRef<Path>, pool_size: usize) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let pool_size = pool_size.max(1);
        let write_connection = Arc::new(Database::open(path)?);
        let read_connections = (1..pool_size)
            .map(|_| Database::open_readonly(path).map(Arc::new))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            write_connection,
            read_connections,
            next_read: Mutex::new(0),
        })
    }

    pub fn acquire(&self) -> PooledConnection<'_> {
        let connection = if self.read_connections.is_empty() {
            Arc::clone(&self.write_connection)
        } else {
            let mut next = self
                .next_read
                .lock()
                .expect("database pool next-read lock is poisoned");
            let idx = *next;
            *next = (*next + 1) % self.read_connections.len();
            Arc::clone(&self.read_connections[idx])
        };

        PooledConnection {
            connection,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn acquire_write(&self) -> PooledConnection<'_> {
        PooledConnection {
            connection: Arc::clone(&self.write_connection),
            _marker: std::marker::PhantomData,
        }
    }
}

pub struct PooledConnection<'pool> {
    connection: Arc<Database>,
    _marker: std::marker::PhantomData<&'pool ()>,
}

impl std::ops::Deref for PooledConnection<'_> {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl Database {
    pub fn open_readonly(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn item_count_estimated(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT CAST(COALESCE(MAX(rowid), 0) AS BIGINT) FROM clipboard_items",
                [],
                |row| row.get(0),
            )?;
            Ok(count as u64)
        })
    }

    pub fn set_preview_path(
        &self,
        item_id: &str,
        preview_path: &str,
    ) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let affected = connection.execute(
                "UPDATE clipboard_items SET preview_path = ?2 WHERE id = ?1",
                rusqlite::params![item_id, preview_path],
            )?;
            Ok(affected > 0)
        })
    }

    pub fn repair(&self) -> Result<RepairResult, StorageError> {
        self.with_connection(|connection| {
            let integrity: String = connection
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;

            let is_ok = integrity == "ok";
            if !is_ok {
                let _ = connection.execute("PRAGMA quick_check", []);
            }

            let page_count: i64 = connection
                .query_row("PRAGMA page_count", [], |row| row.get(0))?;
            let freelist_count: i64 = connection
                .query_row("PRAGMA freelist_count", [], |row| row.get(0))?;

            Ok(RepairResult {
                integrity_ok: is_ok,
                integrity_message: integrity,
                page_count,
                freelist_count,
            })
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairResult {
    pub integrity_ok: bool,
    pub integrity_message: String,
    pub page_count: i64,
    pub freelist_count: i64,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use crate::domain::{ClipboardItem, ClipboardKind};

    use super::{Database, DatabasePool};
    use crate::storage::ClipboardRepository;

    fn text_item(id: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: format!("record-{id}"),
            text_content: Some(format!("content-{id}")),
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 12,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
        }
    }

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "clipboard-pool-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn creates_pool_with_configured_size() {
        let db_path = temporary_path("pool-size");
        let pool = DatabasePool::open(&db_path, 4).unwrap();

        let conn = pool.acquire_write();
        conn.save_item(&text_item("item", 100)).unwrap();
        assert_eq!(conn.item_count().unwrap(), 1);
        assert_eq!(conn.item_count_estimated().unwrap(), 1);

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn read_connections_can_query() {
        let db_path = temporary_path("read-query");
        let pool = DatabasePool::open(&db_path, 2).unwrap();
        pool.acquire_write()
            .save_item(&text_item("a", 100))
            .unwrap();
        pool.acquire_write()
            .save_item(&text_item("b", 200))
            .unwrap();

        let conn = pool.acquire();
        assert_eq!(conn.item_count().unwrap(), 2);

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn set_preview_path_updates_database() {
        let db_path = temporary_path("preview-path");
        let database = Database::open(&db_path).unwrap();
        database
            .save_item(&text_item("item", 100))
            .unwrap();

        assert!(database.set_preview_path("item", "previews/thumb.jpg").unwrap());
        let stored = database.get_item("item").unwrap().unwrap();
        assert_eq!(stored.preview_path.unwrap(), "previews/thumb.jpg");

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn repair_checks_integrity() {
        let db_path = temporary_path("repair");
        let database = Database::open(&db_path).unwrap();
        database.save_item(&text_item("item", 100)).unwrap();

        let result = database.repair().unwrap();
        assert!(result.integrity_ok);

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn item_count_estimated_is_fast() {
        let database = Database::open_in_memory().unwrap();
        for i in 0..10 {
            database
                .save_item(&text_item(&format!("item-{i}"), i * 100))
                .unwrap();
        }

        assert_eq!(database.item_count_estimated().unwrap(), 10);
    }
}
