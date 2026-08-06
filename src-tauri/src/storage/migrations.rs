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

    ensure_item_tags_table(connection)?;

    // Databases created before HTML capture gained a column get it here.
    // `CREATE TABLE IF NOT EXISTS` never alters an existing table, so a
    // separate idempotent ALTER is required for upgrades.
    ensure_column(connection, "clipboard_items", "html_content", "TEXT")?;
    ensure_column(connection, "clipboard_items", "rtf_content", "TEXT")?;

    // `search_outbox.sequence` is an INTEGER PRIMARY KEY, which already
    // creates an index on the column, so the redundant explicit index adds
    // maintenance cost on every outbox insert without helping reads. Drop it
    // from databases that predate this cleanup; `IF EXISTS` keeps this safe on
    // every open and for fresh databases.
    connection.execute_batch("DROP INDEX IF EXISTS search_outbox_sequence_idx;")?;

    Ok(())
}

/// Tags are the only listable metadata, but storing them only inside
/// `metadata_json` forces `list_all_tags` / tag filtering to scan every row and
/// parse JSON. The `item_tags` junction table mirrors the tags stored in
/// `clipboard_items.metadata_json['tags']` for active items, so tag filtering
/// and counting can hit an index. `metadata_json` stays the source of truth;
/// every read/write path keeps the copy in sync.
///
/// The backfill is guarded by an emptiness check so upgrades only pay for the
/// O(n) JSON scan once, and `INSERT OR IGNORE` makes a concurrent run race-safe.
fn ensure_item_tags_table(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS item_tags (
            item_id TEXT NOT NULL,
            tag TEXT NOT NULL,
            PRIMARY KEY (item_id, tag),
            FOREIGN KEY (item_id) REFERENCES clipboard_items (id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS item_tags_tag_idx ON item_tags (tag, item_id);",
    )?;

    let rows: i64 = connection.query_row("SELECT COUNT(*) FROM item_tags", [], |row| row.get(0))?;
    if rows == 0 {
        connection.execute_batch(
            "INSERT OR IGNORE INTO item_tags (item_id, tag)
             SELECT clipboard_items.id, value
             FROM clipboard_items, json_each(clipboard_items.metadata_json, '$.tags')
             WHERE deleted = 0
               AND value IS NOT NULL
               AND TRIM(value) <> '';",
        )?;
    }
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
    fn existing_database_gains_rtf_content_column() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE clipboard_items (
                    id TEXT PRIMARY KEY NOT NULL,
                    kind TEXT NOT NULL,
                    title TEXT NOT NULL,
                    text_content TEXT,
                    html_content TEXT,
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
        assert!(columns.contains(&"rtf_content".to_owned()));

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

    #[test]
    fn backfills_item_tags_from_existing_metadata_tags() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE clipboard_items (
                    id TEXT PRIMARY KEY,
                    content_type TEXT NOT NULL,
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
                );
                INSERT INTO clipboard_items
                    (id, content_type, title, content_hash, size_bytes,
                     created_at_ms, deleted, metadata_json)
                VALUES
                    ('a', 'text', 'a', 'h-a', 1, 1, 0,
                        '{\"tags\":[\"work\",\"project\"]}'),
                    ('b', 'text', 'b', 'h-b', 1, 2, 0,
                        '{\"tags\":[\"work\"]}'),
                    ('c', 'text', 'c', 'h-c', 1, 3, 1,
                        '{\"tags\":[\"archived\"]}');",
            )
            .unwrap();

        create_schema(&connection).unwrap();

        let pairs: Vec<(String, String)> = connection
            .prepare("SELECT item_id, tag FROM item_tags ORDER BY item_id, tag")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            pairs,
            vec![
                ("a".to_owned(), "project".to_owned()),
                ("a".to_owned(), "work".to_owned()),
                ("b".to_owned(), "work".to_owned()),
            ]
        );

        // Re-running schema creation must be idempotent and not duplicate rows.
        create_schema(&connection).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM item_tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }
}
