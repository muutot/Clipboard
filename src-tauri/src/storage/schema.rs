use rusqlite::Connection;

use super::{StorageError, migrations};

pub(super) const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SchemaInitResult {
    pub was_reset: bool,
}

/// Schema v1 is the clean baseline. Pre-v1 databases are reset once for this
/// redesign; later versions are migrated sequentially and are never reset.
pub(super) fn initialize(connection: &Connection) -> Result<SchemaInitResult, StorageError> {
    initialize_after_version_read(connection, || {})
}

fn initialize_after_version_read(
    connection: &Connection,
    after_version_read: impl FnOnce(),
) -> Result<SchemaInitResult, StorageError> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    validate_schema_version(version)?;
    migrations::validate_registered_plan(1, SCHEMA_VERSION)?;
    after_version_read();

    // Current-schema opens must remain possible while storage migration holds
    // a writer reservation. Only reset/upgrade needs a new writer transaction.
    if version == SCHEMA_VERSION {
        create_current_schema(connection)?;
        return Ok(SchemaInitResult { was_reset: false });
    }

    migrations::with_schema_transaction(connection, |connection| {
        // Another opener may have initialized/upgraded since the first read.
        // Decide destructive reset and the migration chain under the same writer
        // reservation as their effects, never from the pre-lock version.
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        validate_schema_version(version)?;
        if version == 0 {
            reset_to_current_schema(connection)?;
            return Ok(SchemaInitResult { was_reset: true });
        }
        if version < SCHEMA_VERSION {
            migrations::migrate_to_current(
                connection,
                version,
                SCHEMA_VERSION,
                create_current_schema,
            )?;
        } else {
            create_current_schema(connection)?;
        }
        Ok(SchemaInitResult { was_reset: false })
    })
}

fn validate_schema_version(version: i64) -> Result<(), StorageError> {
    if version < 0 {
        return Err(StorageError::InvalidStoredValue {
            field: "PRAGMA user_version",
            value: version,
        });
    }
    if version > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedDatabaseSchemaVersion {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }

    Ok(())
}

