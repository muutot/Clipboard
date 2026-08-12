use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, params_from_iter, OptionalExtension, Row, Transaction};
use uuid::Uuid;

use super::{Database, StorageError};
use crate::{
    domain::{ClipboardItem, ClipboardKind},
    sync::v1::{
        checkpoint_object_key, parse_segment_key, DeviceCursor, DeviceHead, MutationBatch,
        ObjectRef, RecordVersion, ReplicatedItem, Tombstone,
    },
};

const LOOKUP_CHUNK_SIZE: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSnapshot {
    pub through_sequence: u64,
    pub mutations: MutationBatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutboxBatch {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub mutations: MutationBatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRemoteState {
    pub remote_scope: String,
    pub epoch: String,
    pub snapshot: Option<ObjectRef>,
    pub snapshot_sequence: u64,
    pub published_sequence: u64,
    pub last_segment_key: Option<String>,
    pub remote_prepared: bool,
    pub initialized: bool,
    pub updated_at_ms: i64,
}

impl SyncRemoteState {
    pub fn device_head(&self, device_id: &str) -> Result<DeviceHead, StorageError> {
        let snapshot = self.snapshot.clone().ok_or_else(|| {
            StorageError::InvalidSyncState("device head has no published snapshot".to_string())
        })?;
        Ok(DeviceHead {
            device_id: device_id.to_string(),
            epoch: self.epoch.clone(),
            snapshot,
            published_sequence: self.published_sequence,
            last_segment_key: self.last_segment_key.clone(),
            updated_at_ms: self.updated_at_ms,
        })
    }
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
            is_favorite: row.get(13)?,
            metadata_json: row.get(14)?,
            modified_at_ms: row.get(15)?,
            writer_device_id: row.get(16)?,
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
                last_used_at_ms: None,
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
    pub fn set_sync_device_id(&self, device_id: &str) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('device_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [device_id],
            )?;
            Ok(())
        })
    }

    pub fn get_sync_device_id(&self) -> Result<String, StorageError> {
        self.with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT value FROM sync_metadata WHERE key = 'device_id'",
                [],
                |row| row.get(0),
            )?)
        })
    }

    /// Ensures one stable UUID identity for the current database. Invalid
    /// values are replaced directly; no historical identity alias is kept.
    pub fn ensure_sync_device_id(&self) -> Result<String, StorageError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'device_id'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing) = existing.as_deref() {
                if Uuid::parse_str(existing).is_ok() {
                    transaction.commit()?;
                    return Ok(existing.to_string());
                }
            }

            let device_id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('device_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [&device_id],
            )?;
            transaction.commit()?;
            Ok(device_id)
        })
    }

    /// Enables the sole v1 replication state. Existing clipboard rows become
    /// the first local snapshot; no historical schema or sync state is read.
    pub fn initialize_sync(&self) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let current_enabled: Option<String> = transaction
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_enabled'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if current_enabled.as_deref() == Some("1") {
                transaction.commit()?;
                return Ok(false);
            }

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
                        sync_writer_device_id = ?1",
                [&device_id],
            )?;
            transaction.execute_batch(
                "DELETE FROM sync_item_aliases;
                 DELETE FROM sync_outbox;
                 DELETE FROM sync_tombstones;
                 DELETE FROM sync_publication_state;
                 DELETE FROM sync_cursors;
                 DELETE FROM sync_checkpoint_cursors;
                 DELETE FROM sync_checkpoint_state;
                 DELETE FROM sync_remote_resources;
                 DELETE FROM sqlite_sequence WHERE name = 'sync_outbox';",
            )?;
            transaction.execute(
                "INSERT INTO sync_tombstones
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
                "DELETE FROM sync_metadata WHERE key = 'sync_suppress_changelog'",
                [],
            )?;
            transaction.execute(
                "INSERT INTO sync_metadata (key, value)
                 VALUES ('sync_enabled', '1')
                 ON CONFLICT(key) DO UPDATE SET value = '1'",
                [],
            )?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub fn is_sync_initialized(&self) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let enabled: Option<String> = connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = 'sync_enabled'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(enabled.as_deref() == Some("1"))
        })
    }

    pub fn export_sync_snapshot(&self) -> Result<SyncSnapshot, StorageError> {
        self.with_connection(|connection| {
            let through_sequence = current_sequence(connection)?;
            let mutations = load_mutations(connection, None)?;
            Ok(SyncSnapshot {
                through_sequence,
                mutations,
            })
        })
    }

    pub fn get_sync_outbox_batch(
        &self,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, StorageError> {
        if limit == 0 {
            return Ok(None);
        }
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT sequence, item_id
                   FROM sync_outbox
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
            Ok(Some(SyncOutboxBatch {
                first_sequence: u64::try_from(first_sequence).map_err(|_| {
                    StorageError::InvalidStoredValue {
                        field: "sync_outbox.sequence",
                        value: first_sequence,
                    }
                })?,
                last_sequence: u64::try_from(last_sequence).map_err(|_| {
                    StorageError::InvalidStoredValue {
                        field: "sync_outbox.sequence",
                        value: last_sequence,
                    }
                })?,
                mutations,
            }))
        })
    }

    pub fn acknowledge_sync_outbox(&self, through_sequence: u64) -> Result<u64, StorageError> {
        let through_sequence =
            i64::try_from(through_sequence).map_err(|_| StorageError::ValueOutOfRange {
                field: "sync_outbox.sequence",
            })?;
        self.with_connection(|connection| {
            let deleted = connection.execute(
                "DELETE FROM sync_outbox WHERE sequence <= ?1",
                [through_sequence],
            )?;
            Ok(deleted as u64)
        })
    }

    pub fn count_sync_outbox(&self) -> Result<u64, StorageError> {
        self.with_connection(|connection| {
            let count: i64 =
                connection.query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))?;
            u64::try_from(count).map_err(|_| StorageError::InvalidStoredValue {
                field: "sync_outbox.count",
                value: count,
            })
        })
    }

    pub fn get_or_create_sync_remote_state(
        &self,
        remote_scope: &str,
    ) -> Result<SyncRemoteState, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some(state) = load_remote_state(&transaction, remote_scope)? {
                transaction.commit()?;
                return Ok(state);
            }
            let epoch = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO sync_publication_state (remote_scope, epoch, updated_at_ms)
                 VALUES (?1, ?2, ?3)",
                params![remote_scope, &epoch, current_time_ms()],
            )?;
            let state = load_remote_state(&transaction, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState(
                    "newly created remote state could not be read".to_string(),
                )
            })?;
            transaction.commit()?;
            Ok(state)
        })
    }

    pub fn reset_sync_remote_state(
        &self,
        remote_scope: &str,
    ) -> Result<SyncRemoteState, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let epoch = Uuid::new_v4().to_string();
            connection.execute(
                "INSERT INTO sync_publication_state
                    (remote_scope, epoch, updated_at_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(remote_scope) DO UPDATE SET
                    epoch = excluded.epoch,
                    snapshot_key = NULL,
                    snapshot_sha256 = NULL,
                    snapshot_size_bytes = 0,
                    snapshot_record_count = 0,
                    snapshot_sequence = 0,
                    published_sequence = 0,
                    last_segment_key = NULL,
                    initialized = 0,
                    updated_at_ms = excluded.updated_at_ms",
                params![remote_scope, &epoch, current_time_ms()],
            )?;
            load_remote_state(connection, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState("reset remote state is missing".to_string())
            })
        })
    }

    pub fn mark_sync_remote_prepared(&self, remote_scope: &str) -> Result<(), StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let affected = connection.execute(
                "UPDATE sync_publication_state
                    SET remote_prepared = 1, updated_at_ms = ?2
                  WHERE remote_scope = ?1",
                params![remote_scope, current_time_ms()],
            )?;
            if affected != 1 {
                return Err(StorageError::InvalidSyncState(
                    "cannot mark cleanup for an unknown remote scope".to_string(),
                ));
            }
            Ok(())
        })
    }

    pub fn commit_sync_bootstrap_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        snapshot: &ObjectRef,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, StorageError> {
        validate_remote_scope(remote_scope)?;
        let through_sequence = sequence_to_i64(through_sequence, "sync snapshot sequence")?;
        let snapshot_size = sequence_to_i64(snapshot.stored_size_bytes, "sync snapshot size")?;
        let snapshot_records =
            sequence_to_i64(snapshot.record_count, "sync snapshot record count")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if through_sequence
                > sequence_to_i64(current_sequence(&transaction)?, "sync current sequence")?
            {
                return Err(StorageError::InvalidSyncState(
                    "bootstrap publication exceeds the local outbox high-water".to_string(),
                ));
            }
            let affected = transaction.execute(
                "UPDATE sync_publication_state
                    SET snapshot_key = ?3,
                        snapshot_sha256 = ?4,
                        snapshot_size_bytes = ?5,
                        snapshot_record_count = ?6,
                        snapshot_sequence = ?7,
                        published_sequence = ?7,
                        last_segment_key = NULL,
                        initialized = 1,
                        updated_at_ms = ?8
                  WHERE remote_scope = ?1 AND epoch = ?2 AND remote_prepared = 1",
                params![
                    remote_scope,
                    expected_epoch,
                    &snapshot.key,
                    &snapshot.sha256,
                    snapshot_size,
                    snapshot_records,
                    through_sequence,
                    current_time_ms(),
                ],
            )?;
            if affected != 1 {
                return Err(StorageError::InvalidSyncState(
                    "remote epoch changed before bootstrap publication committed".to_string(),
                ));
            }
            transaction.execute(
                "DELETE FROM sync_outbox WHERE sequence <= ?1",
                [through_sequence],
            )?;
            let state = load_remote_state(&transaction, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState("published remote state is missing".to_string())
            })?;
            transaction.commit()?;
            Ok(state)
        })
    }

    pub fn commit_sync_segment_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        last_segment_key: &str,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, StorageError> {
        validate_remote_scope(remote_scope)?;
        let through_sequence = sequence_to_i64(through_sequence, "sync segment sequence")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let current = load_remote_state(&transaction, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState("remote publication state is missing".to_string())
            })?;
            if current.epoch != expected_epoch {
                return Err(StorageError::InvalidSyncState(
                    "remote epoch changed before segment publication committed".to_string(),
                ));
            }
            if !current.initialized || current.snapshot.is_none() {
                return Err(StorageError::InvalidSyncState(
                    "cannot publish a segment before the bootstrap snapshot".to_string(),
                ));
            }
            if through_sequence
                > sequence_to_i64(current_sequence(&transaction)?, "sync current sequence")?
            {
                return Err(StorageError::InvalidSyncState(
                    "segment publication exceeds the local outbox high-water".to_string(),
                ));
            }
            if through_sequence
                < sequence_to_i64(current.published_sequence, "sync published sequence")?
            {
                return Err(StorageError::InvalidSyncState(
                    "segment publication sequence regressed".to_string(),
                ));
            }
            if through_sequence
                == sequence_to_i64(current.published_sequence, "sync published sequence")?
                && current.last_segment_key.as_deref() != Some(last_segment_key)
            {
                return Err(StorageError::InvalidSyncState(
                    "equal publication sequence has a different segment key".to_string(),
                ));
            }
            transaction.execute(
                "UPDATE sync_publication_state
                    SET published_sequence = ?3,
                        last_segment_key = ?4,
                        updated_at_ms = ?5
                  WHERE remote_scope = ?1 AND epoch = ?2",
                params![
                    remote_scope,
                    expected_epoch,
                    through_sequence,
                    last_segment_key,
                    current_time_ms(),
                ],
            )?;
            transaction.execute(
                "DELETE FROM sync_outbox WHERE sequence <= ?1",
                [through_sequence],
            )?;
            let state = load_remote_state(&transaction, remote_scope)?.ok_or_else(|| {
                StorageError::InvalidSyncState("published remote state is missing".to_string())
            })?;
            transaction.commit()?;
            Ok(state)
        })
    }

    pub fn get_sync_cursor(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<DeviceCursor>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| load_cursor(connection, remote_scope, device_id))
    }

    pub fn list_sync_cursors(&self, remote_scope: &str) -> Result<Vec<DeviceCursor>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT device_id, epoch, sequence, last_segment_key
                   FROM sync_cursors
                  WHERE remote_scope = ?1
                  ORDER BY device_id",
            )?;
            let rows = statement
                .query_map([remote_scope], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(device_id, epoch, sequence, last_segment_key)| {
                    Ok(DeviceCursor {
                        device_id,
                        epoch,
                        sequence: stored_sequence(sequence, "sync cursor sequence")?,
                        last_segment_key,
                    })
                })
                .collect()
        })
    }

    pub fn get_sync_checkpoint_state(
        &self,
        remote_scope: &str,
    ) -> Result<Option<(u64, String)>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT generation, checkpoint_sha256
                       FROM sync_checkpoint_state
                      WHERE remote_scope = ?1",
                    [remote_scope],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
                .map(|(generation, sha256)| {
                    let generation = stored_sequence(generation, "sync checkpoint generation")?;
                    checkpoint_object_key(generation, &sha256)
                        .map_err(StorageError::InvalidSyncState)?;
                    Ok((generation, sha256))
                })
                .transpose()
        })
    }

    pub fn get_sync_checkpoint_cursors(
        &self,
        remote_scope: &str,
    ) -> Result<Vec<DeviceCursor>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| load_checkpoint_cursors(connection, remote_scope))
    }

    pub fn record_sync_checkpoint_published(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
    ) -> Result<(), StorageError> {
        validate_remote_scope(remote_scope)?;
        checkpoint_object_key(generation, checkpoint_sha256)
            .map_err(StorageError::InvalidSyncState)?;
        validate_checkpoint_cursors(cursors)?;
        let generation = sequence_to_i64(generation, "sync checkpoint generation")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some((stored_generation, stored_sha256)) = transaction
                .query_row(
                    "SELECT generation, checkpoint_sha256
                       FROM sync_checkpoint_state
                      WHERE remote_scope = ?1",
                    [remote_scope],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
            {
                if stored_generation > generation {
                    return Err(StorageError::InvalidSyncState(
                        "published checkpoint generation regressed".to_string(),
                    ));
                }
                if stored_generation == generation && stored_sha256 != checkpoint_sha256 {
                    return Err(StorageError::InvalidSyncState(
                        "equal published checkpoint generation has a different digest".to_string(),
                    ));
                }
            }
            transaction.execute(
                "INSERT INTO sync_checkpoint_state
                    (remote_scope, generation, checkpoint_sha256, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(remote_scope) DO UPDATE SET
                    generation = excluded.generation,
                    checkpoint_sha256 = excluded.checkpoint_sha256,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    remote_scope,
                    generation,
                    checkpoint_sha256,
                    current_time_ms(),
                ],
            )?;
            replace_checkpoint_cursors(&transaction, remote_scope, cursors)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn apply_sync_checkpoint(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        mutations: &MutationBatch,
    ) -> Result<u64, StorageError> {
        validate_remote_scope(remote_scope)?;
        checkpoint_object_key(generation, checkpoint_sha256)
            .map_err(StorageError::InvalidSyncState)?;
        let generation = sequence_to_i64(generation, "sync checkpoint generation")?;
        validate_checkpoint_cursors(cursors)?;

        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some((stored_generation, stored_sha256)) = transaction
                .query_row(
                    "SELECT generation, checkpoint_sha256
                       FROM sync_checkpoint_state
                      WHERE remote_scope = ?1",
                    [remote_scope],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
            {
                if stored_generation > generation {
                    return Err(StorageError::InvalidSyncState(
                        "checkpoint generation regressed".to_string(),
                    ));
                }
                if stored_generation == generation && stored_sha256 != checkpoint_sha256 {
                    return Err(StorageError::InvalidSyncState(
                        "equal checkpoint generation has a different digest".to_string(),
                    ));
                }
            }

            set_changelog_suppressed(&transaction, true)?;
            let applied = apply_mutations(&transaction, mutations)?;
            set_changelog_suppressed(&transaction, false)?;
            transaction.execute(
                "DELETE FROM sync_cursors WHERE remote_scope = ?1",
                [remote_scope],
            )?;
            for cursor in cursors {
                upsert_cursor(&transaction, remote_scope, cursor, None)?;
            }
            transaction.execute(
                "INSERT INTO sync_checkpoint_state
                    (remote_scope, generation, checkpoint_sha256, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(remote_scope) DO UPDATE SET
                    generation = excluded.generation,
                    checkpoint_sha256 = excluded.checkpoint_sha256,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    remote_scope,
                    generation,
                    checkpoint_sha256,
                    current_time_ms(),
                ],
            )?;
            replace_checkpoint_cursors(&transaction, remote_scope, cursors)?;
            transaction.commit()?;
            Ok(applied)
        })
    }

    pub fn apply_sync_snapshot(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        mutations: &MutationBatch,
    ) -> Result<u64, StorageError> {
        validate_remote_scope(remote_scope)?;
        validate_cursor_identity(cursor)?;
        if cursor.last_segment_key.is_some() {
            return Err(StorageError::InvalidSyncState(
                "snapshot cursor must not contain a segment key".to_string(),
            ));
        }
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some(existing) = load_cursor(&transaction, remote_scope, &cursor.device_id)? {
                if existing.epoch == cursor.epoch && existing.sequence > cursor.sequence {
                    return Err(StorageError::InvalidSyncState(
                        "snapshot cursor sequence regressed".to_string(),
                    ));
                }
            }
            set_changelog_suppressed(&transaction, true)?;
            let applied = apply_mutations(&transaction, mutations)?;
            set_changelog_suppressed(&transaction, false)?;
            upsert_cursor(&transaction, remote_scope, cursor, Some(snapshot_sha256))?;
            transaction.commit()?;
            Ok(applied)
        })
    }

    pub fn apply_sync_segment(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        mutations: &MutationBatch,
    ) -> Result<u64, StorageError> {
        validate_remote_scope(remote_scope)?;
        validate_cursor_identity(cursor)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let existing =
                load_cursor(&transaction, remote_scope, &cursor.device_id)?.ok_or_else(|| {
                    StorageError::InvalidSyncState(
                        "segment received before its device snapshot".to_string(),
                    )
                })?;
            if existing.epoch != cursor.epoch {
                return Err(StorageError::InvalidSyncState(
                    "segment epoch does not match the applied snapshot".to_string(),
                ));
            }
            if cursor.sequence < existing.sequence {
                return Err(StorageError::InvalidSyncState(
                    "segment cursor sequence regressed".to_string(),
                ));
            }
            if cursor.sequence == existing.sequence {
                if cursor.last_segment_key == existing.last_segment_key {
                    transaction.commit()?;
                    return Ok(0);
                }
                return Err(StorageError::InvalidSyncState(
                    "equal segment sequence has a different object key".to_string(),
                ));
            }
            if cursor.last_segment_key.is_none() {
                return Err(StorageError::InvalidSyncState(
                    "segment cursor is missing its object key".to_string(),
                ));
            }
            set_changelog_suppressed(&transaction, true)?;
            let applied = apply_mutations(&transaction, mutations)?;
            set_changelog_suppressed(&transaction, false)?;
            upsert_cursor(&transaction, remote_scope, cursor, None)?;
            transaction.commit()?;
            Ok(applied)
        })
    }
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn validate_remote_scope(remote_scope: &str) -> Result<(), StorageError> {
    if remote_scope.len() != 64
        || !remote_scope
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StorageError::InvalidSyncState(
            "remote scope must be a lowercase SHA-256 digest".to_string(),
        ));
    }
    Ok(())
}

