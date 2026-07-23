use std::{collections::HashSet, time::SystemTime};

use rusqlite::{params, Connection, TransactionBehavior};

use super::StorageError;

struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: r#"
        CREATE TABLE clipboard_items (
            id TEXT PRIMARY KEY NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'link', 'image', 'file')),
            title TEXT NOT NULL,
            text_content TEXT,
            resource_path TEXT,
            preview_path TEXT,
            content_hash TEXT NOT NULL,
            source_app TEXT,
            size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
            created_at_ms INTEGER NOT NULL,
            last_used_at_ms INTEGER,
            is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1)),
            metadata_json TEXT NOT NULL DEFAULT '{}',
            UNIQUE (kind, content_hash)
        );

        CREATE INDEX clipboard_items_created_at_idx
            ON clipboard_items (created_at_ms DESC);

        CREATE INDEX clipboard_items_favorite_created_at_idx
            ON clipboard_items (is_favorite, created_at_ms DESC);

        CREATE TABLE ocr_results (
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

        CREATE INDEX ocr_results_image_hash_idx
            ON ocr_results (image_hash);

        CREATE INDEX ocr_results_status_idx
            ON ocr_results (status, created_at_ms);

        CREATE TABLE search_outbox (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id TEXT NOT NULL,
            operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
            created_at_ms INTEGER NOT NULL
        );

        CREATE INDEX search_outbox_sequence_idx
            ON search_outbox (sequence);

        CREATE TRIGGER clipboard_items_search_insert
        AFTER INSERT ON clipboard_items
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.id, 'upsert', NEW.created_at_ms);
        END;

        CREATE TRIGGER clipboard_items_search_update
        AFTER UPDATE ON clipboard_items
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.id, 'upsert', NEW.created_at_ms);
        END;

        CREATE TRIGGER clipboard_items_search_delete
        AFTER DELETE ON clipboard_items
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (OLD.id, 'delete', OLD.created_at_ms);
        END;

        CREATE TRIGGER ocr_results_search_insert
        AFTER INSERT ON ocr_results
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.item_id, 'upsert', NEW.created_at_ms);
        END;

        CREATE TRIGGER ocr_results_search_update
        AFTER UPDATE ON ocr_results
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.item_id, 'upsert', NEW.created_at_ms);
        END;
    "#,
    },
    Migration {
        version: 2,
        sql: r#"
        ALTER TABLE clipboard_items ADD COLUMN deleted_at_ms INTEGER;
        ALTER TABLE clipboard_items ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1));

        CREATE INDEX clipboard_items_deleted_created_at_idx
            ON clipboard_items (deleted, created_at_ms DESC);

        DROP TRIGGER IF EXISTS clipboard_items_search_update;
        CREATE TRIGGER clipboard_items_search_update
        AFTER UPDATE ON clipboard_items
        BEGIN
            INSERT INTO search_outbox (item_id, operation, created_at_ms)
            VALUES (NEW.id, CASE WHEN NEW.deleted = 1 THEN 'delete' ELSE 'upsert' END, NEW.created_at_ms);
        END;
    "#,
    },
];

pub(super) fn run(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            applied_at_ms INTEGER NOT NULL
        );",
    )?;

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let applied_versions = {
        let mut statement = transaction.prepare("SELECT version FROM schema_migrations")?;
        let versions = statement
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<HashSet<_>, _>>()?;
        versions
    };

    for migration in MIGRATIONS {
        if applied_versions.contains(&migration.version) {
            continue;
        }

        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, applied_at_ms) VALUES (?1, ?2)",
            params![migration.version, current_time_ms()],
        )?;
    }

    transaction.commit()?;
    Ok(())
}

fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
