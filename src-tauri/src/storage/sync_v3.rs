use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params_from_iter, OptionalExtension, Row};

use super::{Database, StorageError};
use crate::{
    domain::{ClipboardItem, ClipboardKind},
    sync::v3::{MutationBatch, RecordVersion, ReplicatedItem, Tombstone},
};

const PROTOCOL_VERSION: &str = "3";
const LOOKUP_CHUNK_SIZE: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncV3Snapshot {
    pub through_sequence: u64,
    pub mutations: MutationBatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncV3OutboxBatch {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub mutations: MutationBatch,
}

struct StoredReplicatedItem {
    id: String,
    kind: String,
    title: String,
    text_content: Option<String>,
    html_content: Option<String>,
    rtf_content: Option<String>,
    resource_path: Option<String>,
    preview_path: Option<String>,
    content_hash: String,
    source_app: Option<String>,
    icon_path: Option<String>,
    size_bytes: i64,
    created_at_ms: i64,
    last_used_at_ms: Option<i64>,
    is_favorite: bool,
    metadata_json: Option<String>,
    modified_at_ms: i64,
    writer_device_id: String,
}

impl StoredReplicatedItem {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind: row.get(1)?,
            title: row.get(2)?,
            text_content: row.get(3)?,
            html_content: row.get(4)?,
            rtf_content: row.get(5)?,
            resource_path: row.get(6)?,
            preview_path: row.get(7)?,
            content_hash: row.get(8)?,
            source_app: row.get(9)?,
            icon_path: row.get(10)?,
            size_bytes: row.get(11)?,
            created_at_ms: row.get(12)?,
            last_used_at_ms: row.get(13)?,
            is_favorite: row.get(14)?,
            metadata_json: row.get(15)?,
            modified_at_ms: row.get(16)?,
            writer_device_id: row.get(17)?,
        })
    }

    fn into_wire(self) -> Result<ReplicatedItem, StorageError> {
        let size_bytes =
            u64::try_from(self.size_bytes).map_err(|_| StorageError::InvalidStoredValue {
                field: "clipboard_items.size_bytes",
                value: self.size_bytes,
            })?;
        Ok(ReplicatedItem {
            item: ClipboardItem {
                id: self.id,
                kind: kind_from_storage(&self.kind)?,
                title: self.title,
                text_content: self.text_content,
                html_content: self.html_content,
                rtf_content: self.rtf_content,
                resource_path: self.resource_path,
                preview_path: self.preview_path,
                content_hash: self.content_hash,
                source_app: self.source_app,
                icon_path: self.icon_path,
                size_bytes,
                created_at_ms: self.created_at_ms,
                last_used_at_ms: self.last_used_at_ms,
                is_favorite: self.is_favorite,
                metadata_json: self.metadata_json,
            },
            version: RecordVersion {
                modified_at_ms: self.modified_at_ms,
                writer_device_id: self.writer_device_id,
            },
        })
    }
}

struct StoredTombstone {
    item_id: String,
    kind: String,
    content_hash: String,
    deleted_at_ms: i64,
    modified_at_ms: i64,
    writer_device_id: String,
}

impl StoredTombstone {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            item_id: row.get(0)?,
            kind: row.get(1)?,
            content_hash: row.get(2)?,
            deleted_at_ms: row.get(3)?,
            modified_at_ms: row.get(4)?,
            writer_device_id: row.get(5)?,
        })
    }

    fn into_wire(self) -> Result<Tombstone, StorageError> {
        Ok(Tombstone {
            item_id: self.item_id,
            kind: kind_from_storage(&self.kind)?,
            content_hash: self.content_hash,
            deleted_at_ms: self.deleted_at_ms,
            version: RecordVersion {
                modified_at_ms: self.modified_at_ms,
                writer_device_id: self.writer_device_id,
            },
        })
    }
}