fn sequence_to_i64(value: u64, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::ValueOutOfRange { field })
}

fn stored_sequence(value: i64, field: &'static str) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::InvalidStoredValue { field, value })
}

fn load_remote_state(
    connection: &rusqlite::Connection,
    remote_scope: &str,
) -> Result<Option<SyncRemoteState>, StorageError> {
    let stored = connection
        .query_row(
            "SELECT epoch, snapshot_key, snapshot_sha256,
                    snapshot_size_bytes, snapshot_record_count,
                    snapshot_sequence, published_sequence, last_segment_key,
                    remote_prepared, initialized, updated_at_ms
               FROM sync_publication_state
              WHERE remote_scope = ?1",
            [remote_scope],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, bool>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        epoch,
        snapshot_key,
        snapshot_sha256,
        snapshot_size_bytes,
        snapshot_record_count,
        snapshot_sequence,
        published_sequence,
        last_segment_key,
        remote_prepared,
        initialized,
        updated_at_ms,
    )) = stored
    else {
        return Ok(None);
    };
    let snapshot = match (snapshot_key, snapshot_sha256) {
        (Some(key), Some(sha256)) => Some(ObjectRef {
            key,
            sha256,
            stored_size_bytes: stored_sequence(snapshot_size_bytes, "sync snapshot size")?,
            record_count: stored_sequence(snapshot_record_count, "sync snapshot record count")?,
        }),
        (None, None) => None,
        _ => {
            return Err(StorageError::InvalidSyncState(
                "remote snapshot key/hash presence does not match".to_string(),
            ));
        }
    };
    Ok(Some(SyncRemoteState {
        remote_scope: remote_scope.to_string(),
        epoch,
        snapshot,
        snapshot_sequence: stored_sequence(snapshot_sequence, "sync snapshot sequence")?,
        published_sequence: stored_sequence(published_sequence, "sync published sequence")?,
        last_segment_key,
        remote_prepared,
        initialized,
        updated_at_ms,
    }))
}

