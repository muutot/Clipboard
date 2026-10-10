//! Protocol-owned sync v1 DTOs and their frozen bincode field order.

use super::*;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Encode, Decode,
)]
#[serde(rename_all = "camelCase")]
pub enum SyncItemKind {
    Text,
    Link,
    Image,
    File,
}

/// Protocol-owned record DTO. Field order is frozen for the v1 bincode layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct SyncItem {
    pub id: String,
    pub kind: SyncItemKind,
    pub title: String,
    pub text_content: Option<String>,
    #[serde(default)]
    pub html_content: Option<String>,
    #[serde(default)]
    pub rtf_content: Option<String>,
    pub resource_path: Option<String>,
    pub preview_path: Option<String>,
    pub content_hash: String,
    pub source_app: Option<String>,
    pub icon_path: Option<String>,
    pub size_bytes: u64,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
    pub is_favorite: bool,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct SnapshotPackHeader {
    pub device_id: String,
    pub epoch: String,
    pub through_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct CheckpointPackHeader {
    pub generation: u64,
    pub vector: Vec<DeviceCursor>,
}

#[derive(Debug)]
pub struct EncodedFile {
    pub(super) path: PathBuf,
    pub sha256: String,
    pub stored_size_bytes: u64,
    pub uncompressed_size_bytes: u64,
    pub record_count: u64,
}

impl EncodedFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for EncodedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Encode, Decode)]
pub struct RecordVersion {
    pub modified_at_ms: i64,
    pub writer_device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ReplicatedItem {
    pub item: SyncItem,
    pub version: RecordVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Tombstone {
    pub item_id: String,
    pub kind: SyncItemKind,
    pub content_hash: String,
    pub deleted_at_ms: i64,
    pub version: RecordVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MutationBatch {
    pub upserts: Vec<ReplicatedItem>,
    pub tombstones: Vec<Tombstone>,
}

impl MutationBatch {
    pub fn len(&self) -> usize {
        self.upserts.len() + self.tombstones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty() && self.tombstones.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ObjectRef {
    pub key: String,
    pub sha256: String,
    pub stored_size_bytes: u64,
    pub record_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct DeviceHead {
    pub device_id: String,
    pub epoch: String,
    pub snapshot: ObjectRef,
    pub published_sequence: u64,
    pub last_segment_key: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct DeviceCursor {
    pub device_id: String,
    pub epoch: String,
    pub sequence: u64,
    pub last_segment_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Segment {
    pub device_id: String,
    pub epoch: String,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub mutations: MutationBatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct CheckpointHead {
    pub generation: u64,
    pub checkpoint: ObjectRef,
    pub vector: Vec<DeviceCursor>,
    pub previous_checkpoint: Option<ObjectRef>,
    pub updated_at_ms: i64,
}
