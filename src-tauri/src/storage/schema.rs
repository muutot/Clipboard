use rusqlite::Connection;

use super::StorageError;

pub(super) const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SchemaInitResult {
    pub was_reset: bool,
}

/// Opens only the current schema. Databases without the exact v1 marker are
/// reset transactionally; no historical table, column, trigger, or row is
/// migrated into the current layout.
pub(super) fn initialize(connection: &Connection) -> Result<SchemaInitResult, StorageError> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        reset_to_current_schema(connection)?;
        return Ok(SchemaInitResult { was_reset: true });
    }

    create_current_schema(connection)?;
    Ok(SchemaInitResult { was_reset: false })
}

fn reset_to_current_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
    if let Err(error) = connection.execute_batch("BEGIN IMMEDIATE;") {
        let _ = connection.execute_batch("PRAGMA foreign_keys = ON;");
        return Err(error.into());
    }

    let result = (|| {
        drop_all_user_objects(connection)?;
        create_current_schema(connection)?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok::<(), StorageError>(())
    })();

    match result {
        Ok(()) => {
            connection.execute_batch("COMMIT; PRAGMA foreign_keys = ON;")?;
            Ok(())
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK; PRAGMA foreign_keys = ON;");
            Err(error)
        }
    }
}

fn drop_all_user_objects(connection: &Connection) -> Result<(), StorageError> {
    let objects = {
        let mut statement = connection.prepare(
            "SELECT type, name
             FROM sqlite_master
             WHERE type IN ('trigger', 'view', 'table')
               AND name NOT LIKE 'sqlite_%'
             ORDER BY CASE type
                 WHEN 'trigger' THEN 0
                 WHEN 'view' THEN 1
                 ELSE 2
             END",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    for (object_type, name) in objects {
        let keyword = match object_type.as_str() {
            "trigger" => "TRIGGER",
            "view" => "VIEW",
            "table" => "TABLE",
            _ => continue,
        };
        connection.execute_batch(&format!(
            "DROP {keyword} IF EXISTS {};",
            quote_identifier(&name)
        ))?;
    }
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn create_current_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS clipboard_items (
            id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'link', 'image', 'file')),
            title TEXT NOT NULL,
            text_content TEXT,
            html_content TEXT,
            rtf_content TEXT,
            resource_path TEXT,
            preview_path TEXT,
            content_hash TEXT NOT NULL,
            source_app TEXT,
            icon_path TEXT,
            size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
            created_at_ms INTEGER NOT NULL,
            last_used_at_ms INTEGER,
            is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1)),
            deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
            deleted_at_ms INTEGER,
            modified_at_ms INTEGER,
            sync_writer_device_id TEXT NOT NULL DEFAULT '',
            metadata_json TEXT DEFAULT '{}',
            UNIQUE (kind, content_hash)
        );

        CREATE INDEX IF NOT EXISTS clipboard_items_created_at_idx
            ON clipboard_items (created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS clipboard_items_favorite_created_at_idx
            ON clipboard_items (is_favorite, created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS clipboard_items_deleted_created_at_idx
            ON clipboard_items (deleted, created_at_ms DESC);

        CREATE TABLE IF NOT EXISTS ocr_results (
            item_id TEXT PRIMARY KEY NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('pending', 'processing', 'completed', 'failed')),
            engine TEXT NOT NULL DEFAULT '',
            model_version TEXT NOT NULL DEFAULT '',
            language TEXT,
            full_text TEXT NOT NULL DEFAULT '',
            blocks_json TEXT NOT NULL DEFAULT '[]',
            image_hash TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            completed_at_ms INTEGER,
            error_message TEXT,
            FOREIGN KEY (item_id) REFERENCES clipboard_items (id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS ocr_results_image_hash_idx
            ON ocr_results (image_hash);
        CREATE INDEX IF NOT EXISTS ocr_results_status_idx
            ON ocr_results (status, created_at_ms);

        CREATE TABLE IF NOT EXISTS search_outbox (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id TEXT NOT NULL,
            operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
            created_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tags (
            name TEXT PRIMARY KEY NOT NULL,
            color TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS item_tags (
            item_id TEXT NOT NULL,
            tag TEXT NOT NULL,
            PRIMARY KEY (item_id, tag),
            FOREIGN KEY (item_id) REFERENCES clipboard_items (id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS item_tags_tag_idx
            ON item_tags (tag, item_id);

        CREATE TABLE IF NOT EXISTS sync_metadata (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS sync_item_aliases (
            alias_id TEXT PRIMARY KEY NOT NULL,
            item_id TEXT NOT NULL,
            FOREIGN KEY (item_id) REFERENCES clipboard_items (id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS sync_item_aliases_item_idx
            ON sync_item_aliases (item_id);

        CREATE TABLE IF NOT EXISTS sync_outbox (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id TEXT NOT NULL,
            operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
            kind TEXT NOT NULL CHECK (kind IN ('text', 'link', 'image', 'file')),
            content_hash TEXT NOT NULL DEFAULT '',
            modified_at_ms INTEGER NOT NULL,
            writer_device_id TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS sync_outbox_item_sequence_idx
            ON sync_outbox (item_id, sequence DESC);

        CREATE TABLE IF NOT EXISTS sync_tombstones (
            item_id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'link', 'image', 'file')),
            content_hash TEXT NOT NULL DEFAULT '',
            deleted_at_ms INTEGER NOT NULL,
            modified_at_ms INTEGER NOT NULL,
            writer_device_id TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS sync_tombstones_version_idx
            ON sync_tombstones (modified_at_ms, writer_device_id);

        CREATE TABLE IF NOT EXISTS sync_publication_state (
            remote_scope TEXT PRIMARY KEY NOT NULL,
            epoch TEXT NOT NULL,
            snapshot_key TEXT,
            snapshot_sha256 TEXT,
            snapshot_size_bytes INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_size_bytes >= 0),
            snapshot_record_count INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_record_count >= 0),
            snapshot_sequence INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_sequence >= 0),
            published_sequence INTEGER NOT NULL DEFAULT 0 CHECK (published_sequence >= 0),
            last_segment_key TEXT,
            remote_prepared INTEGER NOT NULL DEFAULT 0 CHECK (remote_prepared IN (0, 1)),
            initialized INTEGER NOT NULL DEFAULT 0 CHECK (initialized IN (0, 1)),
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS sync_cursors (
            remote_scope TEXT NOT NULL,
            device_id TEXT NOT NULL,
            epoch TEXT NOT NULL,
            sequence INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
            snapshot_sha256 TEXT,
            last_segment_key TEXT,
            updated_at_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (remote_scope, device_id)
        );

        CREATE TABLE IF NOT EXISTS sync_remote_resources (
            remote_scope TEXT NOT NULL,
            object_key TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            size_bytes INTEGER NOT NULL DEFAULT 0 CHECK (size_bytes >= 0),
            confirmed_at_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (remote_scope, object_key)
        );

        CREATE TRIGGER IF NOT EXISTS clipboard_items_search_insert
        AFTER INSERT ON clipboard_items
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.id, 'upsert', NEW.created_at_ms);
        END;

        CREATE TRIGGER IF NOT EXISTS clipboard_items_search_update
        AFTER UPDATE ON clipboard_items
        WHEN OLD.kind != NEW.kind
          OR OLD.title != NEW.title
          OR OLD.text_content IS NOT NEW.text_content
          OR OLD.html_content IS NOT NEW.html_content
          OR OLD.rtf_content IS NOT NEW.rtf_content
          OR OLD.resource_path IS NOT NEW.resource_path
          OR OLD.preview_path IS NOT NEW.preview_path
          OR OLD.icon_path IS NOT NEW.icon_path
          OR OLD.source_app IS NOT NEW.source_app
          OR OLD.is_favorite != NEW.is_favorite
          OR OLD.metadata_json IS NOT NEW.metadata_json
          OR OLD.created_at_ms != NEW.created_at_ms
          OR OLD.deleted != NEW.deleted
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (
                NEW.id,
                CASE WHEN NEW.deleted = 1 THEN 'delete' ELSE 'upsert' END,
                NEW.created_at_ms
            );
        END;

        CREATE TRIGGER IF NOT EXISTS clipboard_items_search_delete
        AFTER DELETE ON clipboard_items
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (OLD.id, 'delete', OLD.created_at_ms);
        END;

        CREATE TRIGGER IF NOT EXISTS ocr_results_search_insert
        AFTER INSERT ON ocr_results
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.item_id, 'upsert', NEW.created_at_ms);
        END;

        CREATE TRIGGER IF NOT EXISTS ocr_results_search_update
        AFTER UPDATE ON ocr_results
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.item_id, 'upsert', NEW.created_at_ms);
        END;

        CREATE TRIGGER IF NOT EXISTS clipboard_items_sync_outbox_insert
        AFTER INSERT ON clipboard_items
        WHEN EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_enabled' AND value = '1'
        )
        AND NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_suppress_changelog' AND value = '1'
        )
        BEGIN
            UPDATE clipboard_items
               SET modified_at_ms = MAX(
                       CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
                       COALESCE(NEW.modified_at_ms, NEW.created_at_ms, 0),
                       COALESCE((
                           SELECT modified_at_ms + 1
                           FROM sync_tombstones
                           WHERE item_id = NEW.id
                       ), 0)
                   ),
                   sync_writer_device_id = COALESCE((
                       SELECT value FROM sync_metadata WHERE key = 'device_id'
                   ), '')
             WHERE id = NEW.id;

            INSERT INTO sync_outbox
                (item_id, operation, kind, content_hash, modified_at_ms, writer_device_id)
            SELECT id, 'upsert', kind, content_hash,
                   COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
              FROM clipboard_items
             WHERE id = NEW.id;

            DELETE FROM sync_tombstones WHERE item_id = NEW.id;
        END;

        CREATE TRIGGER IF NOT EXISTS clipboard_items_sync_outbox_update
        AFTER UPDATE ON clipboard_items
        WHEN EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_enabled' AND value = '1'
        )
        AND NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_suppress_changelog' AND value = '1'
        )
        AND (
               OLD.kind != NEW.kind
            OR OLD.title != NEW.title
            OR OLD.text_content IS NOT NEW.text_content
            OR OLD.html_content IS NOT NEW.html_content
            OR OLD.rtf_content IS NOT NEW.rtf_content
            OR OLD.resource_path IS NOT NEW.resource_path
            OR OLD.preview_path IS NOT NEW.preview_path
            OR OLD.content_hash != NEW.content_hash
            OR OLD.source_app IS NOT NEW.source_app
            OR OLD.icon_path IS NOT NEW.icon_path
            OR OLD.size_bytes != NEW.size_bytes
            OR OLD.created_at_ms != NEW.created_at_ms
            OR OLD.last_used_at_ms IS NOT NEW.last_used_at_ms
            OR OLD.is_favorite != NEW.is_favorite
            OR OLD.deleted != NEW.deleted
            OR OLD.deleted_at_ms IS NOT NEW.deleted_at_ms
            OR OLD.metadata_json IS NOT NEW.metadata_json
        )
        BEGIN
            UPDATE clipboard_items
               SET modified_at_ms = MAX(
                       CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
                       COALESCE(OLD.modified_at_ms, OLD.created_at_ms, 0) + 1,
                       COALESCE(NEW.modified_at_ms, NEW.created_at_ms, 0)
                   ),
                   sync_writer_device_id = COALESCE((
                       SELECT value FROM sync_metadata WHERE key = 'device_id'
                   ), '')
             WHERE id = NEW.id;

            INSERT INTO sync_outbox
                (item_id, operation, kind, content_hash, modified_at_ms, writer_device_id)
            SELECT id, CASE WHEN deleted = 1 THEN 'delete' ELSE 'upsert' END,
                   kind, content_hash, COALESCE(modified_at_ms, created_at_ms),
                   sync_writer_device_id
              FROM clipboard_items
             WHERE id = NEW.id;

            INSERT INTO sync_tombstones
                (item_id, kind, content_hash, deleted_at_ms, modified_at_ms, writer_device_id)
            SELECT id, kind, content_hash,
                   COALESCE(deleted_at_ms, modified_at_ms, created_at_ms),
                   COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
              FROM clipboard_items
             WHERE id = NEW.id AND deleted = 1
            ON CONFLICT(item_id) DO UPDATE SET
                kind = excluded.kind,
                content_hash = excluded.content_hash,
                deleted_at_ms = excluded.deleted_at_ms,
                modified_at_ms = excluded.modified_at_ms,
                writer_device_id = excluded.writer_device_id
            WHERE excluded.modified_at_ms > sync_tombstones.modified_at_ms
               OR (excluded.modified_at_ms = sync_tombstones.modified_at_ms
                   AND excluded.writer_device_id > sync_tombstones.writer_device_id);

            DELETE FROM sync_tombstones
             WHERE item_id = NEW.id AND NEW.deleted = 0;
        END;

        CREATE TRIGGER IF NOT EXISTS clipboard_items_sync_outbox_delete
        AFTER DELETE ON clipboard_items
        WHEN EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_enabled' AND value = '1'
        )
        AND NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_suppress_changelog' AND value = '1'
        )
        BEGIN
            INSERT INTO sync_tombstones
                (item_id, kind, content_hash, deleted_at_ms, modified_at_ms, writer_device_id)
            VALUES (
                OLD.id,
                OLD.kind,
                OLD.content_hash,
                CASE WHEN OLD.deleted = 1
                     THEN COALESCE(OLD.deleted_at_ms, OLD.modified_at_ms, OLD.created_at_ms)
                     ELSE MAX(
                         CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
                         COALESCE(OLD.modified_at_ms, OLD.created_at_ms, 0) + 1
                     )
                END,
                CASE WHEN OLD.deleted = 1
                     THEN COALESCE(OLD.modified_at_ms, OLD.created_at_ms)
                     ELSE MAX(
                         CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
                         COALESCE(OLD.modified_at_ms, OLD.created_at_ms, 0) + 1
                     )
                END,
                CASE WHEN OLD.deleted = 1 AND OLD.sync_writer_device_id != ''
                     THEN OLD.sync_writer_device_id
                     ELSE COALESCE((
                         SELECT value FROM sync_metadata WHERE key = 'device_id'
                     ), '')
                END
            )
            ON CONFLICT(item_id) DO UPDATE SET
                kind = excluded.kind,
                content_hash = excluded.content_hash,
                deleted_at_ms = excluded.deleted_at_ms,
                modified_at_ms = excluded.modified_at_ms,
                writer_device_id = excluded.writer_device_id
            WHERE excluded.modified_at_ms > sync_tombstones.modified_at_ms
               OR (excluded.modified_at_ms = sync_tombstones.modified_at_ms
                   AND excluded.writer_device_id > sync_tombstones.writer_device_id);

            INSERT INTO sync_outbox
                (item_id, operation, kind, content_hash, modified_at_ms, writer_device_id)
            SELECT item_id, 'delete', kind, content_hash, modified_at_ms, writer_device_id
              FROM sync_tombstones
             WHERE item_id = OLD.id;
        END;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_exists(connection: &Connection, name: &str) -> bool {
        connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
                )",
                [name],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn fresh_database_starts_at_schema_one() {
        let connection = Connection::open_in_memory().unwrap();

        assert!(initialize(&connection).unwrap().was_reset);
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for table in [
            "clipboard_items",
            "ocr_results",
            "search_outbox",
            "item_tags",
            "sync_outbox",
            "sync_tombstones",
            "sync_publication_state",
            "sync_cursors",
            "sync_remote_resources",
        ] {
            assert!(table_exists(&connection, table), "missing table {table}");
        }
    }

    #[test]
    fn non_v1_database_is_reset_instead_of_migrated() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE clipboard_items (id TEXT PRIMARY KEY, title TEXT);
                 INSERT INTO clipboard_items VALUES ('legacy', 'legacy');
                 CREATE TABLE sync_changelog (sequence INTEGER PRIMARY KEY);
                 CREATE TABLE legacy_only (value TEXT);",
            )
            .unwrap();

        assert!(initialize(&connection).unwrap().was_reset);
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM clipboard_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(!table_exists(&connection, "sync_changelog"));
        assert!(!table_exists(&connection, "legacy_only"));
    }

    #[test]
    fn reopening_schema_one_preserves_current_rows() {
        let connection = Connection::open_in_memory().unwrap();
        initialize(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO clipboard_items
                    (id, kind, title, content_hash, size_bytes, created_at_ms)
                 VALUES ('current', 'text', 'current', 'hash-current', 7, 1)",
                [],
            )
            .unwrap();

        assert!(!initialize(&connection).unwrap().was_reset);
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM clipboard_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn current_schema_has_no_legacy_sync_tables_or_triggers() {
        let connection = Connection::open_in_memory().unwrap();
        initialize(&connection).unwrap();

        for table in ["sync_changelog", "sync_remote_state", "sync_applied_oplogs"] {
            assert!(!table_exists(&connection, table));
        }
        let legacy_trigger_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'trigger'
                   AND name IN (
                     'clipboard_items_sync_insert',
                     'clipboard_items_sync_update',
                     'clipboard_items_sync_delete',
                     'clipboard_items_set_modified'
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_trigger_count, 0);
    }
}
