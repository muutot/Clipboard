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
        );

        -- Sync metadata: stores the local device identifier.
        -- Triggers read this to populate sync_changelog.device_id.
        CREATE TABLE IF NOT EXISTS sync_metadata (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL DEFAULT ''
        );

        -- Provider/endpoint/path-scoped sync state. Keeping remote state out of
        -- the global sync_metadata namespace prevents one remote from causing
        -- another remote's baseline or oplogs to be skipped.
        CREATE TABLE IF NOT EXISTS sync_remote_state (
            remote_scope TEXT NOT NULL,
            state_key TEXT NOT NULL,
            value TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (remote_scope, state_key)
        );

        -- Applied remote oplog object revisions. Oplogs are immutable for new
        -- clients; legacy mutable names are deliberately re-read every sync.
        CREATE TABLE IF NOT EXISTS sync_applied_oplogs (
            remote_scope TEXT NOT NULL,
            object_name TEXT NOT NULL,
            revision TEXT NOT NULL,
            applied_at_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (remote_scope, object_name)
        );

        -- Maps a remote text/link id to the matching local row when two
        -- devices captured the same content independently under different
        -- per-capture timestamp ids. The alias survives later content edits so
        -- update/delete oplogs continue to resolve to the same entity.
        CREATE TABLE IF NOT EXISTS sync_item_aliases (
            alias_id TEXT PRIMARY KEY NOT NULL,
            item_id TEXT NOT NULL,
            FOREIGN KEY (item_id) REFERENCES clipboard_items (id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS sync_item_aliases_item_idx
            ON sync_item_aliases (item_id);

        -- Operation log for multi-device sync. Records every insert/update/delete
        -- on clipboard_items so devices can exchange changes.
        -- `modified_at_ms` + `device_id` enable conflict resolution (last-write-wins).
        -- `sequence` provides total ordering within a device.
        CREATE TABLE IF NOT EXISTS sync_changelog (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id TEXT NOT NULL,
            operation TEXT NOT NULL CHECK (operation IN ('insert', 'update', 'delete')),
            kind TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            content_hash TEXT NOT NULL DEFAULT '',
            resource_path TEXT,
            preview_path TEXT,
            icon_path TEXT,
            text_content TEXT,
            html_content TEXT,
            rtf_content TEXT,
            metadata_json TEXT,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            source_app TEXT,
            size_bytes INTEGER NOT NULL DEFAULT 0,
            last_used_at_ms INTEGER,
            created_at_ms INTEGER NOT NULL,
            modified_at_ms INTEGER NOT NULL DEFAULT 0,
            device_id TEXT NOT NULL DEFAULT '',
            synced INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS sync_changelog_synced_seq_idx
            ON sync_changelog (synced, sequence);
        CREATE INDEX IF NOT EXISTS sync_changelog_created_idx
            ON sync_changelog (created_at_ms);

        -- Triggers to populate sync_changelog on every data mutation.
        -- device_id is read from sync_metadata (set at startup by the app).
        -- The WHEN guard suppresses echo when remote oplog/baseline data is
        -- being applied locally (sync_metadata key 'sync_suppress_changelog'),
        -- so received entries are not re-broadcast back to the remote.
        -- The triggers are dropped and recreated on every open so databases
        -- created before the guard was added are upgraded in place.
        DROP TRIGGER IF EXISTS clipboard_items_sync_insert;
        CREATE TRIGGER IF NOT EXISTS clipboard_items_sync_insert
        AFTER INSERT ON clipboard_items
        WHEN NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_suppress_changelog' AND value = '1'
        )
        AND NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_enabled' AND value = '1'
        )
        BEGIN
            INSERT INTO sync_changelog
                (item_id, operation, kind, title, content_hash, resource_path,
                 preview_path, icon_path, created_at_ms, modified_at_ms, device_id,
                 text_content, html_content, rtf_content, metadata_json,
                 is_favorite, source_app, size_bytes, last_used_at_ms)
            VALUES
                (NEW.id, 'insert', NEW.kind, NEW.title, NEW.content_hash,
                 NEW.resource_path, NEW.preview_path, NEW.icon_path,
                 NEW.created_at_ms, NEW.created_at_ms,
                 COALESCE((SELECT value FROM sync_metadata WHERE key = 'device_id'), 'unknown'),
                 NEW.text_content, NEW.html_content, NEW.rtf_content, NEW.metadata_json,
                 NEW.is_favorite, NEW.source_app, NEW.size_bytes, NEW.last_used_at_ms);
        END;

        DROP TRIGGER IF EXISTS clipboard_items_sync_update;
        CREATE TRIGGER IF NOT EXISTS clipboard_items_sync_update
        AFTER UPDATE ON clipboard_items
        WHEN NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_suppress_changelog' AND value = '1'
        )
        AND NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_enabled' AND value = '1'
        )
        AND (
            OLD.title != NEW.title
          OR OLD.text_content IS NOT NEW.text_content
          OR OLD.html_content IS NOT NEW.html_content
          OR OLD.rtf_content IS NOT NEW.rtf_content
          OR OLD.resource_path IS NOT NEW.resource_path
          OR OLD.preview_path IS NOT NEW.preview_path
          OR OLD.icon_path IS NOT NEW.icon_path
          OR OLD.is_favorite != NEW.is_favorite
          OR OLD.deleted != NEW.deleted
          OR OLD.metadata_json IS NOT NEW.metadata_json
        )
        BEGIN
            INSERT INTO sync_changelog
                (item_id, operation, kind, title, content_hash, resource_path,
                 preview_path, icon_path, created_at_ms, modified_at_ms, device_id,
                 text_content, html_content, rtf_content, metadata_json,
                 is_favorite, source_app, size_bytes, last_used_at_ms)
            VALUES
                (NEW.id,
                 CASE WHEN OLD.deleted = 0 AND NEW.deleted = 1 THEN 'delete'
                      WHEN OLD.deleted = 1 AND NEW.deleted = 0 THEN 'insert'
                      ELSE 'update' END,
                 NEW.kind, NEW.title, NEW.content_hash,
                 NEW.resource_path, NEW.preview_path, NEW.icon_path,
                 NEW.created_at_ms,
                 COALESCE(NEW.modified_at_ms, strftime('%s', 'now') * 1000),
                 COALESCE((SELECT value FROM sync_metadata WHERE key = 'device_id'), 'unknown'),
                 NEW.text_content, NEW.html_content, NEW.rtf_content, NEW.metadata_json,
                 NEW.is_favorite, NEW.source_app, NEW.size_bytes, NEW.last_used_at_ms);
        END;

        DROP TRIGGER IF EXISTS clipboard_items_sync_delete;
        CREATE TRIGGER IF NOT EXISTS clipboard_items_sync_delete
        AFTER DELETE ON clipboard_items
        WHEN NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_suppress_changelog' AND value = '1'
        )
        AND NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_enabled' AND value = '1'
        )
        BEGIN
            INSERT INTO sync_changelog
                (item_id, operation, kind, title, content_hash, resource_path,
                 preview_path, icon_path, created_at_ms, modified_at_ms, device_id,
                 text_content, html_content, rtf_content, metadata_json,
                 is_favorite, source_app, size_bytes, last_used_at_ms)
            VALUES
                (OLD.id, 'delete', OLD.kind, OLD.title, OLD.content_hash,
                 OLD.resource_path, OLD.preview_path, OLD.icon_path,
                 OLD.created_at_ms, strftime('%s', 'now') * 1000,
                 COALESCE((SELECT value FROM sync_metadata WHERE key = 'device_id'), 'unknown'),
                 OLD.text_content, OLD.html_content, OLD.rtf_content, OLD.metadata_json,
                 OLD.is_favorite, OLD.source_app, OLD.size_bytes, OLD.last_used_at_ms);
        END;

        -- Auto-set modified_at_ms on genuine local row updates. Remote sync
        -- applies carry their source timestamp and must not be rewritten to
        -- the receiver's wall-clock time, otherwise LWW stops converging.
        DROP TRIGGER IF EXISTS clipboard_items_set_modified;
        CREATE TRIGGER IF NOT EXISTS clipboard_items_set_modified
        AFTER UPDATE ON clipboard_items
        WHEN NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_suppress_changelog' AND value = '1'
        )
        AND NOT EXISTS (
            SELECT 1 FROM sync_metadata
            WHERE key = 'sync_enabled' AND value = '1'
        )
        BEGIN
            UPDATE clipboard_items SET modified_at_ms = strftime('%s', 'now') * 1000
            WHERE id = NEW.id AND (modified_at_ms IS NULL OR modified_at_ms < strftime('%s', 'now') * 1000 - 1000);
        END;",
    )?;

    ensure_item_tags_table(connection)?;

    // Databases created before HTML capture gained a column get it here.
    // `CREATE TABLE IF NOT EXISTS` never alters an existing table, so a
    // separate idempotent ALTER is required for upgrades.
    ensure_column(connection, "clipboard_items", "html_content", "TEXT")?;
    ensure_column(connection, "clipboard_items", "rtf_content", "TEXT")?;
    ensure_column(connection, "clipboard_items", "modified_at_ms", "INTEGER")?;
    ensure_column(
        connection,
        "clipboard_items",
        "sync_writer_device_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        connection,
        "sync_changelog",
        "modified_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(connection, "sync_changelog", "text_content", "TEXT")?;
    ensure_column(connection, "sync_changelog", "html_content", "TEXT")?;
    ensure_column(connection, "sync_changelog", "rtf_content", "TEXT")?;
    ensure_column(connection, "sync_changelog", "metadata_json", "TEXT")?;
    ensure_column(
        connection,
        "sync_changelog",
        "is_favorite",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(connection, "sync_changelog", "source_app", "TEXT")?;
    ensure_column(
        connection,
        "sync_changelog",
        "size_bytes",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(connection, "sync_changelog", "last_used_at_ms", "INTEGER")?;

    // `search_outbox.sequence` is an INTEGER PRIMARY KEY, which already
    // creates an index on the column, so the redundant explicit index adds
    // maintenance cost on every outbox insert without helping reads. Drop it
    // from databases that predate this cleanup; `IF EXISTS` keeps this safe on
    // every open and for fresh databases.
    connection.execute_batch("DROP INDEX IF EXISTS search_outbox_sequence_idx;")?;

    create_sync_schema(connection)?;

    Ok(())
}

fn create_sync_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS sync_outbox (
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
            snapshot_size_bytes INTEGER NOT NULL DEFAULT 0,
            snapshot_record_count INTEGER NOT NULL DEFAULT 0,
            snapshot_sequence INTEGER NOT NULL DEFAULT 0,
            published_sequence INTEGER NOT NULL DEFAULT 0,
            last_segment_key TEXT,
            remote_prepared INTEGER NOT NULL DEFAULT 0 CHECK (remote_prepared IN (0, 1)),
            initialized INTEGER NOT NULL DEFAULT 0 CHECK (initialized IN (0, 1)),
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS sync_cursors (
            remote_scope TEXT NOT NULL,
            device_id TEXT NOT NULL,
            epoch TEXT NOT NULL,
            sequence INTEGER NOT NULL DEFAULT 0,
            snapshot_sha256 TEXT,
            last_segment_key TEXT,
            updated_at_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (remote_scope, device_id)
        );

        CREATE TABLE IF NOT EXISTS sync_remote_resources (
            remote_scope TEXT NOT NULL,
            object_key TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            size_bytes INTEGER NOT NULL DEFAULT 0,
            confirmed_at_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (remote_scope, object_key)
        );

        DROP TRIGGER IF EXISTS clipboard_items_sync_outbox_insert;
        CREATE TRIGGER clipboard_items_sync_outbox_insert
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

        DROP TRIGGER IF EXISTS clipboard_items_sync_outbox_update;
        CREATE TRIGGER clipboard_items_sync_outbox_update
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

        DROP TRIGGER IF EXISTS clipboard_items_sync_outbox_delete;
        CREATE TRIGGER clipboard_items_sync_outbox_delete
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
    ensure_column(
        connection,
        "sync_publication_state",
        "snapshot_size_bytes",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "sync_publication_state",
        "snapshot_record_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
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
