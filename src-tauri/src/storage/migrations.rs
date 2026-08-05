use rusqlite::Connection;

use super::StorageError;

pub(super) fn create_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS clipboard_items (
            id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'link', 'image', 'file')),
            title TEXT NOT NULL,
            text_content TEXT,
            html_content TEXT,
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

        CREATE TRIGGER IF NOT EXISTS clipboard_items_search_insert
        AFTER INSERT ON clipboard_items
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.id, 'upsert', NEW.created_at_ms);
        END;

        CREATE TRIGGER IF NOT EXISTS clipboard_items_search_update
        AFTER UPDATE ON clipboard_items
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.id, CASE WHEN NEW.deleted = 1 THEN 'delete' ELSE 'upsert' END, NEW.created_at_ms);
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

        -- Tag registry for the settings tag manager. Item membership is stored
        -- as strings under `metadata_json.tags`; this table only carries the
        -- global presentation metadata (color) keyed by tag name.
        CREATE TABLE IF NOT EXISTS tags (
            name TEXT PRIMARY KEY NOT NULL,
            color TEXT NOT NULL DEFAULT ''
        );",
    )?;

    // Databases created before HTML capture gained a column get it here.
    // `CREATE TABLE IF NOT EXISTS` never alters an existing table, so a
    // separate idempotent ALTER is required for upgrades.
    ensure_column(connection, "clipboard_items", "html_content", "TEXT")?;

    // `search_outbox.sequence` is an INTEGER PRIMARY KEY, which already
    // creates an index on the column, so the redundant explicit index adds
    // maintenance cost on every outbox insert without helping reads. Drop it
    // from databases that predate this cleanup; `IF EXISTS` keeps this safe on
    // every open and for fresh databases.
    connection.execute_batch(
        "DROP INDEX IF EXISTS search_outbox_sequence_idx;",
    )?;

    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    let exists = names
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column);
    if !exists {
        connection.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_database_gains_html_content_column() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE clipboard_items (
                    id TEXT PRIMARY KEY NOT NULL,
                    kind TEXT NOT NULL,
                    title TEXT NOT NULL,
                    text_content TEXT,
                    resource_path TEXT,
                    preview_path TEXT,
                    content_hash TEXT NOT NULL,
                    source_app TEXT,
                    icon_path TEXT,
                    size_bytes INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    last_used_at_ms INTEGER,
                    is_favorite INTEGER NOT NULL DEFAULT 0,
                    deleted INTEGER NOT NULL DEFAULT 0,
                    deleted_at_ms INTEGER,
                    metadata_json TEXT DEFAULT '{}'
                );",
            )
            .unwrap();

        create_schema(&connection).unwrap();

        let columns: Vec<String> = connection
            .prepare("PRAGMA table_info(clipboard_items)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(columns.contains(&"html_content".to_owned()));

        create_schema(&connection).unwrap();
    }

    #[test]
    fn drops_the_redundant_outbox_sequence_index_on_open() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS search_outbox (
                    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                    item_id TEXT NOT NULL,
                    operation TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS search_outbox_sequence_idx
                    ON search_outbox (sequence);",
            )
            .unwrap();

        create_schema(&connection).unwrap();

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*)
                 FROM sqlite_master
                 WHERE type = 'index'
                   AND name = 'search_outbox_sequence_idx'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);

        create_schema(&connection).unwrap();
    }
}
