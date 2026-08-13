use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, params_from_iter, OptionalExtension, Row, Transaction};
use uuid::Uuid;

use super::{Database, StorageError};
use crate::sync::v1::{
    checkpoint_object_key, parse_segment_key, snapshot_object_key, DeviceCursor, DeviceHead,
    MutationBatch, ObjectRef, RecordVersion, ReplicatedItem, SyncHeadCache, SyncItem, SyncItemKind,
    SyncOutboxBatch, SyncRemoteState, SyncResourceRef, SyncSnapshot, SyncSnapshotExport, Tombstone,
};

const LOOKUP_CHUNK_SIZE: usize = 500;
const SYNC_HEAD_CACHE_PREFIX: &str = "sync_head_cache:";

struct StoredReplicatedItem {
    id: String,
    kind: String,
    title: String,
    text_content: Option<String>,
    html_content: Option<String>,
    rtf_content: Option<String>,
    resource_path: Option<String>,
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
            content_hash: row.get(7)?,
            source_app: row.get(8)?,
            icon_path: row.get(9)?,
            size_bytes: row.get(10)?,
            created_at_ms: row.get(11)?,
            is_favorite: row.get(12)?,
            metadata_json: row.get(13)?,
            modified_at_ms: row.get(14)?,
            writer_device_id: row.get(15)?,
        })
    }

    fn into_wire(self) -> Result<ReplicatedItem, StorageError> {
        let size_bytes =
            u64::try_from(self.size_bytes).map_err(|_| StorageError::InvalidStoredValue {
                field: "clipboard_items.size_bytes",
                value: self.size_bytes,
            })?;
        Ok(ReplicatedItem {
            item: SyncItem {
                id: self.id,
                kind: kind_from_storage(&self.kind)?,
                title: self.title,
                text_content: self.text_content,
                html_content: self.html_content,
                rtf_content: self.rtf_content,
                resource_path: self.resource_path,
                preview_path: None,
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
                 DELETE FROM sync_item_resources;
                 DELETE FROM sync_outbox;
                 DELETE FROM sync_tombstones;
                 DELETE FROM sync_publication_state;
                 DELETE FROM sync_cursors;
                 DELETE FROM sync_checkpoint_cursors;
                 DELETE FROM sync_checkpoint_state;
                 DELETE FROM sync_resource_scopes;
                 DELETE FROM sqlite_sequence WHERE name = 'sync_outbox';",
            )?;
            transaction.execute(
                "DELETE FROM sync_metadata
                  WHERE substr(key, 1, length(?1)) = ?1",
                [SYNC_HEAD_CACHE_PREFIX],
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
            let mutations = load_mutations(connection, None, None)?;
            Ok(SyncSnapshot {
                through_sequence,
                mutations,
            })
        })
    }

    pub fn export_sync_snapshot_for_scope(
        &self,
        remote_scope: &str,
    ) -> Result<SyncSnapshot, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let through_sequence = current_sequence(connection)?;
            let mutations = load_mutations(connection, Some(remote_scope), None)?;
            Ok(SyncSnapshot {
                through_sequence,
                mutations,
            })
        })
    }

    /// Visits a deterministic, point-in-time snapshot in bounded mutation
    /// batches. The SQLite read transaction stays open only while rows are
    /// exported; callers must not perform network I/O from `visit`.
    pub fn visit_sync_snapshot_for_scope(
        &self,
        remote_scope: &str,
        batch_size: usize,
        mut visit: impl FnMut(MutationBatch) -> Result<(), StorageError>,
    ) -> Result<SyncSnapshotExport, StorageError> {
        validate_remote_scope(remote_scope)?;
        if batch_size == 0 {
            return Err(StorageError::InvalidSyncState(
                "sync snapshot export batch size must be greater than zero".to_string(),
            ));
        }
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let through_sequence = current_sequence(&transaction)?;
            let mut record_count = 0u64;
            let mut after_item_id = None::<String>;
            loop {
                let batch = load_snapshot_upsert_batch(
                    &transaction,
                    remote_scope,
                    after_item_id.as_deref(),
                    batch_size,
                )?;
                if batch.is_empty() {
                    break;
                }
                after_item_id = batch.upserts.last().map(|item| item.item.id.clone());
                record_count = record_count.checked_add(batch.len() as u64).ok_or(
                    StorageError::ValueOutOfRange {
                        field: "sync snapshot record count",
                    },
                )?;
                visit(batch)?;
            }

            let mut after_tombstone_id = None::<String>;
            loop {
                let batch = load_snapshot_tombstone_batch(
                    &transaction,
                    after_tombstone_id.as_deref(),
                    batch_size,
                )?;
                if batch.is_empty() {
                    break;
                }
                after_tombstone_id = batch
                    .tombstones
                    .last()
                    .map(|tombstone| tombstone.item_id.clone());
                record_count = record_count.checked_add(batch.len() as u64).ok_or(
                    StorageError::ValueOutOfRange {
                        field: "sync snapshot record count",
                    },
                )?;
                visit(batch)?;
            }

            transaction.commit()?;
            Ok(SyncSnapshotExport {
                through_sequence,
                record_count,
            })
        })
    }

    pub fn get_sync_outbox_batch(
        &self,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, StorageError> {
        self.get_sync_outbox_batch_inner(None, limit)
    }

    pub fn get_sync_outbox_batch_for_scope(
        &self,
        remote_scope: &str,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.get_sync_outbox_batch_inner(Some(remote_scope), limit)
    }

    fn get_sync_outbox_batch_inner(
        &self,
        remote_scope: Option<&str>,
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
            let mutations = load_mutations(connection, remote_scope, Some(&item_ids))?;
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

    pub fn record_sync_resource_refs(
        &self,
        remote_scope: &str,
        mutations: &MutationBatch,
        resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
    ) -> Result<(), StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            for replicated in &mutations.upserts {
                let current = transaction
                    .query_row(
                        "SELECT COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
                           FROM clipboard_items
                          WHERE id = ?1 AND deleted = 0",
                        [&replicated.item.id],
                        |row| {
                            Ok(RecordVersion {
                                modified_at_ms: row.get(0)?,
                                writer_device_id: row.get(1)?,
                            })
                        },
                    )
                    .optional()?;
                if current.as_ref() == Some(&replicated.version) {
                    replace_sync_resource_refs(
                        &transaction,
                        remote_scope,
                        &replicated.item.id,
                        resource_refs
                            .get(&replicated.item.id)
                            .map(Vec::as_slice)
                            .unwrap_or_default(),
                    )?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Returns the canonical remote resource references for one item in the
    /// configured scope. Local paths are deliberately not inferred here: a
    /// missing path means the caller must materialize the returned keys first.
    pub fn get_sync_resource_refs(
        &self,
        remote_scope: &str,
        item_id: &str,
    ) -> Result<Vec<SyncResourceRef>, StorageError> {
        validate_remote_scope(remote_scope)?;
        if item_id.is_empty() {
            return Ok(Vec::new());
        }
        self.with_connection(|connection| {
            load_sync_resource_refs(connection, remote_scope, item_id)
        })
    }

    /// Records a successfully materialized path while retaining the canonical
    /// reference. The reference is the stable content identity used to keep a
    /// later remote metadata update attached to the local cache; local path
    /// changes or deletion clear it through the resource trigger. This is a
    /// local-only cache update: it must not alter the replicated version or
    /// enqueue an outbox mutation.
    pub fn mark_sync_resource_materialized(
        &self,
        remote_scope: &str,
        item_id: &str,
        reference: &SyncResourceRef,
        local_path: &str,
    ) -> Result<bool, StorageError> {
        self.mark_sync_resources_materialized(
            remote_scope,
            item_id,
            &[(reference.clone(), local_path.to_string())],
        )
    }

    /// Atomically writes all paths materialized for one item while retaining
    /// their canonical remote references. This is deliberately a local cache
    /// update: replicated versions and the sync outbox remain unchanged.
    pub fn mark_sync_resources_materialized(
        &self,
        remote_scope: &str,
        item_id: &str,
        materialized: &[(SyncResourceRef, String)],
    ) -> Result<bool, StorageError> {
        validate_remote_scope(remote_scope)?;
        if item_id.is_empty() || materialized.is_empty() {
            return Ok(false);
        }
        for (reference, local_path) in materialized {
            if local_path.trim().is_empty() {
                return Ok(false);
            }
            crate::sync::v1::parse_resource_key(&reference.object_key)
                .map_err(StorageError::InvalidSyncState)?;
        }
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let Some(scope_id) = sync_resource_scope_id(&transaction, remote_scope)? else {
                transaction.commit()?;
                return Ok(false);
            };
            let Some(item_kind) = transaction
                .query_row(
                    "SELECT kind FROM clipboard_items WHERE id = ?1 AND deleted = 0",
                    [item_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            else {
                transaction.commit()?;
                return Ok(false);
            };
            let mut stored_refs = Vec::with_capacity(materialized.len());
            for (reference, local_path) in materialized {
                let slot = resource_slot_to_i64(&reference.slot)?;
                let ordinal = sequence_to_i64(u64::from(reference.ordinal), "resource ordinal")?;
                let parsed = crate::sync::v1::parse_resource_key(&reference.object_key)
                    .map_err(StorageError::InvalidSyncState)?;
                let digest = hex::decode(parsed.sha256).map_err(|error| {
                    StorageError::InvalidSyncState(format!(
                        "sync resource digest is not hexadecimal: {error}"
                    ))
                })?;
                let stored: Option<()> = transaction
                    .query_row(
                        "SELECT 1
                           FROM sync_item_resources
                          WHERE scope_id = ?1 AND item_id = ?2 AND slot = ?3 AND ordinal = ?4
                            AND sha256 = ?5",
                        params![scope_id, item_id, slot, ordinal, digest],
                        |_row| Ok(()),
                    )
                    .optional()?;
                if stored.is_none() {
                    transaction.commit()?;
                    return Ok(false);
                }
                if (reference.slot == "image" && item_kind != "image")
                    || (reference.slot == "file" && item_kind != "file")
                    || (reference.slot != "image"
                        && reference.slot != "file"
                        && reference.slot != "icon")
                    || (reference.slot == "icon"
                        && parsed.category != crate::sync::v1::ResourceCategory::Icon)
                {
                    transaction.commit()?;
                    return Ok(false);
                }
                let stored_path = if reference.slot == "icon" {
                    std::path::Path::new(local_path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| {
                            StorageError::InvalidSyncState(
                                "materialized icon path has no portable file name".to_string(),
                            )
                        })?
                        .to_string()
                } else {
                    local_path.clone()
                };
                stored_refs.push((reference.clone(), stored_path));
            }

            let (mut resource_path, mut icon_path, mut text_content, metadata_json): (
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
            ) = transaction.query_row(
                "SELECT resource_path, icon_path, text_content, metadata_json
                   FROM clipboard_items WHERE id = ?1 AND deleted = 0",
                [item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            let mut metadata = metadata_json
                .as_deref()
                .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            let mut file_paths = if item_kind == "file" {
                match text_content.as_deref() {
                    Some(json) => {
                        Some(serde_json::from_str::<Vec<String>>(json).map_err(|error| {
                            StorageError::InvalidSyncState(format!(
                                "stored file resource paths are invalid JSON: {error}"
                            ))
                        })?)
                    }
                    None => None,
                }
            } else {
                None
            };
            let mut changed = false;
            set_changelog_suppressed(&transaction, true)?;
            for (reference, stored_path) in &stored_refs {
                match reference.slot.as_str() {
                    "image" => {
                        changed |= resource_path.as_deref() != Some(stored_path.as_str());
                        resource_path = Some(stored_path.clone());
                        set_metadata_path(&mut metadata, "resourcePath", stored_path);
                        set_metadata_path(&mut metadata, "storagePath", stored_path);
                    }
                    "file" => {
                        let index = usize::try_from(reference.ordinal).map_err(|_| {
                            StorageError::ValueOutOfRange {
                                field: "sync resource ordinal",
                            }
                        })?;
                        if index == 0 {
                            changed |= resource_path.as_deref() != Some(stored_path.as_str());
                            resource_path = Some(stored_path.clone());
                            set_metadata_path(&mut metadata, "resourcePath", stored_path);
                        }
                        if let Some(paths) = file_paths.as_mut() {
                            if paths.len() <= index {
                                paths.resize(index + 1, String::new());
                            }
                            changed |= paths[index] != *stored_path;
                            paths[index] = stored_path.clone();
                        } else if index > 0 {
                            set_changelog_suppressed(&transaction, false)?;
                            transaction.commit()?;
                            return Ok(false);
                        }
                        set_file_metadata_path(&mut metadata, index, stored_path);
                    }
                    "icon" => {
                        changed |= icon_path.as_deref() != Some(stored_path.as_str());
                        icon_path = Some(stored_path.clone());
                        set_metadata_path(&mut metadata, "iconPath", stored_path);
                    }
                    _ => unreachable!(),
                }
            }
            let encoded_metadata = serde_json::to_string(&metadata).map_err(|error| {
                StorageError::InvalidSyncState(format!(
                    "failed to encode materialized resource metadata: {error}"
                ))
            })?;
            if let Some(paths) = file_paths {
                text_content = Some(serde_json::to_string(&paths).map_err(|error| {
                    StorageError::InvalidSyncState(format!(
                        "failed to encode materialized file paths: {error}"
                    ))
                })?);
            }
            if metadata_json.as_deref() != Some(encoded_metadata.as_str()) {
                changed = true;
            }
            if !changed {
                set_changelog_suppressed(&transaction, false)?;
                transaction.commit()?;
                return Ok(false);
            }
            transaction.execute(
                "UPDATE clipboard_items
                    SET resource_path = ?2,
                        text_content = ?3,
                        icon_path = ?4,
                        metadata_json = ?5
                  WHERE id = ?1 AND deleted = 0",
                params![
                    item_id,
                    resource_path,
                    text_content,
                    icon_path,
                    encoded_metadata
                ],
            )?;
            set_changelog_suppressed(&transaction, false)?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub fn materialized_sync_resource_path(
        &self,
        remote_scope: &str,
        item_id: &str,
        reference: &SyncResourceRef,
    ) -> Result<Option<String>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let Some(scope_id) = sync_resource_scope_id(connection, remote_scope)? else {
                return Ok(None);
            };
            let slot = resource_slot_to_i64(&reference.slot)?;
            let ordinal = sequence_to_i64(u64::from(reference.ordinal), "resource ordinal")?;
            let parsed = crate::sync::v1::parse_resource_key(&reference.object_key)
                .map_err(StorageError::InvalidSyncState)?;
            let digest = hex::decode(parsed.sha256).map_err(|error| {
                StorageError::InvalidSyncState(format!(
                    "sync resource digest is not hexadecimal: {error}"
                ))
            })?;
            let exists: bool = connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sync_item_resources
                     WHERE scope_id = ?1 AND item_id = ?2 AND slot = ?3 AND ordinal = ?4
                       AND sha256 = ?5
                )",
                params![scope_id, item_id, slot, ordinal, digest],
                |row| row.get(0),
            )?;
            if !exists {
                return Ok(None);
            }
            let (kind, resource_path, icon_path, text_content): (
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            ) = connection.query_row(
                "SELECT kind, resource_path, icon_path, text_content
                   FROM clipboard_items WHERE id = ?1 AND deleted = 0",
                [item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            match reference.slot.as_str() {
                "image" if kind == "image" && reference.ordinal == 0 => Ok(resource_path),
                "icon" if reference.ordinal == 0 => Ok(icon_path),
                "file" if kind == "file" => {
                    let index = usize::try_from(reference.ordinal).map_err(|_| {
                        StorageError::ValueOutOfRange {
                            field: "sync resource ordinal",
                        }
                    })?;
                    let listed_path = text_content
                        .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
                        .and_then(|paths| paths.get(index).cloned())
                        .filter(|path| !path.is_empty());
                    Ok(listed_path.or_else(|| (index == 0).then_some(resource_path).flatten()))
                }
                _ => Ok(None),
            }
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

    pub(crate) fn get_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<SyncHeadCache>, StorageError> {
        let key = sync_head_cache_key(remote_scope, device_id)?;
        self.with_connection(|connection| {
            let value = connection
                .query_row(
                    "SELECT value FROM sync_metadata WHERE key = ?1",
                    [&key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok(value
                .and_then(|value| serde_json::from_str::<SyncHeadCache>(&value).ok())
                .filter(|cache| valid_sync_head_cache(cache, device_id)))
        })
    }

    pub(crate) fn record_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
        etag: &str,
        stored_size_bytes: u64,
        modified_ms: Option<i64>,
        head: &DeviceHead,
    ) -> Result<(), StorageError> {
        let key = sync_head_cache_key(remote_scope, device_id)?;
        if head.device_id != device_id || !valid_head_etag(etag) {
            return Err(StorageError::InvalidSyncState(
                "sync head cache identity or ETag is invalid".to_string(),
            ));
        }
        let cache = SyncHeadCache {
            etag: etag.to_string(),
            stored_size_bytes,
            modified_ms,
            epoch: head.epoch.clone(),
            snapshot_key: head.snapshot.key.clone(),
            snapshot_sha256: head.snapshot.sha256.clone(),
            snapshot_size_bytes: head.snapshot.stored_size_bytes,
            snapshot_record_count: head.snapshot.record_count,
            published_sequence: head.published_sequence,
            last_segment_key: head.last_segment_key.clone(),
        };
        if !valid_sync_head_cache(&cache, device_id) {
            return Err(StorageError::InvalidSyncState(
                "sync head cache payload is invalid".to_string(),
            ));
        }
        let value = serde_json::to_string(&cache)?;
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
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
        self.apply_sync_checkpoint_with_resources(
            remote_scope,
            generation,
            checkpoint_sha256,
            cursors,
            mutations,
            &BTreeMap::new(),
        )
    }

    pub fn apply_sync_checkpoint_with_resources(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        mutations: &MutationBatch,
        resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
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
            let applied = apply_mutations(&transaction, remote_scope, mutations, resource_refs)?;
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

    pub fn apply_sync_checkpoint_batches<I>(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        batches: I,
    ) -> Result<u64, StorageError>
    where
        I: IntoIterator<
            Item = Result<(MutationBatch, BTreeMap<String, Vec<SyncResourceRef>>), StorageError>,
        >,
    {
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
            let mut applied = 0u64;
            for batch in batches {
                let (mutations, resource_refs) = batch?;
                applied = applied
                    .checked_add(apply_mutations(
                        &transaction,
                        remote_scope,
                        &mutations,
                        &resource_refs,
                    )?)
                    .ok_or(StorageError::ValueOutOfRange {
                        field: "applied sync mutation count",
                    })?;
            }
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
        self.apply_sync_snapshot_with_resources(
            remote_scope,
            cursor,
            snapshot_sha256,
            mutations,
            &BTreeMap::new(),
        )
    }

    pub fn apply_sync_snapshot_with_resources(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        mutations: &MutationBatch,
        resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
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
            let applied = apply_mutations(&transaction, remote_scope, mutations, resource_refs)?;
            set_changelog_suppressed(&transaction, false)?;
            upsert_cursor(&transaction, remote_scope, cursor, Some(snapshot_sha256))?;
            transaction.commit()?;
            Ok(applied)
        })
    }

    pub fn apply_sync_snapshot_batches<I>(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        batches: I,
    ) -> Result<u64, StorageError>
    where
        I: IntoIterator<
            Item = Result<(MutationBatch, BTreeMap<String, Vec<SyncResourceRef>>), StorageError>,
        >,
    {
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
            let mut applied = 0u64;
            for batch in batches {
                let (mutations, resource_refs) = batch?;
                applied = applied
                    .checked_add(apply_mutations(
                        &transaction,
                        remote_scope,
                        &mutations,
                        &resource_refs,
                    )?)
                    .ok_or(StorageError::ValueOutOfRange {
                        field: "applied sync mutation count",
                    })?;
            }
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
        self.apply_sync_segment_with_resources(remote_scope, cursor, mutations, &BTreeMap::new())
    }

    pub fn apply_sync_segment_with_resources(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        mutations: &MutationBatch,
        resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
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
            let applied = apply_mutations(&transaction, remote_scope, mutations, resource_refs)?;
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

fn sync_head_cache_key(remote_scope: &str, device_id: &str) -> Result<String, StorageError> {
    validate_remote_scope(remote_scope)?;
    let parsed = Uuid::parse_str(device_id).map_err(|_| {
        StorageError::InvalidSyncState("sync head cache device is not a UUID".into())
    })?;
    if parsed.to_string() != device_id {
        return Err(StorageError::InvalidSyncState(
            "sync head cache device is not canonical lowercase".to_string(),
        ));
    }
    Ok(format!(
        "{SYNC_HEAD_CACHE_PREFIX}{remote_scope}:{device_id}"
    ))
}

fn valid_head_etag(etag: &str) -> bool {
    !etag.is_empty() && etag.len() <= 256 && !etag.bytes().any(|byte| byte.is_ascii_control())
}

fn valid_sync_head_cache(cache: &SyncHeadCache, device_id: &str) -> bool {
    if !valid_head_etag(&cache.etag)
        || cache.stored_size_bytes == 0
        || cache.snapshot_size_bytes == 0
        || Uuid::parse_str(&cache.epoch).is_err()
        || Uuid::parse_str(&cache.epoch).is_ok_and(|epoch| epoch.to_string() != cache.epoch)
        || snapshot_object_key(device_id, &cache.epoch, &cache.snapshot_sha256)
            .ok()
            .as_deref()
            != Some(cache.snapshot_key.as_str())
    {
        return false;
    }
    match cache.last_segment_key.as_deref() {
        None => true,
        Some(key) => parse_segment_key(key).is_ok_and(|parsed| {
            parsed.device_id == device_id
                && parsed.epoch == cache.epoch
                && parsed.last_sequence == cache.published_sequence
        }),
    }
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
    remote_scope: &str,
    mutations: &MutationBatch,
    resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
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
        let refs = resource_refs
            .get(&replicated.item.id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let old_refs = load_sync_resource_refs(transaction, remote_scope, &target_id)?;
        let content_slot = match replicated.item.kind {
            SyncItemKind::Image => "image",
            SyncItemKind::File => "file",
            SyncItemKind::Text | SyncItemKind::Link => "",
        };
        let preserve_local_resource = exists
            && !content_slot.is_empty()
            && resource_slot_matches(&old_refs, refs, content_slot)
            && replicated.item.resource_path.is_none();
        let preserve_local_icon = exists
            && resource_slot_matches(&old_refs, refs, "icon")
            && replicated.item.icon_path.is_none();
        let local_paths = exists.then(|| {
            transaction.query_row(
                "SELECT resource_path, preview_path, icon_path, text_content, metadata_json
                   FROM clipboard_items
                  WHERE id = ?1",
                [&target_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
        });
        let (local_resource, local_preview, local_icon, local_text, local_metadata) =
            match local_paths {
                Some(row) => row?,
                None => (None, None, None, None, None),
            };
        let resource_path = if preserve_local_resource {
            local_resource.as_ref()
        } else {
            replicated.item.resource_path.as_ref()
        };
        let preview_path = if preserve_local_resource {
            local_preview.as_ref()
        } else {
            replicated.item.preview_path.as_ref()
        };
        let icon_path = if preserve_local_icon {
            local_icon.as_ref()
        } else {
            replicated.item.icon_path.as_ref()
        };
        let text_content = if preserve_local_resource && replicated.item.kind == SyncItemKind::File
        {
            local_text.as_ref()
        } else {
            replicated.item.text_content.as_ref()
        };
        let metadata_json = if preserve_local_resource || preserve_local_icon {
            merge_local_resource_metadata(
                replicated.item.metadata_json.as_deref(),
                local_metadata.as_deref(),
                preserve_local_resource,
                preserve_local_icon,
            )?
        } else {
            replicated.item.metadata_json.clone()
        };
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
                    text_content,
                    &replicated.item.html_content,
                    &replicated.item.rtf_content,
                    resource_path,
                    preview_path,
                    &replicated.item.content_hash,
                    &replicated.item.source_app,
                    icon_path,
                    size_bytes,
                    replicated.item.created_at_ms,
                    replicated.item.is_favorite,
                    &metadata_json,
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
        replace_item_tags(transaction, &target_id, metadata_json.as_deref())?;
        replace_sync_resource_refs(transaction, remote_scope, &target_id, refs)?;
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
        transaction.execute(
            "DELETE FROM sync_item_resources WHERE item_id = ?1",
            [&target_id],
        )?;
        applied += 1;
    }
    Ok(applied)
}

fn load_sync_resource_refs(
    connection: &rusqlite::Connection,
    remote_scope: &str,
    item_id: &str,
) -> Result<Vec<SyncResourceRef>, StorageError> {
    let Some(scope_id) = sync_resource_scope_id(connection, remote_scope)? else {
        return Ok(Vec::new());
    };
    let mut statement = connection.prepare(
        "SELECT slot, ordinal, sha256, extension
           FROM sync_item_resources
          WHERE scope_id = ?1 AND item_id = ?2
          ORDER BY slot, ordinal",
    )?;
    let references = statement
        .query_map(params![scope_id, item_id], |row| {
            let ordinal = row.get::<_, i64>(1)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, ordinal))?;
            let slot = resource_slot_from_i64(row.get(0)?)?;
            let sha256 = row.get::<_, Vec<u8>>(2)?;
            let extension = row.get::<_, String>(3)?;
            sync_resource_ref_from_parts(slot, ordinal, &sha256, &extension)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(references)
}

fn resource_slot_matches(
    old_refs: &[SyncResourceRef],
    new_refs: &[SyncResourceRef],
    slot: &str,
) -> bool {
    let old = old_refs
        .iter()
        .filter(|reference| reference.slot == slot)
        .collect::<Vec<_>>();
    let new = new_refs
        .iter()
        .filter(|reference| reference.slot == slot)
        .collect::<Vec<_>>();
    old == new
}

fn merge_local_resource_metadata(
    remote_json: Option<&str>,
    local_json: Option<&str>,
    preserve_resource: bool,
    preserve_icon: bool,
) -> Result<Option<String>, StorageError> {
    let mut remote = remote_json
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
    let Some(local) =
        local_json.and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
    else {
        return Ok(remote_json.map(str::to_string));
    };
    if preserve_resource {
        copy_json_key(&local, &mut remote, "resourcePath");
        copy_json_key(&local, &mut remote, "storagePath");
        copy_json_key(&local, &mut remote, "previewPath");
        copy_file_storage_paths(&local, &mut remote);
    }
    if preserve_icon {
        copy_json_key(&local, &mut remote, "iconPath");
    }
    serde_json::to_string(&remote).map(Some).map_err(|error| {
        StorageError::InvalidSyncState(format!("failed to merge local resource metadata: {error}"))
    })
}

fn copy_json_key(source: &serde_json::Value, target: &mut serde_json::Value, key: &str) {
    let (Some(source), Some(target)) = (source.as_object(), target.as_object_mut()) else {
        return;
    };
    match source.get(key) {
        Some(value) => {
            target.insert(key.to_string(), value.clone());
        }
        None => {
            target.remove(key);
        }
    }
}

fn copy_file_storage_paths(source: &serde_json::Value, target: &mut serde_json::Value) {
    let Some(source_files) = source.get("files").and_then(serde_json::Value::as_array) else {
        return;
    };
    let Some(target_files) = target
        .get_mut("files")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (source, target) in source_files.iter().zip(target_files.iter_mut()) {
        copy_json_key(source, target, "storagePath");
        copy_json_key(source, target, "path");
    }
}

fn replace_sync_resource_refs(
    transaction: &Transaction<'_>,
    remote_scope: &str,
    item_id: &str,
    references: &[SyncResourceRef],
) -> Result<(), StorageError> {
    let scope_id = get_or_create_sync_resource_scope(transaction, remote_scope)?;
    transaction.execute(
        "DELETE FROM sync_item_resources
          WHERE scope_id = ?1 AND item_id = ?2",
        params![scope_id, item_id],
    )?;
    for reference in references {
        let parsed = crate::sync::v1::parse_resource_key(&reference.object_key)
            .map_err(StorageError::InvalidSyncState)?;
        let slot_is_valid = match reference.slot.as_str() {
            "image" => {
                reference.ordinal == 0
                    && parsed.category == crate::sync::v1::ResourceCategory::Image
            }
            "file" => parsed.category == crate::sync::v1::ResourceCategory::File,
            "icon" => {
                reference.ordinal == 0 && parsed.category == crate::sync::v1::ResourceCategory::Icon
            }
            _ => false,
        };
        if !slot_is_valid {
            return Err(StorageError::InvalidSyncState(
                "sync resource reference has an invalid slot/category".to_string(),
            ));
        }
        transaction.execute(
            "INSERT INTO sync_item_resources
                (scope_id, item_id, slot, ordinal, sha256, extension)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                scope_id,
                item_id,
                resource_slot_to_i64(&reference.slot)?,
                i64::from(reference.ordinal),
                hex::decode(&parsed.sha256).map_err(|error| {
                    StorageError::InvalidSyncState(format!(
                        "sync resource digest is not hexadecimal: {error}"
                    ))
                })?,
                &parsed.extension,
            ],
        )?;
    }
    Ok(())
}

fn sync_resource_scope_id(
    connection: &rusqlite::Connection,
    remote_scope: &str,
) -> Result<Option<i64>, StorageError> {
    Ok(connection
        .query_row(
            "SELECT id FROM sync_resource_scopes WHERE remote_scope = ?1",
            [remote_scope],
            |row| row.get(0),
        )
        .optional()?)
}

fn get_or_create_sync_resource_scope(
    transaction: &Transaction<'_>,
    remote_scope: &str,
) -> Result<i64, StorageError> {
    transaction.execute(
        "INSERT INTO sync_resource_scopes (remote_scope) VALUES (?1)
         ON CONFLICT(remote_scope) DO NOTHING",
        [remote_scope],
    )?;
    Ok(transaction.query_row(
        "SELECT id FROM sync_resource_scopes WHERE remote_scope = ?1",
        [remote_scope],
        |row| row.get(0),
    )?)
}

fn resource_slot_to_i64(slot: &str) -> Result<i64, StorageError> {
    match slot {
        "image" => Ok(0),
        "file" => Ok(1),
        "icon" => Ok(2),
        _ => Err(StorageError::InvalidSyncState(
            "sync resource reference has an unknown slot".to_string(),
        )),
    }
}

fn resource_slot_from_i64(slot: i64) -> rusqlite::Result<&'static str> {
    match slot {
        0 => Ok("image"),
        1 => Ok("file"),
        2 => Ok("icon"),
        _ => Err(rusqlite::Error::IntegralValueOutOfRange(0, slot)),
    }
}

fn sync_resource_ref_from_parts(
    slot: &str,
    ordinal: u32,
    sha256: &[u8],
    extension: &str,
) -> rusqlite::Result<SyncResourceRef> {
    if sha256.len() != 32 {
        return Err(rusqlite::Error::InvalidColumnType(
            2,
            "sha256".to_string(),
            rusqlite::types::Type::Blob,
        ));
    }
    let category = match slot {
        "image" => crate::sync::v1::ResourceCategory::Image,
        "file" => crate::sync::v1::ResourceCategory::File,
        "icon" => crate::sync::v1::ResourceCategory::Icon,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let object_key =
        crate::sync::v1::resource_object_key(category, &hex::encode(sha256), extension)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(SyncResourceRef {
        slot: slot.to_string(),
        ordinal,
        object_key,
    })
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

fn kind_to_storage(kind: SyncItemKind) -> &'static str {
    match kind {
        SyncItemKind::Text => "text",
        SyncItemKind::Link => "link",
        SyncItemKind::Image => "image",
        SyncItemKind::File => "file",
    }
}

fn kind_from_storage(kind: &str) -> Result<SyncItemKind, StorageError> {
    match kind {
        "text" => Ok(SyncItemKind::Text),
        "link" => Ok(SyncItemKind::Link),
        "image" => Ok(SyncItemKind::Image),
        "file" => Ok(SyncItemKind::File),
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

fn load_snapshot_upsert_batch(
    connection: &rusqlite::Connection,
    remote_scope: &str,
    after_item_id: Option<&str>,
    limit: usize,
) -> Result<MutationBatch, StorageError> {
    let limit = i64::try_from(limit).map_err(|_| StorageError::ValueOutOfRange {
        field: "sync snapshot export batch size",
    })?;
    let mut statement = connection.prepare(
        "SELECT id, kind, title, text_content, html_content, rtf_content,
                resource_path, content_hash, source_app,
                icon_path, size_bytes, created_at_ms,
                is_favorite, metadata_json,
                COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
           FROM clipboard_items
          WHERE deleted = 0
            AND (?1 IS NULL OR id > ?1)
          ORDER BY id
          LIMIT ?2",
    )?;
    let stored_items = statement
        .query_map(
            params![after_item_id, limit],
            StoredReplicatedItem::from_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let item_ids = stored_items
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let mut resource_refs = load_sync_resource_ref_map(connection, remote_scope, Some(&item_ids))?;
    let mut upserts = Vec::with_capacity(stored_items.len());
    for stored in stored_items {
        let mut item = stored.into_wire()?;
        if let Some(references) = resource_refs.remove(&item.item.id) {
            restore_sync_resource_refs(&mut item, &references)?;
        }
        upserts.push(item);
    }
    Ok(MutationBatch {
        upserts,
        tombstones: Vec::new(),
    })
}

fn load_snapshot_tombstone_batch(
    connection: &rusqlite::Connection,
    after_item_id: Option<&str>,
    limit: usize,
) -> Result<MutationBatch, StorageError> {
    let limit = i64::try_from(limit).map_err(|_| StorageError::ValueOutOfRange {
        field: "sync snapshot export batch size",
    })?;
    let mut statement = connection.prepare(
        "SELECT item_id, kind, content_hash, deleted_at_ms,
                modified_at_ms, writer_device_id
           FROM sync_tombstones
          WHERE (?1 IS NULL OR item_id > ?1)
          ORDER BY item_id
          LIMIT ?2",
    )?;
    let stored_tombstones = statement
        .query_map(params![after_item_id, limit], StoredTombstone::from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut tombstones = Vec::with_capacity(stored_tombstones.len());
    for stored in stored_tombstones {
        tombstones.push(stored.into_wire()?);
    }
    Ok(MutationBatch {
        upserts: Vec::new(),
        tombstones,
    })
}

fn load_mutations(
    connection: &rusqlite::Connection,
    remote_scope: Option<&str>,
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
                        resource_path, content_hash, source_app,
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
            let mut resource_refs = match remote_scope {
                Some(remote_scope) => {
                    load_sync_resource_ref_map(connection, remote_scope, Some(chunk))?
                }
                None => BTreeMap::new(),
            };
            for stored in stored_items {
                let mut item = stored.into_wire()?;
                if let Some(references) = resource_refs.remove(&item.item.id) {
                    restore_sync_resource_refs(&mut item, &references)?;
                }
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
                    resource_path, content_hash, source_app,
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
        let mut resource_refs = match remote_scope {
            Some(remote_scope) => load_sync_resource_ref_map(connection, remote_scope, None)?,
            None => BTreeMap::new(),
        };
        let mut snapshot_upserts = Vec::with_capacity(stored_items.len());
        for stored in stored_items {
            let mut item = stored.into_wire()?;
            if let Some(references) = resource_refs.remove(&item.item.id) {
                restore_sync_resource_refs(&mut item, &references)?;
            }
            snapshot_upserts.push(item);
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

fn load_sync_resource_ref_map(
    connection: &rusqlite::Connection,
    remote_scope: &str,
    item_ids: Option<&[String]>,
) -> Result<BTreeMap<String, Vec<SyncResourceRef>>, StorageError> {
    let Some(scope_id) = sync_resource_scope_id(connection, remote_scope)? else {
        return Ok(BTreeMap::new());
    };
    let (sql, values) = if let Some(item_ids) = item_ids {
        if item_ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let placeholders = (2..=(item_ids.len() + 1))
            .map(|position| format!("?{position}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> =
            Vec::with_capacity(item_ids.len() + 1);
        values.push(Box::new(scope_id));
        values.extend(
            item_ids
                .iter()
                .cloned()
                .map(|item_id| Box::new(item_id) as Box<dyn rusqlite::types::ToSql>),
        );
        (
            format!(
                "SELECT item_id, slot, ordinal, sha256, extension
                   FROM sync_item_resources
                  WHERE scope_id = ?1 AND item_id IN ({placeholders})
                  ORDER BY item_id, slot, ordinal"
            ),
            values,
        )
    } else {
        (
            "SELECT item_id, slot, ordinal, sha256, extension
               FROM sync_item_resources
              WHERE scope_id = ?1
              ORDER BY item_id, slot, ordinal"
                .to_string(),
            vec![Box::new(scope_id) as Box<dyn rusqlite::types::ToSql>],
        )
    };
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map(
            params_from_iter(values.iter().map(|value| value.as_ref())),
            |row| {
                let item_id = row.get::<_, String>(0)?;
                let slot = resource_slot_from_i64(row.get(1)?)?;
                let ordinal = row.get::<_, i64>(2)?;
                let sha256 = row.get::<_, Vec<u8>>(3)?;
                let extension = row.get::<_, String>(4)?;
                let reference = sync_resource_ref_from_parts(
                    slot,
                    u32::try_from(ordinal)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, ordinal))?,
                    &sha256,
                    &extension,
                )?;
                Ok((item_id, reference))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut references = BTreeMap::<String, Vec<SyncResourceRef>>::new();
    for (item_id, reference) in rows {
        references.entry(item_id).or_default().push(reference);
    }
    Ok(references)
}

fn restore_sync_resource_refs(
    replicated: &mut ReplicatedItem,
    references: &[SyncResourceRef],
) -> Result<(), StorageError> {
    if references.is_empty() {
        return Ok(());
    }

    let mut file_paths = replicated
        .item
        .text_content
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok());
    let mut metadata = replicated
        .item
        .metadata_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());

    for reference in references {
        let slot = reference.slot.as_str();
        let ordinal = i64::from(reference.ordinal);
        let object_key = &reference.object_key;
        let parsed = crate::sync::v1::parse_resource_key(object_key)
            .map_err(StorageError::InvalidSyncState)?;
        match slot {
            "image" if ordinal == 0 => {
                if replicated.item.kind != SyncItemKind::Image
                    || parsed.category != crate::sync::v1::ResourceCategory::Image
                {
                    return Err(StorageError::InvalidSyncState(
                        "image sync resource category does not match item kind".to_string(),
                    ));
                }
                replicated.item.resource_path = Some(object_key.clone());
                rewrite_exported_metadata_path(&mut metadata, "resourcePath", object_key);
                rewrite_exported_metadata_path(&mut metadata, "storagePath", object_key);
            }
            "file" => {
                if parsed.category != crate::sync::v1::ResourceCategory::File
                    || replicated.item.kind != SyncItemKind::File
                {
                    return Err(StorageError::InvalidSyncState(
                        "file sync resource category does not match item kind".to_string(),
                    ));
                }
                let index =
                    usize::try_from(ordinal).map_err(|_| StorageError::InvalidStoredValue {
                        field: "sync_item_resources.ordinal",
                        value: ordinal,
                    })?;
                let paths = file_paths.get_or_insert_with(Vec::new);
                if paths.len() <= index {
                    paths.resize(index + 1, String::new());
                }
                paths[index] = object_key.clone();
                if index == 0 {
                    replicated.item.resource_path = Some(object_key.clone());
                    rewrite_exported_metadata_path(&mut metadata, "resourcePath", object_key);
                }
                rewrite_exported_file_metadata_path(&mut metadata, index, object_key);
            }
            "icon" if ordinal == 0 => {
                if parsed.category != crate::sync::v1::ResourceCategory::Icon {
                    return Err(StorageError::InvalidSyncState(
                        "icon sync resource has a non-icon category".to_string(),
                    ));
                }
                replicated.item.icon_path = Some(object_key.clone());
            }
            _ => {
                return Err(StorageError::InvalidSyncState(
                    "sync item resource has an unknown slot".to_string(),
                ));
            }
        }
    }

    if let Some(paths) = file_paths {
        replicated.item.text_content = Some(serde_json::to_string(&paths).map_err(|error| {
            StorageError::InvalidSyncState(format!(
                "failed to encode restored file resource paths: {error}"
            ))
        })?);
    }
    if let Some(metadata) = metadata {
        replicated.item.metadata_json =
            Some(serde_json::to_string(&metadata).map_err(|error| {
                StorageError::InvalidSyncState(format!(
                    "failed to encode restored resource metadata: {error}"
                ))
            })?);
    }
    Ok(())
}

fn rewrite_exported_metadata_path(
    metadata: &mut Option<serde_json::Value>,
    key: &str,
    object_key: &str,
) {
    let Some(serde_json::Value::Object(object)) = metadata else {
        return;
    };
    object.insert(
        key.to_string(),
        serde_json::Value::String(object_key.to_string()),
    );
}

fn rewrite_exported_file_metadata_path(
    metadata: &mut Option<serde_json::Value>,
    index: usize,
    object_key: &str,
) {
    let Some(serde_json::Value::Object(object)) = metadata else {
        return;
    };
    let Some(serde_json::Value::Array(files)) = object.get_mut("files") else {
        return;
    };
    let Some(serde_json::Value::Object(file)) = files.get_mut(index) else {
        return;
    };
    file.insert(
        "storagePath".to_string(),
        serde_json::Value::String(object_key.to_string()),
    );
}

fn set_metadata_path(metadata: &mut serde_json::Value, key: &str, value: &str) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

fn set_file_metadata_path(metadata: &mut serde_json::Value, index: usize, value: &str) {
    let Some(files) = metadata
        .get_mut("files")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    let Some(file) = files.get_mut(index).and_then(|value| value.as_object_mut()) else {
        return;
    };
    file.insert(
        "storagePath".to_string(),
        serde_json::Value::String(value.to_string()),
    );
    file.insert(
        "path".to_string(),
        serde_json::Value::String(value.to_string()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{ClipboardItem, ClipboardKind},
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
            item: item(id, hash, text).into(),
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
                connection.execute(
                    "INSERT INTO sync_metadata (key, value) VALUES (?1, 'stale')",
                    [sync_head_cache_key(REMOTE_SCOPE, REMOTE_DEVICE)?],
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
    fn sync_head_cache_round_trips_and_ignores_corrupt_values() {
        let database = Database::open_in_memory().unwrap();
        let snapshot_sha256 = "b".repeat(64);
        let head = DeviceHead {
            device_id: REMOTE_DEVICE.to_string(),
            epoch: REMOTE_EPOCH.to_string(),
            snapshot: ObjectRef {
                key: snapshot_object_key(REMOTE_DEVICE, REMOTE_EPOCH, &snapshot_sha256).unwrap(),
                sha256: snapshot_sha256,
                stored_size_bytes: 123,
                record_count: 4,
            },
            published_sequence: 5,
            last_segment_key: None,
            updated_at_ms: 10,
        };
        database
            .record_sync_head_cache(
                REMOTE_SCOPE,
                REMOTE_DEVICE,
                "\"etag-one\"",
                88,
                Some(1234),
                &head,
            )
            .unwrap();

        let cached = database
            .get_sync_head_cache(REMOTE_SCOPE, REMOTE_DEVICE)
            .unwrap()
            .unwrap();
        assert_eq!(cached.etag, "\"etag-one\"");
        assert_eq!(cached.stored_size_bytes, 88);
        assert_eq!(cached.modified_ms, Some(1234));
        assert!(cached.matches_head(&head));
        assert!(cached.matches_cursor(&DeviceCursor {
            device_id: REMOTE_DEVICE.to_string(),
            epoch: REMOTE_EPOCH.to_string(),
            sequence: 5,
            last_segment_key: None,
        }));

        let key = sync_head_cache_key(REMOTE_SCOPE, REMOTE_DEVICE).unwrap();
        database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE sync_metadata SET value = '{broken' WHERE key = ?1",
                    [&key],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(database
            .get_sync_head_cache(REMOTE_SCOPE, REMOTE_DEVICE)
            .unwrap()
            .is_none());
        assert!(database
            .record_sync_head_cache(REMOTE_SCOPE, REMOTE_DEVICE, "bad\netag", 88, None, &head)
            .is_err());
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
    fn snapshot_visitor_is_deterministic_bounded_and_point_in_time() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        for index in 0..7 {
            database
                .save_item(&item(
                    &format!("item-{index:02}"),
                    &format!("hash-{index:02}"),
                    &format!("value-{index:02}"),
                ))
                .unwrap();
        }
        database.soft_delete("item-06").unwrap();
        let expected_sequence = current_sequence(&database.connection.lock().unwrap()).unwrap();

        let mut batches = Vec::new();
        let export = database
            .visit_sync_snapshot_for_scope(REMOTE_SCOPE, 2, |batch| {
                assert!(batch.len() <= 2);
                batches.push(batch);
                Ok(())
            })
            .unwrap();

        assert_eq!(export.through_sequence, expected_sequence);
        assert_eq!(export.record_count, 7);
        let upsert_ids = batches
            .iter()
            .flat_map(|batch| batch.upserts.iter().map(|item| item.item.id.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            upsert_ids,
            vec!["item-00", "item-01", "item-02", "item-03", "item-04", "item-05"]
        );
        assert_eq!(
            batches
                .iter()
                .flat_map(|batch| batch.tombstones.iter())
                .map(|tombstone| tombstone.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-06"]
        );
    }

    #[test]
    fn batched_snapshot_apply_rolls_back_every_prior_chunk_on_late_failure() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let good = MutationBatch {
            upserts: vec![replicated(
                "remote-good",
                "remote-good-hash",
                "good",
                RecordVersion {
                    modified_at_ms: 10,
                    writer_device_id: "11111111-1111-4111-8111-111111111111".to_string(),
                },
            )],
            tombstones: Vec::new(),
        };
        let bad = MutationBatch {
            upserts: vec![replicated(
                "remote-bad",
                "remote-bad-hash",
                "bad",
                RecordVersion {
                    modified_at_ms: 11,
                    writer_device_id: "not-a-uuid".to_string(),
                },
            )],
            tombstones: Vec::new(),
        };
        let cursor = DeviceCursor {
            device_id: "22222222-2222-4222-8222-222222222222".to_string(),
            epoch: "33333333-3333-4333-8333-333333333333".to_string(),
            sequence: 2,
            last_segment_key: None,
        };

        assert!(database
            .apply_sync_snapshot_batches(
                REMOTE_SCOPE,
                &cursor,
                &"a".repeat(64),
                [Ok((good, BTreeMap::new())), Ok((bad, BTreeMap::new())),],
            )
            .is_err());
        assert!(database.get_item("remote-good").unwrap().is_none());
        assert!(database
            .get_sync_cursor(REMOTE_SCOPE, &cursor.device_id)
            .unwrap()
            .is_none());
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
    fn resource_refs_commit_with_the_cursor_and_survive_local_only_updates() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let mut remote = replicated(
            "remote-image",
            "hash-remote-image",
            "remote image",
            RecordVersion {
                modified_at_ms: 500,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        );
        remote.item.kind = ClipboardKind::Image.into();
        remote.item.text_content = None;
        remote.item.resource_path = None;
        remote.item.metadata_json = Some("{}".to_string());
        let object_key = format!("v1/resources/image/sha256-{}.png", "a".repeat(64));
        let refs = BTreeMap::from([(
            remote.item.id.clone(),
            vec![SyncResourceRef {
                slot: "image".to_string(),
                ordinal: 0,
                object_key: object_key.clone(),
            }],
        )]);
        database
            .apply_sync_snapshot_with_resources(
                REMOTE_SCOPE,
                &cursor(0, None),
                &"a".repeat(64),
                &MutationBatch {
                    upserts: vec![remote],
                    tombstones: Vec::new(),
                },
                &refs,
            )
            .unwrap();
        database
            .set_preview_path("remote-image", "previews/remote-image.jpg")
            .unwrap();

        let exported = database
            .export_sync_snapshot_for_scope(REMOTE_SCOPE)
            .unwrap();
        assert_eq!(
            exported.mutations.upserts[0].item.resource_path.as_deref(),
            Some(object_key.as_str())
        );
        assert!(exported.mutations.upserts[0].item.preview_path.is_none());

        let mut invalid_refs = refs;
        invalid_refs.get_mut("remote-image").unwrap()[0].object_key = "invalid".to_string();
        let mut newer = exported.mutations.upserts[0].clone();
        newer.version.modified_at_ms += 1;
        assert!(database
            .apply_sync_segment_with_resources(
                REMOTE_SCOPE,
                &cursor(1, Some("segment-1")),
                &MutationBatch {
                    upserts: vec![newer],
                    tombstones: Vec::new(),
                },
                &invalid_refs,
            )
            .is_err());
        assert!(
            database
                .get_sync_cursor(REMOTE_SCOPE, REMOTE_DEVICE)
                .unwrap()
                .unwrap()
                .sequence
                == 0
        );
        assert_eq!(
            database
                .export_sync_snapshot_for_scope(REMOTE_SCOPE)
                .unwrap()
                .mutations
                .upserts[0]
                .item
                .resource_path
                .as_deref(),
            Some(object_key.as_str())
        );
    }

    #[test]
    fn materialized_resources_update_paths_without_replicating_the_cache_write() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let first_key = format!("v1/resources/file/sha256-{}.txt", "b".repeat(64));
        let second_key = format!("v1/resources/file/sha256-{}.txt", "c".repeat(64));
        let icon_key = format!("v1/resources/icon/sha256-{}.png", "d".repeat(64));
        let mut remote = replicated(
            "materialized-files",
            "hash-materialized-files",
            "materialized files",
            RecordVersion {
                modified_at_ms: 500,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        );
        remote.item.kind = ClipboardKind::File.into();
        remote.item.resource_path = None;
        remote.item.text_content = Some("[\"\",\"\"]".to_string());
        remote.item.metadata_json = Some(
            serde_json::json!({
                "resourcePath": null,
                "files": [{"storagePath": null}, {"storagePath": null}],
                "iconPath": null
            })
            .to_string(),
        );
        let refs = vec![
            SyncResourceRef {
                slot: "file".to_string(),
                ordinal: 0,
                object_key: first_key.clone(),
            },
            SyncResourceRef {
                slot: "file".to_string(),
                ordinal: 1,
                object_key: second_key.clone(),
            },
            SyncResourceRef {
                slot: "icon".to_string(),
                ordinal: 0,
                object_key: icon_key.clone(),
            },
        ];
        database
            .apply_sync_snapshot_with_resources(
                REMOTE_SCOPE,
                &cursor(0, None),
                &"a".repeat(64),
                &MutationBatch {
                    upserts: vec![remote],
                    tombstones: Vec::new(),
                },
                &BTreeMap::from([("materialized-files".to_string(), refs.clone())]),
            )
            .unwrap();
        let version_before = database
            .with_connection(|connection| {
                Ok(connection.query_row(
                    "SELECT modified_at_ms, sync_writer_device_id
                       FROM clipboard_items WHERE id = 'materialized-files'",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )?)
            })
            .unwrap();

        assert!(database
            .mark_sync_resources_materialized(
                REMOTE_SCOPE,
                "materialized-files",
                &[
                    (refs[0].clone(), "C:\\cache\\first.txt".to_string()),
                    (refs[1].clone(), "C:\\cache\\second.txt".to_string()),
                    (refs[2].clone(), "C:\\cache\\icons\\source.png".to_string()),
                ],
            )
            .unwrap());

        let stored = database.get_item("materialized-files").unwrap().unwrap();
        assert_eq!(
            stored.resource_path.as_deref(),
            Some("C:\\cache\\first.txt")
        );
        assert_eq!(stored.icon_path.as_deref(), Some("source.png"));
        assert_eq!(
            serde_json::from_str::<Vec<String>>(stored.text_content.as_deref().unwrap()).unwrap(),
            ["C:\\cache\\first.txt", "C:\\cache\\second.txt"]
        );
        let metadata: serde_json::Value =
            serde_json::from_str(stored.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(metadata["files"][1]["storagePath"], "C:\\cache\\second.txt");
        assert_eq!(metadata["iconPath"], "source.png");
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
        assert_eq!(
            database
                .get_sync_resource_refs(REMOTE_SCOPE, "materialized-files")
                .unwrap(),
            refs
        );
        let version_after = database
            .with_connection(|connection| {
                Ok(connection.query_row(
                    "SELECT modified_at_ms, sync_writer_device_id
                       FROM clipboard_items WHERE id = 'materialized-files'",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(version_after, version_before);
        let exported = database
            .export_sync_snapshot_for_scope(REMOTE_SCOPE)
            .unwrap();
        let item = &exported.mutations.upserts[0].item;
        assert_eq!(item.resource_path.as_deref(), Some(first_key.as_str()));
        assert_eq!(item.icon_path.as_deref(), Some(icon_key.as_str()));
        assert_eq!(
            serde_json::from_str::<Vec<String>>(item.text_content.as_deref().unwrap()).unwrap(),
            vec![first_key, second_key]
        );
    }

    #[test]
    fn materialized_resource_write_is_atomic_when_any_reference_is_stale() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let object_key = format!("v1/resources/image/sha256-{}.png", "a".repeat(64));
        let reference = SyncResourceRef {
            slot: "image".to_string(),
            ordinal: 0,
            object_key: object_key.clone(),
        };
        let mut remote = replicated(
            "atomic-image",
            "hash-atomic-image",
            "atomic image",
            RecordVersion {
                modified_at_ms: 500,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        );
        remote.item.kind = ClipboardKind::Image.into();
        remote.item.text_content = None;
        remote.item.resource_path = None;
        database
            .apply_sync_snapshot_with_resources(
                REMOTE_SCOPE,
                &cursor(0, None),
                &"a".repeat(64),
                &MutationBatch {
                    upserts: vec![remote],
                    tombstones: Vec::new(),
                },
                &BTreeMap::from([("atomic-image".to_string(), vec![reference.clone()])]),
            )
            .unwrap();
        let stale = SyncResourceRef {
            object_key: format!("v1/resources/icon/sha256-{}.png", "b".repeat(64)),
            slot: "icon".to_string(),
            ordinal: 0,
        };

        assert!(!database
            .mark_sync_resources_materialized(
                REMOTE_SCOPE,
                "atomic-image",
                &[
                    (reference, "C:\\cache\\image.png".to_string()),
                    (stale, "C:\\cache\\icon.png".to_string()),
                ],
            )
            .unwrap());
        assert!(database
            .get_item("atomic-image")
            .unwrap()
            .unwrap()
            .resource_path
            .is_none());
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
    }

    #[test]
    fn single_file_materialization_keeps_null_text_content_and_reuses_resource_path() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let object_key = format!("v1/resources/file/sha256-{}.txt", "e".repeat(64));
        let reference = SyncResourceRef {
            slot: "file".to_string(),
            ordinal: 0,
            object_key,
        };
        let mut remote = replicated(
            "single-file",
            "hash-single-file",
            "single file",
            RecordVersion {
                modified_at_ms: 500,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        );
        remote.item.kind = ClipboardKind::File.into();
        remote.item.resource_path = None;
        remote.item.text_content = None;
        remote.item.metadata_json = Some(
            serde_json::json!({
                "resourcePath": null,
                "files": [{"storagePath": null}]
            })
            .to_string(),
        );
        database
            .apply_sync_snapshot_with_resources(
                REMOTE_SCOPE,
                &cursor(0, None),
                &"f".repeat(64),
                &MutationBatch {
                    upserts: vec![remote],
                    tombstones: Vec::new(),
                },
                &BTreeMap::from([("single-file".to_string(), vec![reference.clone()])]),
            )
            .unwrap();

        let local_path = "C:\\cache\\single.txt";
        assert!(database
            .mark_sync_resource_materialized(REMOTE_SCOPE, "single-file", &reference, local_path,)
            .unwrap());
        let stored = database.get_item("single-file").unwrap().unwrap();
        assert_eq!(stored.resource_path.as_deref(), Some(local_path));
        assert!(stored.text_content.is_none());
        assert_eq!(
            database
                .materialized_sync_resource_path(REMOTE_SCOPE, "single-file", &reference)
                .unwrap()
                .as_deref(),
            Some(local_path)
        );
        assert!(!database
            .mark_sync_resource_materialized(REMOTE_SCOPE, "single-file", &reference, local_path,)
            .unwrap());
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
    }

    #[test]
    fn unchanged_remote_file_refs_preserve_materialized_local_paths() {
        let database = Database::open_in_memory().unwrap();
        database.initialize_sync().unwrap();
        let first_key = format!("v1/resources/file/sha256-{}.txt", "b".repeat(64));
        let second_key = format!("v1/resources/file/sha256-{}.txt", "c".repeat(64));
        let local_paths = vec!["C:\\cache\\first.txt", "C:\\cache\\second.txt"];
        let mut remote = replicated(
            "remote-files",
            "hash-remote-files",
            "remote files",
            RecordVersion {
                modified_at_ms: 500,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        );
        remote.item.kind = ClipboardKind::File.into();
        remote.item.resource_path = Some(local_paths[0].to_string());
        remote.item.text_content = Some(serde_json::to_string(&local_paths).unwrap());
        remote.item.metadata_json = Some(
            serde_json::json!({
                "resourcePath": local_paths[0],
                "files": [
                    {"storagePath": local_paths[0]},
                    {"storagePath": local_paths[1]}
                ]
            })
            .to_string(),
        );
        let refs = BTreeMap::from([(
            remote.item.id.clone(),
            vec![
                SyncResourceRef {
                    slot: "file".to_string(),
                    ordinal: 0,
                    object_key: first_key.clone(),
                },
                SyncResourceRef {
                    slot: "file".to_string(),
                    ordinal: 1,
                    object_key: second_key.clone(),
                },
            ],
        )]);
        database
            .apply_sync_snapshot_with_resources(
                REMOTE_SCOPE,
                &cursor(0, None),
                &"a".repeat(64),
                &MutationBatch {
                    upserts: vec![remote.clone()],
                    tombstones: Vec::new(),
                },
                &refs,
            )
            .unwrap();

        remote.version.modified_at_ms += 1;
        remote.item.title = "renamed remotely".to_string();
        remote.item.resource_path = None;
        remote.item.text_content = Some("[\"\",\"\"]".to_string());
        remote.item.metadata_json = Some(
            serde_json::json!({
                "resourcePath": null,
                "files": [{"storagePath": null}, {"storagePath": null}],
                "tags": ["remote"]
            })
            .to_string(),
        );
        database
            .apply_sync_segment_with_resources(
                REMOTE_SCOPE,
                &cursor(1, Some("segment-1")),
                &MutationBatch {
                    upserts: vec![remote],
                    tombstones: Vec::new(),
                },
                &refs,
            )
            .unwrap();

        let stored = database.get_item("remote-files").unwrap().unwrap();
        assert_eq!(stored.resource_path.as_deref(), Some(local_paths[0]));
        assert_eq!(
            serde_json::from_str::<Vec<String>>(stored.text_content.as_deref().unwrap()).unwrap(),
            local_paths
        );
        let metadata: serde_json::Value =
            serde_json::from_str(stored.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(metadata["files"][1]["storagePath"], local_paths[1]);
        assert_eq!(metadata["tags"], serde_json::json!(["remote"]));
        let exported = database
            .export_sync_snapshot_for_scope(REMOTE_SCOPE)
            .unwrap();
        let item = &exported.mutations.upserts[0].item;
        assert_eq!(item.resource_path.as_deref(), Some(first_key.as_str()));
        assert_eq!(
            serde_json::from_str::<Vec<String>>(item.text_content.as_deref().unwrap()).unwrap(),
            vec![first_key, second_key]
        );
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
                kind: ClipboardKind::Text.into(),
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
                item: item("broken-checkpoint", "hash-broken-checkpoint", "broken").into(),
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
                kind: ClipboardKind::Text.into(),
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
        collision.item.kind = ClipboardKind::Image.into();
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