fn load_cursor(
    connection: &rusqlite::Connection,
    remote_scope: &str,
    device_id: &str,
) -> Result<Option<DeviceCursor>, StorageError> {
    let stored = connection
        .query_row(
            "SELECT epoch, sequence, last_segment_key
               FROM sync_cursors
              WHERE remote_scope = ?1 AND device_id = ?2",
            params![remote_scope, device_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    stored
        .map(|(epoch, sequence, last_segment_key)| {
            Ok(DeviceCursor {
                device_id: device_id.to_string(),
                epoch,
                sequence: stored_sequence(sequence, "sync cursor sequence")?,
                last_segment_key,
            })
        })
        .transpose()
}

fn load_checkpoint_cursors(
    connection: &rusqlite::Connection,
    remote_scope: &str,
) -> Result<Vec<DeviceCursor>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT device_id, epoch, sequence, last_segment_key
           FROM sync_checkpoint_cursors
          WHERE remote_scope = ?1
          ORDER BY device_id",
    )?;
    let cursors = statement
        .query_map([remote_scope], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .map(|row| {
            let (device_id, epoch, sequence, last_segment_key) = row?;
            Ok(DeviceCursor {
                device_id,
                epoch,
                sequence: stored_sequence(sequence, "sync checkpoint cursor sequence")?,
                last_segment_key,
            })
        })
        .collect();
    cursors
}

fn validate_checkpoint_cursors(cursors: &[DeviceCursor]) -> Result<(), StorageError> {
    let mut identities = BTreeSet::new();
    for cursor in cursors {
        validate_cursor_identity(cursor)?;
        if let Some(key) = cursor.last_segment_key.as_deref() {
            let parsed = parse_segment_key(key).map_err(StorageError::InvalidSyncState)?;
            if parsed.device_id != cursor.device_id
                || parsed.epoch != cursor.epoch
                || parsed.last_sequence != cursor.sequence
            {
                return Err(StorageError::InvalidSyncState(
                    "checkpoint cursor does not match its segment key".to_string(),
                ));
            }
        }
        if !identities.insert(cursor.device_id.as_str()) {
            return Err(StorageError::InvalidSyncState(
                "checkpoint contains duplicate device cursors".to_string(),
            ));
        }
    }
    Ok(())
}

fn replace_checkpoint_cursors(
    transaction: &Transaction<'_>,
    remote_scope: &str,
    cursors: &[DeviceCursor],
) -> Result<(), StorageError> {
    transaction.execute(
        "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
        [remote_scope],
    )?;
    for cursor in cursors {
        transaction.execute(
            "INSERT INTO sync_checkpoint_cursors
                (remote_scope, device_id, epoch, sequence, last_segment_key)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                remote_scope,
                &cursor.device_id,
                &cursor.epoch,
                sequence_to_i64(cursor.sequence, "sync checkpoint cursor sequence")?,
                &cursor.last_segment_key,
            ],
        )?;
    }
    Ok(())
}