impl Database {
    /// Performs the one-time, intentionally incompatible transition to sync v3.
    /// Clipboard rows remain intact; only legacy sync state is discarded.
    pub fn enable_sync_v3(&self) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let current_version: Option<String> = transaction
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_protocol_version'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let reset = current_version.as_deref() != Some(PROTOCOL_VERSION);
            if reset {
                let device_id: String = transaction.query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'device_id'",
                    [],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "INSERT INTO sync_metadata (key, value)
                     VALUES ('sync_suppress_changelog', '1')
                     ON CONFLICT(key) DO UPDATE SET value = '1'",
                    [],
                )?;
                transaction.execute(
                    "UPDATE clipboard_items
                        SET modified_at_ms = COALESCE(modified_at_ms, created_at_ms),
                            sync_writer_device_id = CASE
                                WHEN sync_writer_device_id = '' THEN ?1
                                ELSE sync_writer_device_id
                            END",
                    [&device_id],
                )?;
                transaction.execute_batch(
                    "DELETE FROM sync_changelog;
                     DELETE FROM sync_remote_state;
                     DELETE FROM sync_applied_oplogs;
                     DELETE FROM sync_item_aliases;
                     DELETE FROM sync_v3_outbox;
                     DELETE FROM sync_v3_tombstones;
                     DELETE FROM sync_v3_remote_state;
                     DELETE FROM sync_v3_cursors;
                     DELETE FROM sync_v3_remote_resources;
                     DELETE FROM sqlite_sequence WHERE name = 'sync_v3_outbox';",
                )?;
                transaction.execute(
                    "INSERT INTO sync_v3_tombstones
                        (item_id, kind, content_hash, deleted_at_ms,
                         modified_at_ms, writer_device_id)
                     SELECT id, kind, content_hash,
                            COALESCE(deleted_at_ms, modified_at_ms, created_at_ms),
                            COALESCE(modified_at_ms, created_at_ms),
                            sync_writer_device_id
                       FROM clipboard_items
                      WHERE deleted = 1",
                    [],
                )?;
                transaction.execute(
                    "DELETE FROM sync_metadata
                      WHERE key IN ('legacy_device_id', 'sync_suppress_changelog')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO sync_metadata (key, value)
                     VALUES ('sync_protocol_version', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    [PROTOCOL_VERSION],
                )?;
            }
            transaction.execute(
                "INSERT INTO sync_metadata (key, value)
                 VALUES ('sync_v3_enabled', '1')
                 ON CONFLICT(key) DO UPDATE SET value = '1'",
                [],
            )?;
            transaction.commit()?;
            Ok(reset)
        })
    }

    pub fn is_sync_v3_enabled(&self) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let enabled: Option<String> = connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_v3_enabled'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(enabled.as_deref() == Some("1"))
        })
    }

    pub fn export_sync_v3_snapshot(&self) -> Result<SyncV3Snapshot, StorageError> {
        self.with_connection(|connection| {
            let through_sequence = current_sequence(connection)?;
            let mutations = load_mutations(connection, None)?;
            Ok(SyncV3Snapshot {
                through_sequence,
                mutations,
            })
        })
    }

    pub fn get_sync_v3_outbox_batch(
        &self,
        limit: usize,
    ) -> Result<Option<SyncV3OutboxBatch>, StorageError> {
        if limit == 0 {
            return Ok(None);
        }
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT sequence, item_id
                   FROM sync_v3_outbox
                  ORDER BY sequence ASC
                  LIMIT ?1",
            )?;
            let rows = statement
                .query_map([limit as i64], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let Some((first_sequence, _)) = rows.first() else {
                return Ok(None);
            };
            let first_sequence = *first_sequence;
            let last_sequence = rows.last().map(|row| row.0).unwrap_or(first_sequence);
            let item_ids = rows
                .into_iter()
                .map(|(_, item_id)| item_id)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let mutations = load_mutations(connection, Some(&item_ids))?;
            if mutations.len() != item_ids.len() {
                return Err(StorageError::InvalidSyncState(
                    "outbox references an item without a live row or tombstone".to_string(),
                ));
            }
            Ok(Some(SyncV3OutboxBatch {
                first_sequence: u64::try_from(first_sequence).map_err(|_| {
                    StorageError::InvalidStoredValue {
                        field: "sync_v3_outbox.sequence",
                        value: first_sequence,
                    }
                })?,
                last_sequence: u64::try_from(last_sequence).map_err(|_| {
                    StorageError::InvalidStoredValue {
                        field: "sync_v3_outbox.sequence",
                        value: last_sequence,
                    }
                })?,
                mutations,
            }))
        })
    }

    pub fn acknowledge_sync_v3_outbox(&self, through_sequence: u64) -> Result<u64, StorageError> {
        let through_sequence =
            i64::try_from(through_sequence).map_err(|_| StorageError::ValueOutOfRange {
                field: "sync_v3_outbox.sequence",
            })?;
        self.with_connection(|connection| {
            let deleted = connection.execute(
                "DELETE FROM sync_v3_outbox WHERE sequence <= ?1",
                [through_sequence],
            )?;
            Ok(deleted as u64)
        })
    }

    pub fn count_sync_v3_outbox(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 =
                connection
                    .query_row("SELECT COUNT(*) FROM sync_v3_outbox", [], |row| row.get(0))?;
            u64::try_from(count).map_err(|_| StorageError::InvalidStoredValue {
                field: "sync_v3_outbox.count",
                value: count,
            })
        })
    }
}

