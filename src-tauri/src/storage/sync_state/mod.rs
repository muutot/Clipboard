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
mod mutations;
mod outbox;
mod remote;
mod resource_refs;
mod resources;
mod snapshot;
mod state;
mod versions;

#[cfg(test)]
mod tests;

use mutations::*;
use resource_refs::*;
use snapshot::*;
use state::*;
use versions::*;

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