fn reset_to_current_schema(connection: &Connection) -> Result<(), StorageError> {
    drop_all_user_objects(connection)?;
    create_current_schema(connection)?;
    connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
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

        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
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
        CREATE INDEX IF NOT EXISTS clipboard_items_deleted_last_used_idx
            ON clipboard_items (deleted, last_used_at_ms DESC, id DESC);
        CREATE INDEX IF NOT EXISTS clipboard_items_favorite_last_used_idx
            ON clipboard_items (deleted, is_favorite, last_used_at_ms DESC, id DESC);

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

        CREATE TABLE IF NOT EXISTS sync_checkpoint_state (
            remote_scope TEXT PRIMARY KEY NOT NULL,
            generation INTEGER NOT NULL CHECK (generation > 0),
            checkpoint_sha256 TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS sync_checkpoint_cursors (
            remote_scope TEXT NOT NULL,
            device_id TEXT NOT NULL,
            epoch TEXT NOT NULL,
            sequence INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
            last_segment_key TEXT,
            PRIMARY KEY (remote_scope, device_id),
            FOREIGN KEY (remote_scope) REFERENCES sync_checkpoint_state(remote_scope)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS sync_resource_scopes (
            id INTEGER PRIMARY KEY,
            remote_scope TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS sync_item_resources (
            scope_id INTEGER NOT NULL,
            item_id TEXT NOT NULL,
            slot INTEGER NOT NULL CHECK (slot IN (0, 1, 2)),
            ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal >= 0),
            sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
            extension TEXT NOT NULL CHECK (length(extension) BETWEEN 1 AND 16),
            PRIMARY KEY (scope_id, item_id, slot, ordinal),
            FOREIGN KEY (scope_id) REFERENCES sync_resource_scopes(id) ON DELETE CASCADE,
            FOREIGN KEY (item_id) REFERENCES clipboard_items(id) ON DELETE CASCADE
        ) WITHOUT ROWID;

        CREATE TRIGGER IF NOT EXISTS clipboard_items_sync_resources_update
        AFTER UPDATE OF resource_path, icon_path, text_content, deleted ON clipboard_items
        WHEN NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_suppress_changelog' AND value = '1'
        )
        AND (
               OLD.resource_path IS NOT NEW.resource_path
            OR OLD.icon_path IS NOT NEW.icon_path
            OR OLD.text_content IS NOT NEW.text_content
            OR OLD.deleted != NEW.deleted
        )
        BEGIN
            DELETE FROM sync_item_resources WHERE item_id = NEW.id;
        END;

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

-- The two sync outbox triggers below derive the replication version from
-- `modified_at_ms` with a `+ 1` step. SQLite promotes an overflowing INTEGER
-- sum to REAL, and a REAL version is unreadable as `i64` by
-- `sync_state::winning_local_version`, so one poisoned row would abort every
-- subsequent sync apply. Saturate at i64::MAX instead: the value stops being
-- able to advance, which is bad for that row, but it can never change type and
-- take the rest of the sync engine down with it.
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
                           SELECT MIN(9223372036854775807, modified_at_ms + 1)
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
            OR OLD.content_hash != NEW.content_hash
            OR OLD.source_app IS NOT NEW.source_app
            OR OLD.icon_path IS NOT NEW.icon_path
            OR OLD.size_bytes != NEW.size_bytes
            OR OLD.created_at_ms != NEW.created_at_ms
            OR OLD.is_favorite != NEW.is_favorite
            OR OLD.deleted != NEW.deleted
            OR OLD.deleted_at_ms IS NOT NEW.deleted_at_ms
            OR OLD.metadata_json IS NOT NEW.metadata_json
        )
        BEGIN
            UPDATE clipboard_items
               SET modified_at_ms = MAX(
                       CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
                       MIN(9223372036854775807, COALESCE(OLD.modified_at_ms, OLD.created_at_ms, 0) + 1),
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
        AND OLD.deleted = 0
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
    fn stale_pre_v1_initialization_preserves_another_openers_committed_rows() {
        let root = std::env::temp_dir().join(format!(
            "clipboard-schema-concurrent-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("history.sqlite3");
        let delayed = Connection::open(&path).unwrap();
        delayed
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        let outcome = initialize_after_version_read(&delayed, || {
            let winner = crate::storage::Database::open(&path).unwrap();
            assert!(winner.schema_was_reset());
            winner.with_connection(|connection| {
                connection.execute_batch(
                    "INSERT INTO clipboard_items (id, kind, title, content_hash, size_bytes, created_at_ms)
                     VALUES ('committed-after-initialization', 'text', 'keep', 'keep', 4, 1);
                     INSERT INTO item_tags (item_id, tag) VALUES ('committed-after-initialization', 'keep-tag');
                     INSERT INTO ocr_results (item_id, status, image_hash, created_at_ms)
                     VALUES ('committed-after-initialization', 'completed', 'keep-hash', 1);"
                )?;
                Ok(())
            }).unwrap();
        }).unwrap();
        let counts: (i64, i64, i64) = delayed
            .query_row(
                "SELECT (SELECT COUNT(*) FROM clipboard_items), (SELECT COUNT(*) FROM item_tags),
                    (SELECT COUNT(*) FROM ocr_results)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        drop(delayed);
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(
            counts,
            (1, 1, 1),
            "late initialization must not repeat the completed baseline reset"
        );
        assert!(!outcome.was_reset);
    }

    #[test]
    fn version_advanced_before_reset_is_rejected_without_touching_future_data() {
        let root =
            std::env::temp_dir().join(format!("clipboard-schema-future-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("history.sqlite3");
        let delayed = Connection::open(&path).unwrap();
        delayed
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        let result = initialize_after_version_read(&delayed, || {
            let newer = Connection::open(&path).unwrap();
            newer
                .execute_batch(
                    "CREATE TABLE future_data (value TEXT NOT NULL);
                INSERT INTO future_data VALUES ('keep'); PRAGMA user_version=3;",
                )
                .unwrap();
        });
        let version: i64 = delayed
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let future_value = delayed
            .query_row("SELECT value FROM future_data", [], |row| {
                row.get::<_, String>(0)
            })
            .ok();
        let foreign_keys: i64 = delayed
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        drop(delayed);
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            matches!(
                result,
                Err(StorageError::UnsupportedDatabaseSchemaVersion { found: 3, .. })
            ),
            "newer schema accepted by a stale initializer: {result:?}"
        );
        assert_eq!(version, 3);
        assert_eq!(future_value.as_deref(), Some("keep"));
        assert_eq!(foreign_keys, 1);
    }

    #[test]
    fn stale_v1_initialization_does_not_repeat_an_already_completed_backfill() {
        let root =
            std::env::temp_dir().join(format!("clipboard-schema-upgrade-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("history.sqlite3");
        let delayed = Connection::open(&path).unwrap();
        delayed
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        initialize(&delayed).unwrap();
        delayed.pragma_update(None, "user_version", 1).unwrap();
        let outcome = initialize_after_version_read(&delayed, || {
            let winner = Connection::open(&path).unwrap();
            initialize(&winner).unwrap();
            // A schema-valid NULL created after v2 commits is a sentinel:
            // opening v2 must not replay v1's data transformation on new rows.
            winner
                .execute_batch(
                    "INSERT INTO clipboard_items
                (id, kind, title, content_hash, size_bytes, created_at_ms, last_used_at_ms)
                VALUES ('after-upgrade', 'text', 'keep', 'keep', 4, 100, NULL);",
                )
                .unwrap();
        })
        .unwrap();
        let usage: Option<i64> = delayed
            .query_row(
                "SELECT last_used_at_ms FROM clipboard_items
            WHERE id='after-upgrade'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(delayed);
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(usage, None);
        assert!(!outcome.was_reset);
    }

    #[test]
    fn failed_baseline_creation_rolls_back_and_allows_retry() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON;
            CREATE TABLE legacy_data (value TEXT NOT NULL);
            INSERT INTO legacy_data VALUES ('keep'); PRAGMA max_page_count=2;",
            )
            .unwrap();
        let result = initialize(&connection);
        assert!(
            result.is_err(),
            "bounded SQLite pages must fail schema creation"
        );
        assert!(connection.is_autocommit());
        assert_eq!(
            connection
                .query_row("SELECT value FROM legacy_data", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "keep"
        );
        assert!(!table_exists(&connection, "clipboard_items"));
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        connection
            .pragma_update(None, "max_page_count", 100_000)
            .unwrap();
        assert!(initialize(&connection).unwrap().was_reset);
    }

    #[test]
    fn current_schema_can_open_during_an_independent_writer_reservation() {
        let root =
            std::env::temp_dir().join(format!("clipboard-schema-read-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("history.sqlite3");
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        initialize(&writer).unwrap();
        // A real previously initialized Database already has its device ID.
        // Seed it before the reservation so the reader needs no identity write.
        drop(crate::storage::Database::open(&path).unwrap());
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        let reader = crate::storage::Database::open(&path);
        writer.execute_batch("ROLLBACK").unwrap();
        assert!(
            reader.is_ok(),
            "current-schema reads must not need the reset/upgrade reservation"
        );
        drop(reader);
        drop(writer);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fresh_database_starts_at_current_schema() {
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
            "sync_resource_scopes",
            "sync_item_resources",
        ] {
            assert!(table_exists(&connection, table), "missing table {table}");
        }
    }

    #[test]
    fn pre_v1_database_is_reset_instead_of_migrated() {
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
    fn newer_schema_is_rejected_without_modifying_data() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE future_data (value TEXT NOT NULL);
                 INSERT INTO future_data VALUES ('keep');
                 PRAGMA user_version = 3;",
            )
            .unwrap();

        assert!(matches!(
            initialize(&connection),
            Err(StorageError::UnsupportedDatabaseSchemaVersion {
                found: 3,
                supported: SCHEMA_VERSION
            })
        ));
        assert_eq!(
            connection
                .query_row("SELECT value FROM future_data", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "keep"
        );
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn current_version_has_a_complete_registered_migration_plan() {
        migrations::validate_registered_plan(1, SCHEMA_VERSION).unwrap();
    }

    #[test]
    fn v1_migration_backfills_only_missing_usage_without_outbox_changes() {
        let connection = Connection::open_in_memory().unwrap();
        initialize(&connection).unwrap();
        connection
            .execute_batch(
                "DROP INDEX clipboard_items_deleted_last_used_idx;
             DROP INDEX clipboard_items_favorite_last_used_idx;
             PRAGMA user_version = 1;
             INSERT INTO sync_metadata (key, value) VALUES ('sync_enabled', '1');
             INSERT INTO clipboard_items
                 (id, kind, title, text_content, content_hash, size_bytes,
                  created_at_ms, last_used_at_ms, deleted)
             VALUES ('unused', 'text', 'title', 'keep body', 'hash-1', 9, 100, NULL, 0),
                    ('used', 'text', 'title', 'keep body', 'hash-2', 9, 200, 50, 0),
                    ('deleted', 'text', 'title', 'keep body', 'hash-3', 9, 300, NULL, 1);
             INSERT INTO item_tags (item_id, tag) VALUES ('unused', 'keep-tag');",
            )
            .unwrap();
        let snapshots = |connection: &Connection| {
            connection
                .prepare(
                    "SELECT id, text_content, created_at_ms, modified_at_ms, deleted
                 FROM clipboard_items ORDER BY id",
                )
                .unwrap()
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        let outbox_counts = |connection: &Connection| {
            connection.query_row(
                "SELECT (SELECT COUNT(*) FROM search_outbox), (SELECT COUNT(*) FROM sync_outbox)",
                [], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            ).unwrap()
        };
        let before = snapshots(&connection);
        let counts = outbox_counts(&connection);

        assert!(!initialize(&connection).unwrap().was_reset);
        assert_eq!(snapshots(&connection), before);
        assert_eq!(outbox_counts(&connection), counts);
        for (id, expected) in [("unused", 100), ("used", 50), ("deleted", 300)] {
            let actual: i64 = connection
                .query_row(
                    "SELECT last_used_at_ms FROM clipboard_items WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(actual, expected);
        }
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let index_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN
             ('clipboard_items_deleted_last_used_idx', 'clipboard_items_favorite_last_used_idx')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 2);
        let tag: String = connection
            .query_row(
                "SELECT tag FROM item_tags WHERE item_id = 'unused'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tag, "keep-tag");
        assert!(!initialize(&connection).unwrap().was_reset);
        assert_eq!(outbox_counts(&connection), counts);
    }

    #[test]
    fn reopening_current_schema_preserves_rows() {
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
