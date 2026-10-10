//! Sync-state persistence, split by concern.
//!
//! The `impl Database` blocks live in focused submodules; shared row types,
//! constants and free helpers stay here so every submodule can reach them with
//! `use super::*`. The split is a pure move: no behavior, signatures or test
//! expectations changed.

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

mod apply;
mod checkpoints;
mod device;
mod outbox;
mod remote;
mod resources;
#[cfg(test)]
mod tests;

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
