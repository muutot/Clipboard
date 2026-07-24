use rusqlite::Connection;

use super::StorageError;

pub(super) fn create_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS clipboard_items (
            id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'link', 'image', 'file')),
            title TEXT NOT NULL,
            text_content TEXT,
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

        CREATE INDEX IF NOT EXISTS search_outbox_sequence_idx
            ON search_outbox (sequence);

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
        END;",
    )?;

    migrate_metadata_not_null(connection)?;

    Ok(())
}

fn migrate_metadata_not_null(connection: &Connection) -> Result<(), StorageError> {
    let has_not_null: bool = connection
        .query_row(
            "SELECT sql LIKE '%metadata_json TEXT NOT NULL%'
             FROM sqlite_master
             WHERE type = 'table' AND name = 'clipboard_items'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !has_not_null {
        return Ok(());
    }

    eprintln!("[migrations] migrating clipboard_items: removing NOT NULL from metadata_json");

    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS clipboard_items_new (
            id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'link', 'image', 'file')),
            title TEXT NOT NULL,
            text_content TEXT,
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

        INSERT INTO clipboard_items_new
            SELECT id, kind, title, text_content, resource_path, preview_path,
                   content_hash, source_app, icon_path, size_bytes, created_at_ms,
                   last_used_at_ms, is_favorite, deleted, deleted_at_ms, metadata_json
            FROM clipboard_items;

        DROP TABLE IF EXISTS clipboard_items;
        ALTER TABLE clipboard_items_new RENAME TO clipboard_items;

        CREATE INDEX IF NOT EXISTS clipboard_items_created_at_idx
            ON clipboard_items (created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS clipboard_items_favorite_created_at_idx
            ON clipboard_items (is_favorite, created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS clipboard_items_deleted_created_at_idx
            ON clipboard_items (deleted, created_at_ms DESC);

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
        END;",
    )?;

    eprintln!("[migrations] migration complete");

    Ok(())
}
