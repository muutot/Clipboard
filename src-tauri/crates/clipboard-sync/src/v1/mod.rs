//! Provider-neutral object-store and namespace primitives for sync protocol v1.

pub mod engine;
pub mod layout;
pub mod remote;
pub mod repository;
pub mod resources;
pub mod wire;

pub use engine::{sync_database, SyncEngineOptions, SyncEngineResult};
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
pub use crate::s3::{S3RequestMetrics, S3RequestMetricsSnapshot};
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