fn upsert_cursor(
    transaction: &Transaction<'_>,
    remote_scope: &str,
    cursor: &DeviceCursor,
    snapshot_sha256: Option<&str>,
) -> Result<(), StorageError> {
    let sequence = sequence_to_i64(cursor.sequence, "sync cursor sequence")?;
    transaction.execute(
        "INSERT INTO sync_cursors
            (remote_scope, device_id, epoch, sequence, snapshot_sha256,
             last_segment_key, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(remote_scope, device_id) DO UPDATE SET
            epoch = excluded.epoch,
            sequence = excluded.sequence,
            snapshot_sha256 = COALESCE(excluded.snapshot_sha256, sync_cursors.snapshot_sha256),
            last_segment_key = excluded.last_segment_key,
            updated_at_ms = excluded.updated_at_ms",
        params![
            remote_scope,
            &cursor.device_id,
            &cursor.epoch,
            sequence,
            snapshot_sha256,
            &cursor.last_segment_key,
            current_time_ms(),
        ],
    )?;
    Ok(())
}

fn set_changelog_suppressed(
    transaction: &Transaction<'_>,
    suppressed: bool,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO sync_metadata (key, value)
         VALUES ('sync_suppress_changelog', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [if suppressed { "1" } else { "0" }],
    )?;
    Ok(())
}

fn apply_mutations(
    transaction: &Transaction<'_>,
    mutations: &MutationBatch,
) -> Result<u64, StorageError> {
    let mut applied = 0u64;
    for replicated in &mutations.upserts {
        validate_record_version(&replicated.version)?;
        let kind = kind_to_storage(replicated.item.kind);
        let target_id = resolve_sync_item_id(
            transaction,
            &replicated.item.id,
            kind,
            &replicated.item.content_hash,
        )?
        .unwrap_or_else(|| replicated.item.id.clone());
        if winning_local_version(transaction, &target_id)?
            .is_some_and(|local| replicated.version <= local)
        {
            continue;
        }
        let size_bytes = i64::try_from(replicated.item.size_bytes).map_err(|_| {
            StorageError::ValueOutOfRange {
                field: "clipboard_items.size_bytes",
            }
        })?;
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM clipboard_items WHERE id = ?1)",
            [&target_id],
            |row| row.get::<_, bool>(0),
        )?;
        if exists {
            transaction.execute(
                "UPDATE clipboard_items
                    SET kind = ?2,
                        title = ?3,
                        text_content = ?4,
                        html_content = ?5,
                        rtf_content = ?6,
                        resource_path = ?7,
                        preview_path = ?8,
                        content_hash = ?9,
                        source_app = ?10,
                        icon_path = ?11,
                        size_bytes = ?12,
                        created_at_ms = ?13,
                        is_favorite = ?14,
                        metadata_json = ?15,
                        deleted = 0,
                        deleted_at_ms = NULL,
                        modified_at_ms = ?16,
                        sync_writer_device_id = ?17
                  WHERE id = ?1",
                params![
                    &target_id,
                    kind,
                    &replicated.item.title,
                    &replicated.item.text_content,
                    &replicated.item.html_content,
                    &replicated.item.rtf_content,
                    &replicated.item.resource_path,
                    &replicated.item.preview_path,
                    &replicated.item.content_hash,
                    &replicated.item.source_app,
                    &replicated.item.icon_path,
                    size_bytes,
                    replicated.item.created_at_ms,
                    replicated.item.is_favorite,
                    &replicated.item.metadata_json,
                    replicated.version.modified_at_ms,
                    &replicated.version.writer_device_id,
                ],
            )?;
        } else {
            transaction.execute(
                "INSERT INTO clipboard_items
                    (id, kind, title, text_content, html_content, rtf_content,
                     resource_path, preview_path, content_hash, source_app,
                     icon_path, size_bytes, created_at_ms, last_used_at_ms,
                     is_favorite, metadata_json, deleted, deleted_at_ms,
                     modified_at_ms, sync_writer_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                         ?11, ?12, ?13, ?13, ?14, ?15, 0, NULL, ?16, ?17)",
                params![
                    &target_id,
                    kind,
                    &replicated.item.title,
                    &replicated.item.text_content,
                    &replicated.item.html_content,
                    &replicated.item.rtf_content,
                    &replicated.item.resource_path,
                    &replicated.item.preview_path,
                    &replicated.item.content_hash,
                    &replicated.item.source_app,
                    &replicated.item.icon_path,
                    size_bytes,
                    replicated.item.created_at_ms,
                    replicated.item.is_favorite,
                    &replicated.item.metadata_json,
                    replicated.version.modified_at_ms,
                    &replicated.version.writer_device_id,
                ],
            )?;
        }
        transaction.execute(
            "DELETE FROM sync_tombstones WHERE item_id IN (?1, ?2)",
            params![&target_id, &replicated.item.id],
        )?;
        replace_item_tags(
            transaction,
            &target_id,
            replicated.item.metadata_json.as_deref(),
        )?;
        applied += 1;
    }

    for tombstone in &mutations.tombstones {
        validate_record_version(&tombstone.version)?;
        let kind = kind_to_storage(tombstone.kind);
        let target_id = resolve_sync_item_id(
            transaction,
            &tombstone.item_id,
            kind,
            &tombstone.content_hash,
        )?
        .unwrap_or_else(|| tombstone.item_id.clone());
        if winning_local_version(transaction, &target_id)?
            .is_some_and(|local| tombstone.version <= local)
        {
            continue;
        }
        transaction.execute(
            "UPDATE clipboard_items
                SET deleted = 1,
                    deleted_at_ms = ?2,
                    modified_at_ms = ?3,
                    sync_writer_device_id = ?4
              WHERE id = ?1",
            params![
                &target_id,
                tombstone.deleted_at_ms,
                tombstone.version.modified_at_ms,
                &tombstone.version.writer_device_id,
            ],
        )?;
        transaction.execute(
            "INSERT INTO sync_tombstones
                (item_id, kind, content_hash, deleted_at_ms,
                 modified_at_ms, writer_device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(item_id) DO UPDATE SET
                kind = excluded.kind,
                content_hash = excluded.content_hash,
                deleted_at_ms = excluded.deleted_at_ms,
                modified_at_ms = excluded.modified_at_ms,
                writer_device_id = excluded.writer_device_id",
            params![
                &target_id,
                kind,
                &tombstone.content_hash,
                tombstone.deleted_at_ms,
                tombstone.version.modified_at_ms,
                &tombstone.version.writer_device_id,
            ],
        )?;
        if target_id != tombstone.item_id {
            transaction.execute(
                "DELETE FROM sync_tombstones WHERE item_id = ?1",
                [&tombstone.item_id],
            )?;
        }
        transaction.execute("DELETE FROM item_tags WHERE item_id = ?1", [&target_id])?;
        applied += 1;
    }
    Ok(applied)
}

