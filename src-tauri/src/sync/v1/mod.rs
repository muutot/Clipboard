//! S3-first sync v1 primitives.
//!
//! This namespace is intentionally incompatible with the discarded baseline
//! and oplog implementation. Transport orchestration is added separately so
//! the strict object layout and wire envelope stay independently testable.

#[cfg(test)]
mod engine_tests;

pub use clipboard_sync::v1::{engine, layout, remote, repository, resources, wire};
pub use engine::{sync_database, SyncEngineOptions, SyncEngineResult};

pub use clipboard_sync::s3::{S3RequestMetrics, S3RequestMetricsSnapshot};
pub use layout::{
    checkpoint_object_key, head_object_key, obsolete_object_candidate, parse_checkpoint_key,
    parse_head_key, parse_resource_key, parse_segment_key, resource_object_key, segment_object_key,
    segment_prefix, snapshot_object_key, ParsedCheckpointKey, ParsedResourceKey, ParsedSegmentKey,
    ResourceCategory, CHECKPOINT_HEAD_KEY, HEADS_PREFIX, V1_ROOT,
};
pub use remote::{
    cleanup_obsolete_objects, DownloadedFile, DownloadedObject, ObjectInfo, ObjectMetadata,
    ObjectStore, ObsoleteCleanupReport, PutCondition, PutOutcome, S3ObjectStore,
};
pub use repository::{
    SyncEnginePaths, SyncHeadCache, SyncIncomingBatch, SyncOutboxBatch, SyncRemoteState,
    SyncRepository, SyncResourceReferences, SyncSnapshot, SyncSnapshotExport,
};
pub use resources::{
    collect_mutation_resource_refs, defer_mutation_resources, ensure_resource_uploaded,
    fingerprint_resource, materialize_mutation_resources, materialize_resource,
    prepare_mutation_resources, verify_local_resource, MaterializedResource, ResourceDescriptor,
    ResourceLimits, ResourceRoots, ResourceTransferStats, ResourceUploadResult, SyncResourceRef,
};
pub use wire::{
    decode_checkpoint_head, decode_device_head, decode_segment, encode_checkpoint_head,
    encode_device_head, encode_segment, large_pack_chunk_limit_bytes, mutation_batch_encoded_size,
    open_checkpoint_pack, open_snapshot_pack, CheckpointHead, CheckpointPackHeader,
    CheckpointPackReader, DeviceCursor, DeviceHead, EncodedFile, EncodedObject, LargePackKind,
    LargePackWriter, MutationBatch, ObjectRef, RecordVersion, ReplicatedItem, Segment, SessionKey,
    SnapshotPackHeader, SnapshotPackReader, SyncItem, SyncItemKind, Tombstone,
};

use crate::domain::{ClipboardItem, ClipboardKind};

impl From<&crate::storage::StoragePaths> for SyncEnginePaths {
    fn from(paths: &crate::storage::StoragePaths) -> Self {
        Self::new(
            paths.data_directory.clone(),
            ResourceRoots::new(
                paths.images.clone(),
                paths.files.clone(),
                paths.storage.join("icons"),
            ),
        )
    }
}

impl From<ClipboardKind> for SyncItemKind {
    fn from(kind: ClipboardKind) -> Self {
        match kind {
            ClipboardKind::Text => Self::Text,
            ClipboardKind::Link => Self::Link,
            ClipboardKind::Image => Self::Image,
            ClipboardKind::File => Self::File,
        }
    }
}

impl From<SyncItemKind> for ClipboardKind {
    fn from(kind: SyncItemKind) -> Self {
        match kind {
            SyncItemKind::Text => Self::Text,
            SyncItemKind::Link => Self::Link,
            SyncItemKind::Image => Self::Image,
            SyncItemKind::File => Self::File,
        }
    }
}

impl From<ClipboardItem> for SyncItem {
    fn from(item: ClipboardItem) -> Self {
        Self {
            id: item.id,
            kind: item.kind.into(),
            title: item.title,
            text_content: item.text_content,
            html_content: item.html_content,
            rtf_content: item.rtf_content,
            resource_path: item.resource_path,
            preview_path: item.preview_path,
            content_hash: item.content_hash,
            source_app: item.source_app,
            icon_path: item.icon_path,
            size_bytes: item.size_bytes,
            created_at_ms: item.created_at_ms,
            last_used_at_ms: item.last_used_at_ms,
            is_favorite: item.is_favorite,
            metadata_json: item.metadata_json,
        }
    }
}

impl From<SyncItem> for ClipboardItem {
    fn from(item: SyncItem) -> Self {
        Self {
            id: item.id,
            kind: item.kind.into(),
            title: item.title,
            text_content: item.text_content,
            html_content: item.html_content,
            rtf_content: item.rtf_content,
            resource_path: item.resource_path,
            preview_path: item.preview_path,
            content_hash: item.content_hash,
            source_app: item.source_app,
            icon_path: item.icon_path,
            size_bytes: item.size_bytes,
            created_at_ms: item.created_at_ms,
            last_used_at_ms: item.last_used_at_ms,
            is_favorite: item.is_favorite,
            metadata_json: item.metadata_json,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application_item(kind: ClipboardKind) -> ClipboardItem {
        ClipboardItem {
            id: "wire-compatible-item".to_string(),
            kind,
            title: "title".to_string(),
            text_content: Some("text".to_string()),
            html_content: Some("<b>text</b>".to_string()),
            rtf_content: Some(r"{\rtf1 text}".to_string()),
            resource_path: Some("v1/resources/file/sha256-deadbeef.bin".to_string()),
            preview_path: None,
            content_hash: "hash".to_string(),
            source_app: Some("test".to_string()),
            icon_path: None,
            size_bytes: 4,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: true,
            metadata_json: Some("{}".to_string()),
        }
    }

    #[test]
    fn protocol_dto_preserves_the_original_v1_bincode_layout() {
        for kind in [
            ClipboardKind::Text,
            ClipboardKind::Link,
            ClipboardKind::Image,
            ClipboardKind::File,
        ] {
            let application = application_item(kind);
            let protocol = SyncItem::from(application.clone());
            let application_bytes =
                bincode::encode_to_vec(&application, bincode::config::standard()).unwrap();
            let protocol_bytes =
                bincode::encode_to_vec(&protocol, bincode::config::standard()).unwrap();

            assert_eq!(protocol_bytes, application_bytes);
            assert_eq!(ClipboardItem::from(protocol), application);
        }
    }
}
