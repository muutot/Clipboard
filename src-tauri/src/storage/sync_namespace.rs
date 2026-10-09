use super::{Database, StorageError};
use rusqlite::OptionalExtension;

impl Database {
    /// Credential-independent lookup for offline materialization of an upgraded
    /// namespace. This stores only the public descriptor, never a password/key.
    pub fn sync_namespace_binding(
        &self,
        stable_scope: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.with_connection(|connection| {
            Ok(connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = ?1",
                    [format!("sync_namespace:{stable_scope}")],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(String::into_bytes))
        })
    }
    pub fn remember_sync_namespace(
        &self,
        stable_scope: &str,
        descriptor: &[u8],
    ) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            connection.execute("INSERT INTO sync_metadata (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![format!("sync_namespace:{stable_scope}"), String::from_utf8_lossy(descriptor)])?;
            Ok(())
        })
    }
}