fn validate_record_version(version: &RecordVersion) -> Result<(), StorageError> {
    let writer = Uuid::parse_str(&version.writer_device_id).ok();
    if version.modified_at_ms < 0
        || writer
            .as_ref()
            .is_none_or(|writer| writer.to_string() != version.writer_device_id)
    {
        return Err(StorageError::InvalidSyncState(
            "record version must contain a non-negative timestamp and writer UUID".to_string(),
        ));
    }
    Ok(())
}

fn validate_cursor_identity(cursor: &DeviceCursor) -> Result<(), StorageError> {
    for (label, value) in [
        ("device", cursor.device_id.as_str()),
        ("epoch", cursor.epoch.as_str()),
    ] {
        let parsed = Uuid::parse_str(value).map_err(|_| {
            StorageError::InvalidSyncState(format!("cursor {label} id is not a UUID"))
        })?;
        if parsed.to_string() != value {
            return Err(StorageError::InvalidSyncState(format!(
                "cursor {label} id is not canonical lowercase"
            )));
        }
    }
    Ok(())
}

fn winning_local_version(
    transaction: &Transaction<'_>,
    item_id: &str,
) -> Result<Option<RecordVersion>, StorageError> {
    let row_version = transaction
        .query_row(
            "SELECT COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
               FROM clipboard_items
              WHERE id = ?1",
            [item_id],
            |row| {
                Ok(RecordVersion {
                    modified_at_ms: row.get(0)?,
                    writer_device_id: row.get(1)?,
                })
            },
        )
        .optional()?;
    let tombstone_version = transaction
        .query_row(
            "SELECT modified_at_ms, writer_device_id
               FROM sync_tombstones
              WHERE item_id = ?1",
            [item_id],
            |row| {
                Ok(RecordVersion {
                    modified_at_ms: row.get(0)?,
                    writer_device_id: row.get(1)?,
                })
            },
        )
        .optional()?;
    Ok(match (row_version, tombstone_version) {
        (Some(row), Some(tombstone)) => Some(row.max(tombstone)),
        (Some(row), None) => Some(row),
        (None, Some(tombstone)) => Some(tombstone),
        (None, None) => None,
    })
}