fn kind_from_storage(kind: &str) -> Result<ClipboardKind, StorageError> {
    match kind {
        "text" => Ok(ClipboardKind::Text),
        "link" => Ok(ClipboardKind::Link),
        "image" => Ok(ClipboardKind::Image),
        "file" => Ok(ClipboardKind::File),
        _ => Err(StorageError::InvalidClipboardKind(kind.to_string())),
    }
}

fn current_sequence(connection: &rusqlite::Connection) -> Result<u64, StorageError> {
    let sequence: Option<i64> = connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'sync_v3_outbox'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let sequence = sequence.unwrap_or(0);
    u64::try_from(sequence).map_err(|_| StorageError::InvalidStoredValue {
        field: "sync_v3_outbox.sequence",
        value: sequence,
    })
}

fn load_mutations(
    connection: &rusqlite::Connection,
    item_ids: Option<&[String]>,
) -> Result<MutationBatch, StorageError> {
    let mut upserts = BTreeMap::<String, ReplicatedItem>::new();
    let mut tombstones = BTreeMap::<String, Tombstone>::new();
    if let Some(item_ids) = item_ids {
        for chunk in item_ids.chunks(LOOKUP_CHUNK_SIZE) {
            let placeholders = (1..=chunk.len())
                .map(|position| format!("?{position}"))
                .collect::<Vec<_>>()
                .join(", ");
            let item_sql = format!(
                "SELECT id, kind, title, text_content, html_content, rtf_content,
                        resource_path, preview_path, content_hash, source_app,
                        icon_path, size_bytes, created_at_ms, last_used_at_ms,
                        is_favorite, metadata_json,
                        COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
                   FROM clipboard_items
                  WHERE deleted = 0 AND id IN ({placeholders})"
            );
            let mut item_statement = connection.prepare(&item_sql)?;
            let stored_items = item_statement
                .query_map(
                    params_from_iter(chunk.iter()),
                    StoredReplicatedItem::from_row,
                )?
                .collect::<Result<Vec<_>, _>>()?;
            for stored in stored_items {
                let item = stored.into_wire()?;
                upserts.insert(item.item.id.clone(), item);
            }

            let tombstone_sql = format!(
                "SELECT item_id, kind, content_hash, deleted_at_ms,
                        modified_at_ms, writer_device_id
                   FROM sync_v3_tombstones
                  WHERE item_id IN ({placeholders})"
            );
            let mut tombstone_statement = connection.prepare(&tombstone_sql)?;
            let stored_tombstones = tombstone_statement
                .query_map(params_from_iter(chunk.iter()), StoredTombstone::from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            for stored in stored_tombstones {
                let tombstone = stored.into_wire()?;
                tombstones.insert(tombstone.item_id.clone(), tombstone);
            }
        }
    } else {
        let mut item_statement = connection.prepare(
            "SELECT id, kind, title, text_content, html_content, rtf_content,
                    resource_path, preview_path, content_hash, source_app,
                    icon_path, size_bytes, created_at_ms, last_used_at_ms,
                    is_favorite, metadata_json,
                    COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
               FROM clipboard_items
              WHERE deleted = 0
              ORDER BY id",
        )?;
        let stored_items = item_statement
            .query_map([], StoredReplicatedItem::from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut snapshot_upserts = Vec::with_capacity(stored_items.len());
        for stored in stored_items {
            snapshot_upserts.push(stored.into_wire()?);
        }

        let mut tombstone_statement = connection.prepare(
            "SELECT item_id, kind, content_hash, deleted_at_ms,
                    modified_at_ms, writer_device_id
               FROM sync_v3_tombstones
              ORDER BY item_id",
        )?;
        let stored_tombstones = tombstone_statement
            .query_map([], StoredTombstone::from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut snapshot_tombstones = Vec::with_capacity(stored_tombstones.len());
        for stored in stored_tombstones {
            snapshot_tombstones.push(stored.into_wire()?);
        }
        return Ok(MutationBatch {
            upserts: snapshot_upserts,
            tombstones: snapshot_tombstones,
        });
    }

    for (item_id, tombstone) in &tombstones {
        if let Some(item) = upserts.get(item_id) {
            if tombstone.version >= item.version {
                upserts.remove(item_id);
            }
        }
    }
    tombstones.retain(|item_id, tombstone| {
        upserts
            .get(item_id)
            .is_none_or(|item| tombstone.version >= item.version)
    });

    Ok(MutationBatch {
        upserts: upserts.into_values().collect(),
        tombstones: tombstones.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::ClipboardKind,
        storage::{ClipboardRepository, TextItemUpdate},
    };

    fn item(id: &str, hash: &str, text: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_string(),
            kind: ClipboardKind::Text,
            title: text.to_string(),
            text_content: Some(text.to_string()),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: hash.to_string(),
            source_app: Some("test".to_string()),
            icon_path: None,
            size_bytes: text.len() as u64,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: Some("{}".to_string()),
        }
    }

    #[test]
    fn enabling_v3_preserves_items_and_discards_only_legacy_sync_state() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("existing", "hash-existing", "existing"))
            .unwrap();
        assert_eq!(database.count_unsynced_changelog().unwrap(), 1);
        assert!(!database.is_sync_v3_enabled().unwrap());

        assert!(database.enable_sync_v3().unwrap());
        assert!(database.is_sync_v3_enabled().unwrap());
        assert_eq!(database.count_unsynced_changelog().unwrap(), 0);
        let snapshot = database.export_sync_v3_snapshot().unwrap();
        assert_eq!(snapshot.through_sequence, 0);
        assert_eq!(snapshot.mutations.upserts.len(), 1);
        assert_eq!(snapshot.mutations.upserts[0].item.id, "existing");
        assert_eq!(
            snapshot.mutations.upserts[0].version.writer_device_id,
            database.get_sync_device_id().unwrap()
        );

        database.save_item(&item("new", "hash-new", "new")).unwrap();
        assert_eq!(database.count_sync_v3_outbox().unwrap(), 1);
        assert_eq!(database.count_unsynced_changelog().unwrap(), 0);
    }

    #[test]
    fn outbox_batch_coalesces_repeated_changes_to_current_state() {
        let database = Database::open_in_memory().unwrap();
        database.enable_sync_v3().unwrap();
        database
            .save_item(&item("item", "hash-1", "first"))
            .unwrap();
        for (title, text, hash) in [("second", "second", "hash-2"), ("third", "third", "hash-3")] {
            database
                .update_text_item(&TextItemUpdate {
                    id: "item",
                    kind: ClipboardKind::Text,
                    title,
                    text_content: text,
                    content_hash: hash,
                    size_bytes: text.len() as u64,
                    metadata_json: None,
                })
                .unwrap();
        }

        let batch = database.get_sync_v3_outbox_batch(100).unwrap().unwrap();
        assert_eq!(batch.first_sequence, 1);
        assert_eq!(batch.last_sequence, 3);
        assert_eq!(batch.mutations.len(), 1);
        assert_eq!(batch.mutations.upserts[0].item.title, "third");
        assert_eq!(batch.mutations.upserts[0].item.content_hash, "hash-3");
        assert_eq!(database.acknowledge_sync_v3_outbox(3).unwrap(), 3);
        assert_eq!(database.count_sync_v3_outbox().unwrap(), 0);
        assert_eq!(
            database.export_sync_v3_snapshot().unwrap().through_sequence,
            3
        );
    }

    #[test]
    fn permanent_delete_keeps_a_compact_tombstone() {
        let database = Database::open_in_memory().unwrap();
        database.enable_sync_v3().unwrap();
        database
            .save_item(&item("deleted", "hash-deleted", "deleted"))
            .unwrap();
        assert!(database.soft_delete("deleted").unwrap());
        assert!(database.permanently_delete("deleted").unwrap());

        let snapshot = database.export_sync_v3_snapshot().unwrap();
        assert!(snapshot.mutations.upserts.is_empty());
        assert_eq!(snapshot.mutations.tombstones.len(), 1);
        assert_eq!(snapshot.mutations.tombstones[0].item_id, "deleted");
        assert_eq!(
            snapshot.mutations.tombstones[0].content_hash,
            "hash-deleted"
        );
    }

    #[test]
    fn restoring_a_soft_deleted_item_replaces_its_tombstone() {
        let database = Database::open_in_memory().unwrap();
        database.enable_sync_v3().unwrap();
        database
            .save_item(&item("restored", "hash-restored", "restored"))
            .unwrap();
        assert!(database.soft_delete("restored").unwrap());
        assert!(database.restore_deleted("restored").unwrap());

        let snapshot = database.export_sync_v3_snapshot().unwrap();
        assert_eq!(snapshot.mutations.upserts.len(), 1);
        assert!(snapshot.mutations.tombstones.is_empty());
    }

    #[test]
    fn enabling_v3_again_is_idempotent_and_keeps_pending_changes() {
        let database = Database::open_in_memory().unwrap();
        assert!(database.enable_sync_v3().unwrap());
        database
            .save_item(&item("pending", "hash-pending", "pending"))
            .unwrap();
        assert!(!database.enable_sync_v3().unwrap());
        assert_eq!(database.count_sync_v3_outbox().unwrap(), 1);
    }
}