fn resolve_sync_item_id(
    transaction: &Transaction<'_>,
    remote_id: &str,
    kind: &str,
    content_hash: &str,
) -> Result<Option<String>, StorageError> {
    let exact = transaction
        .query_row(
            "SELECT id FROM clipboard_items WHERE id = ?1",
            [remote_id],
            |row| row.get(0),
        )
        .optional()?;
    if exact.is_some() {
        return Ok(exact);
    }
    let alias = transaction
        .query_row(
            "SELECT item_id FROM sync_item_aliases WHERE alias_id = ?1",
            [remote_id],
            |row| row.get(0),
        )
        .optional()?;
    if alias.is_some() {
        return Ok(alias);
    }
    if !matches!(kind, "text" | "link") {
        return Ok(None);
    }
    let matching = transaction
        .query_row(
            "SELECT id FROM clipboard_items WHERE kind = ?1 AND content_hash = ?2",
            params![kind, content_hash],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(item_id) = matching.as_deref() {
        transaction.execute(
            "INSERT INTO sync_item_aliases (alias_id, item_id)
             VALUES (?1, ?2)
             ON CONFLICT(alias_id) DO UPDATE SET item_id = excluded.item_id",
            params![remote_id, item_id],
        )?;
    }
    Ok(matching)
}

fn replace_item_tags(
    transaction: &Transaction<'_>,
    item_id: &str,
    metadata_json: Option<&str>,
) -> Result<(), StorageError> {
    transaction.execute("DELETE FROM item_tags WHERE item_id = ?1", [item_id])?;
    let Some(metadata_json) = metadata_json else {
        return Ok(());
    };
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(metadata_json) else {
        return Ok(());
    };
    let Some(tags) = metadata.get("tags").and_then(|value| value.as_array()) else {
        return Ok(());
    };
    let mut unique = BTreeSet::new();
    for tag in tags.iter().filter_map(|value| value.as_str()) {
        let tag = tag.trim();
        if !tag.is_empty() && unique.insert(tag) {
            transaction.execute(
                "INSERT INTO item_tags (item_id, tag) VALUES (?1, ?2)",
                params![item_id, tag],
            )?;
        }
    }
    Ok(())
}

fn kind_to_storage(kind: ClipboardKind) -> &'static str {
    match kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image => "image",
        ClipboardKind::File => "file",
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
            "SELECT seq FROM sqlite_sequence WHERE name = 'sync_outbox'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let sequence = sequence.unwrap_or(0);
    u64::try_from(sequence).map_err(|_| StorageError::InvalidStoredValue {
        field: "sync_outbox.sequence",
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
                        icon_path, size_bytes, created_at_ms,
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
                   FROM sync_tombstones
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
                    icon_path, size_bytes, created_at_ms,
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
               FROM sync_tombstones
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

    const REMOTE_SCOPE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const REMOTE_DEVICE: &str = "11111111-1111-4111-8111-111111111111";
    const REMOTE_EPOCH: &str = "22222222-2222-4222-8222-222222222222";

    fn replicated(id: &str, hash: &str, text: &str, version: RecordVersion) -> ReplicatedItem {
        ReplicatedItem {
            item: item(id, hash, text),
            version,
        }
    }

    fn cursor(sequence: u64, key: Option<&str>) -> DeviceCursor {
        DeviceCursor {
            device_id: REMOTE_DEVICE.to_string(),
            epoch: REMOTE_EPOCH.to_string(),
            sequence,
            last_segment_key: key.map(str::to_string),
        }
    }

    #[test]
    fn initializing_v1_preserves_items_and_discards_pending_v1_state() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("existing", "hash-existing", "existing"))
            .unwrap();
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO sync_item_aliases (alias_id, item_id)
                     VALUES ('pending-alias', 'existing')",
                    [],
                )?;
                connection.execute(
                    "UPDATE clipboard_items
                        SET sync_writer_device_id = 'pending-writer'
                      WHERE id = 'existing'",
                    [],
                )?;
                connection.execute(
                    "INSERT INTO sync_publication_state (remote_scope, epoch)
                     VALUES ('pending-remote', 'pending-epoch')",
                    [],
                )?;
                connection.execute(
                    "INSERT INTO sync_outbox
                        (item_id, operation, kind, content_hash, modified_at_ms, writer_device_id)
                     VALUES ('existing', 'upsert', 'text', 'hash-existing', 100, 'pending-writer')",
                    [],
                )?;
                connection.execute(
                    "INSERT INTO sync_checkpoint_state
                        (remote_scope, generation, checkpoint_sha256)
                     VALUES (?1, 1, ?2)",
                    params![REMOTE_SCOPE, "a".repeat(64)],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(!database.is_sync_initialized().unwrap());

        assert!(database.initialize_sync().unwrap());
        assert!(database.is_sync_initialized().unwrap());
        let snapshot = database.export_sync_snapshot().unwrap();
        assert_eq!(snapshot.through_sequence, 0);
        assert_eq!(snapshot.mutations.upserts.len(), 1);
        assert_eq!(snapshot.mutations.upserts[0].item.id, "existing");
        assert_eq!(
            snapshot.mutations.upserts[0].version.writer_device_id,
            database.get_sync_device_id().unwrap()
        );
        database
            .with_connection(|connection| {
                let alias_count: i64 =
                    connection.query_row("SELECT COUNT(*) FROM sync_item_aliases", [], |row| {
                        row.get(0)
                    })?;
                let publication_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM sync_publication_state",
                    [],
                    |row| row.get(0),
                )?;
                let outbox_count: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))?;
                let checkpoint_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM sync_checkpoint_state",
                    [],
                    |row| row.get(0),
                )?;
                let metadata_count: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM sync_metadata", [], |row| row.get(0))?;
                assert_eq!(alias_count, 0);
                assert_eq!(publication_count, 0);
                assert_eq!(outbox_count, 0);
                assert_eq!(checkpoint_count, 0);
                assert_eq!(metadata_count, 2);
                Ok(())
            })
            .unwrap();

        database.save_item(&item("new", "hash-new", "new")).unwrap();
        assert_eq!(database.count_sync_outbox().unwrap(), 1);
    }

    #[test]
    fn outbox_batch_coalesces_repeated_changes_to_current_state() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
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

        let batch = database.get_sync_outbox_batch(100).unwrap().unwrap();
        assert_eq!(batch.first_sequence, 1);
        assert_eq!(batch.last_sequence, 3);
        assert_eq!(batch.mutations.len(), 1);
        assert_eq!(batch.mutations.upserts[0].item.title, "third");
        assert_eq!(batch.mutations.upserts[0].item.content_hash, "hash-3");
        assert_eq!(database.acknowledge_sync_outbox(3).unwrap(), 3);
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
        assert_eq!(database.export_sync_snapshot().unwrap().through_sequence, 3);
    }

    #[test]
    fn local_last_used_changes_never_enter_the_sync_outbox_or_wire() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let mut local = item("local-usage", "hash-local-usage", "local usage");
        local.last_used_at_ms = Some(150);
        database.save_item(&local).unwrap();
        database.acknowledge_sync_outbox(1).unwrap();

        database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE clipboard_items SET last_used_at_ms = 999 WHERE id = 'local-usage'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(database.count_sync_outbox().unwrap(), 0);
        let snapshot = database.export_sync_snapshot().unwrap();
        assert_eq!(snapshot.mutations.upserts.len(), 1);
        assert_eq!(snapshot.mutations.upserts[0].item.last_used_at_ms, None);
        assert_eq!(
            database
                .get_item("local-usage")
                .unwrap()
                .unwrap()
                .last_used_at_ms,
            Some(999)
        );
    }

    #[test]
    fn remote_last_used_value_is_ignored_on_insert_and_update() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let mut remote = replicated(
            "remote-usage",
            "hash-remote-usage",
            "remote usage",
            RecordVersion {
                modified_at_ms: 500,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        );
        remote.item.created_at_ms = 100;
        remote.item.last_used_at_ms = Some(9_999);
        let first = MutationBatch {
            upserts: vec![remote.clone()],
            tombstones: Vec::new(),
        };
        database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(1, None), "a", &first)
            .unwrap();
        assert_eq!(
            database
                .get_item("remote-usage")
                .unwrap()
                .unwrap()
                .last_used_at_ms,
            Some(100)
        );

        database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE clipboard_items SET last_used_at_ms = 777 WHERE id = 'remote-usage'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        remote.item.title = "remote update".to_string();
        remote.item.last_used_at_ms = Some(8_888);
        remote.version.modified_at_ms = 501;
        let second = MutationBatch {
            upserts: vec![remote],
            tombstones: Vec::new(),
        };
        database
            .apply_sync_segment(REMOTE_SCOPE, &cursor(2, Some("segment-2")), &second)
            .unwrap();
        let stored = database.get_item("remote-usage").unwrap().unwrap();
        assert_eq!(stored.title, "remote update");
        assert_eq!(stored.last_used_at_ms, Some(777));
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
    }

    #[test]
    fn permanent_delete_keeps_a_compact_tombstone() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        database
            .save_item(&item("deleted", "hash-deleted", "deleted"))
            .unwrap();
        assert!(database.soft_delete("deleted").unwrap());
        let before_purge = database.count_sync_outbox().unwrap();
        assert!(database.permanently_delete("deleted").unwrap());
        assert_eq!(database.count_sync_outbox().unwrap(), before_purge);

        let snapshot = database.export_sync_snapshot().unwrap();
        assert!(snapshot.mutations.upserts.is_empty());
        assert_eq!(snapshot.mutations.tombstones.len(), 1);
        assert_eq!(snapshot.mutations.tombstones[0].item_id, "deleted");
        assert_eq!(
            snapshot.mutations.tombstones[0].content_hash,
            "hash-deleted"
        );
    }

    #[test]
    fn purging_a_remote_tombstone_does_not_echo_a_delete() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        database
            .save_item(&item(
                "remote-deleted",
                "hash-remote-deleted",
                "remote deleted",
            ))
            .unwrap();
        database.acknowledge_sync_outbox(1).unwrap();
        let remote_delete_version = current_time_ms() + 10_000;
        let delete = MutationBatch {
            upserts: Vec::new(),
            tombstones: vec![Tombstone {
                item_id: "remote-deleted".to_string(),
                kind: ClipboardKind::Text,
                content_hash: "hash-remote-deleted".to_string(),
                deleted_at_ms: remote_delete_version,
                version: RecordVersion {
                    modified_at_ms: remote_delete_version,
                    writer_device_id: REMOTE_DEVICE.to_string(),
                },
            }],
        };
        database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(1, None), "a", &delete)
            .unwrap();

        assert_eq!(database.count_sync_outbox().unwrap(), 0);
        assert!(database.permanently_delete("remote-deleted").unwrap());
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
        let snapshot = database.export_sync_snapshot().unwrap();
        assert_eq!(snapshot.mutations.tombstones.len(), 1);
        assert_eq!(
            snapshot.mutations.tombstones[0].version.writer_device_id,
            REMOTE_DEVICE
        );
    }

    #[test]
    fn deleting_an_active_row_directly_still_publishes_a_tombstone() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        database
            .save_item(&item(
                "direct-delete",
                "hash-direct-delete",
                "direct delete",
            ))
            .unwrap();
        database.acknowledge_sync_outbox(1).unwrap();

        assert!(database.delete_item("direct-delete").unwrap());
        assert_eq!(database.count_sync_outbox().unwrap(), 1);
        let snapshot = database.export_sync_snapshot().unwrap();
        assert_eq!(snapshot.mutations.tombstones.len(), 1);
        assert_eq!(snapshot.mutations.tombstones[0].item_id, "direct-delete");
    }

    #[test]
    fn restoring_a_soft_deleted_item_replaces_its_tombstone() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        database
            .save_item(&item("restored", "hash-restored", "restored"))
            .unwrap();
        assert!(database.soft_delete("restored").unwrap());
        assert!(database.restore_deleted("restored").unwrap());

        let snapshot = database.export_sync_snapshot().unwrap();
        assert_eq!(snapshot.mutations.upserts.len(), 1);
        assert!(snapshot.mutations.tombstones.is_empty());
    }

    #[test]
    fn initializing_v1_again_is_idempotent_and_keeps_pending_changes() {
        let database = Database::open_in_memory().unwrap();
        assert!(database.initialize_sync().unwrap());
        database
            .save_item(&item("pending", "hash-pending", "pending"))
            .unwrap();
        assert!(!database.initialize_sync().unwrap());
        assert_eq!(database.count_sync_outbox().unwrap(), 1);
    }

    #[test]
    fn remote_publication_state_acknowledges_only_after_snapshot_or_segment_commit() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let first = database
            .get_or_create_sync_remote_state(REMOTE_SCOPE)
            .unwrap();
        assert!(!first.initialized);
        assert!(!first.remote_prepared);
        assert_eq!(
            database
                .get_or_create_sync_remote_state(REMOTE_SCOPE)
                .unwrap()
                .epoch,
            first.epoch
        );
        database.mark_sync_remote_prepared(REMOTE_SCOPE).unwrap();

        database
            .save_item(&item("snapshot", "hash-snapshot", "snapshot"))
            .unwrap();
        let snapshot = ObjectRef {
            key: "v1/snapshots/device/epoch/hash.pack".to_string(),
            sha256: "b".repeat(64),
            stored_size_bytes: 123,
            record_count: 1,
        };
        let published = database
            .commit_sync_bootstrap_published(REMOTE_SCOPE, &first.epoch, &snapshot, 1)
            .unwrap();
        assert!(published.initialized);
        assert!(published.remote_prepared);
        assert_eq!(published.snapshot.as_ref(), Some(&snapshot));
        assert_eq!(database.count_sync_outbox().unwrap(), 0);

        database
            .save_item(&item("segment", "hash-segment", "segment"))
            .unwrap();
        let segment_key = "v1/segments/device/epoch/segment.pack";
        let published = database
            .commit_sync_segment_published(REMOTE_SCOPE, &first.epoch, segment_key, 2)
            .unwrap();
        assert_eq!(published.published_sequence, 2);
        assert_eq!(published.last_segment_key.as_deref(), Some(segment_key));
        assert_eq!(database.count_sync_outbox().unwrap(), 0);

        let reset = database.reset_sync_remote_state(REMOTE_SCOPE).unwrap();
        assert_ne!(reset.epoch, first.epoch);
        assert!(reset.remote_prepared);
        assert!(!reset.initialized);
        assert!(reset.snapshot.is_none());
    }

    #[test]
    fn remote_snapshot_apply_is_atomic_idempotent_and_does_not_echo() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let writer = RecordVersion {
            modified_at_ms: 500,
            writer_device_id: REMOTE_DEVICE.to_string(),
        };
        let mut remote = replicated("remote", "hash-remote", "remote", writer);
        remote.item.metadata_json = Some(r#"{"tags":["shared","work"]}"#.to_string());
        let mutations = MutationBatch {
            upserts: vec![remote],
            tombstones: Vec::new(),
        };

        assert_eq!(
            database
                .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"c".repeat(64), &mutations)
                .unwrap(),
            1
        );
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
        assert_eq!(
            database.get_item("remote").unwrap().unwrap().title,
            "remote"
        );
        let tags = database.list_all_tags().unwrap();
        assert_eq!(
            tags.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>(),
            vec!["shared", "work"]
        );
        assert_eq!(
            database
                .get_sync_cursor(REMOTE_SCOPE, REMOTE_DEVICE)
                .unwrap()
                .unwrap(),
            cursor(0, None)
        );
        assert_eq!(
            database
                .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"c".repeat(64), &mutations)
                .unwrap(),
            0
        );
    }

    #[test]
    fn record_writer_breaks_equal_timestamp_ties_deterministically() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let lower_writer = "00000000-0000-4000-8000-000000000001";
        let higher_writer = "ffffffff-ffff-4fff-bfff-ffffffffffff";
        let initial = MutationBatch {
            upserts: vec![replicated(
                "tie",
                "hash-tie",
                "lower",
                RecordVersion {
                    modified_at_ms: 100,
                    writer_device_id: lower_writer.to_string(),
                },
            )],
            tombstones: Vec::new(),
        };
        database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"d".repeat(64), &initial)
            .unwrap();

        let winner = MutationBatch {
            upserts: vec![replicated(
                "tie",
                "hash-tie-winner",
                "higher",
                RecordVersion {
                    modified_at_ms: 100,
                    writer_device_id: higher_writer.to_string(),
                },
            )],
            tombstones: Vec::new(),
        };
        database
            .apply_sync_segment(REMOTE_SCOPE, &cursor(1, Some("segment-1")), &winner)
            .unwrap();
        assert_eq!(database.get_item("tie").unwrap().unwrap().title, "higher");

        let loser = MutationBatch {
            upserts: vec![replicated(
                "tie",
                "hash-tie-loser",
                "lower-again",
                RecordVersion {
                    modified_at_ms: 100,
                    writer_device_id: lower_writer.to_string(),
                },
            )],
            tombstones: Vec::new(),
        };
        assert_eq!(
            database
                .apply_sync_segment(REMOTE_SCOPE, &cursor(2, Some("segment-2")), &loser,)
                .unwrap(),
            0
        );
        assert_eq!(database.get_item("tie").unwrap().unwrap().title, "higher");
    }

    #[test]
    fn checkpoint_apply_updates_mutations_cursors_and_generation_atomically() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let second_device = "33333333-3333-4333-8333-333333333333";
        let second_epoch = "44444444-4444-4444-8444-444444444444";
        let first_key =
            crate::sync::v1::segment_object_key(REMOTE_DEVICE, REMOTE_EPOCH, 1, 2, &"a".repeat(64))
                .unwrap();
        let cursors = vec![
            cursor(2, Some(&first_key)),
            DeviceCursor {
                device_id: second_device.to_string(),
                epoch: second_epoch.to_string(),
                sequence: 0,
                last_segment_key: None,
            },
        ];
        let mutations = MutationBatch {
            upserts: vec![replicated(
                "checkpoint-item",
                "hash-checkpoint",
                "checkpoint",
                RecordVersion {
                    modified_at_ms: 900,
                    writer_device_id: REMOTE_DEVICE.to_string(),
                },
            )],
            tombstones: Vec::new(),
        };

        assert_eq!(
            database
                .apply_sync_checkpoint(REMOTE_SCOPE, 1, &"b".repeat(64), &cursors, &mutations)
                .unwrap(),
            1
        );
        assert!(database.get_item("checkpoint-item").unwrap().is_some());
        assert_eq!(database.list_sync_cursors(REMOTE_SCOPE).unwrap(), cursors);
        assert_eq!(
            database.get_sync_checkpoint_cursors(REMOTE_SCOPE).unwrap(),
            cursors
        );
        assert_eq!(
            database.get_sync_checkpoint_state(REMOTE_SCOPE).unwrap(),
            Some((1, "b".repeat(64)))
        );
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
        assert_eq!(
            database
                .apply_sync_checkpoint(REMOTE_SCOPE, 1, &"b".repeat(64), &cursors, &mutations)
                .unwrap(),
            0
        );
    }

    #[test]
    fn invalid_checkpoint_rolls_back_rows_cursors_and_generation() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let existing = cursor(1, None);
        let initial = MutationBatch {
            upserts: vec![replicated(
                "existing-checkpoint",
                "hash-existing-checkpoint",
                "existing",
                RecordVersion {
                    modified_at_ms: 500,
                    writer_device_id: REMOTE_DEVICE.to_string(),
                },
            )],
            tombstones: Vec::new(),
        };
        database
            .apply_sync_checkpoint(
                REMOTE_SCOPE,
                1,
                &"c".repeat(64),
                std::slice::from_ref(&existing),
                &initial,
            )
            .unwrap();
        let invalid = MutationBatch {
            upserts: vec![ReplicatedItem {
                item: item("broken-checkpoint", "hash-broken-checkpoint", "broken"),
                version: RecordVersion {
                    modified_at_ms: 600,
                    writer_device_id: "not-a-uuid".to_string(),
                },
            }],
            tombstones: Vec::new(),
        };

        assert!(database
            .apply_sync_checkpoint(
                REMOTE_SCOPE,
                2,
                &"d".repeat(64),
                &[DeviceCursor {
                    device_id: "33333333-3333-4333-8333-333333333333".to_string(),
                    epoch: "44444444-4444-4444-8444-444444444444".to_string(),
                    sequence: 0,
                    last_segment_key: None,
                }],
                &invalid,
            )
            .is_err());
        assert!(database.get_item("broken-checkpoint").unwrap().is_none());
        assert_eq!(
            database.list_sync_cursors(REMOTE_SCOPE).unwrap(),
            vec![existing.clone()]
        );
        assert_eq!(
            database.get_sync_checkpoint_cursors(REMOTE_SCOPE).unwrap(),
            vec![existing]
        );
        assert_eq!(
            database.get_sync_checkpoint_state(REMOTE_SCOPE).unwrap(),
            Some((1, "c".repeat(64)))
        );
    }

    #[test]
    fn tombstone_blocks_an_older_later_segment_from_resurrecting_a_row() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let initial = MutationBatch {
            upserts: vec![replicated(
                "victim",
                "hash-victim",
                "live",
                RecordVersion {
                    modified_at_ms: 100,
                    writer_device_id: REMOTE_DEVICE.to_string(),
                },
            )],
            tombstones: Vec::new(),
        };
        database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"e".repeat(64), &initial)
            .unwrap();
        let deletion = MutationBatch {
            upserts: Vec::new(),
            tombstones: vec![Tombstone {
                item_id: "victim".to_string(),
                kind: ClipboardKind::Text,
                content_hash: "hash-victim".to_string(),
                deleted_at_ms: 200,
                version: RecordVersion {
                    modified_at_ms: 200,
                    writer_device_id: REMOTE_DEVICE.to_string(),
                },
            }],
        };
        database
            .apply_sync_segment(REMOTE_SCOPE, &cursor(1, Some("segment-delete")), &deletion)
            .unwrap();
        let stale = MutationBatch {
            upserts: vec![replicated(
                "victim",
                "hash-victim",
                "stale",
                RecordVersion {
                    modified_at_ms: 150,
                    writer_device_id: REMOTE_DEVICE.to_string(),
                },
            )],
            tombstones: Vec::new(),
        };
        assert_eq!(
            database
                .apply_sync_segment(REMOTE_SCOPE, &cursor(2, Some("segment-stale")), &stale,)
                .unwrap(),
            0
        );
        assert!(database
            .export_sync_snapshot()
            .unwrap()
            .mutations
            .upserts
            .is_empty());
        assert_eq!(
            database
                .export_sync_snapshot()
                .unwrap()
                .mutations
                .tombstones
                .len(),
            1
        );
    }

    #[test]
    fn failed_remote_batch_rolls_back_rows_suppression_and_cursor() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let mut image = item("local-image", "same-image-hash", "image");
        image.kind = ClipboardKind::Image;
        database.save_item(&image).unwrap();
        database.acknowledge_sync_outbox(1).unwrap();

        let first = replicated(
            "would-be-inserted",
            "unique-text-hash",
            "first",
            RecordVersion {
                modified_at_ms: 300,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        );
        let mut collision = replicated(
            "different-image-id",
            "same-image-hash",
            "collision",
            RecordVersion {
                modified_at_ms: 300,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        );
        collision.item.kind = ClipboardKind::Image;
        let failed = MutationBatch {
            upserts: vec![first, collision],
            tombstones: Vec::new(),
        };
        assert!(database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"f".repeat(64), &failed)
            .is_err());
        assert!(database.get_item("would-be-inserted").unwrap().is_none());
        assert!(database
            .get_sync_cursor(REMOTE_SCOPE, REMOTE_DEVICE)
            .unwrap()
            .is_none());

        database
            .save_item(&item("after-failure", "hash-after", "after"))
            .unwrap();
        assert_eq!(database.count_sync_outbox().unwrap(), 1);
    }
}
