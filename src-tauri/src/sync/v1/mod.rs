//! S3-first sync v1 primitives.
//!
//! This namespace is intentionally incompatible with the discarded baseline
//! and oplog implementation. Transport orchestration is added separately so
//! the strict object layout and wire envelope stay independently testable.

pub mod engine;
pub mod layout;
pub mod remote;
pub mod resources;
pub mod wire;

pub use engine::{sync_database, SyncEngineOptions, SyncEngineResult};

pub use layout::{
    checkpoint_object_key, head_object_key, obsolete_object_candidate, parse_head_key,
    parse_resource_key, parse_segment_key, resource_object_key, segment_object_key, segment_prefix,
    snapshot_object_key, ParsedResourceKey, ParsedSegmentKey, ResourceCategory,
    CHECKPOINT_HEAD_KEY, HEADS_PREFIX, V1_ROOT,
};
pub use remote::{
    cleanup_obsolete_objects, DownloadedFile, DownloadedObject, ObjectInfo, ObjectMetadata,
    ObjectStore, ObsoleteCleanupReport, PutCondition, PutOutcome, S3ObjectStore,
};
pub use resources::{
    ensure_resource_uploaded, fingerprint_resource, materialize_mutation_resources,
    materialize_resource, prepare_mutation_resources, MaterializedResource, ResourceDescriptor,
    ResourceLimits, ResourceTransferStats, ResourceUploadResult,
};
pub use wire::{
    decode_checkpoint, decode_checkpoint_head, decode_device_head, decode_segment, decode_snapshot,
    encode_checkpoint, encode_checkpoint_head, encode_device_head, encode_segment, encode_snapshot,
    Checkpoint, CheckpointHead, DeviceCursor, DeviceHead, EncodedObject, MutationBatch, ObjectRef,
    RecordVersion, ReplicatedItem, Segment, SessionKey, Snapshot, Tombstone,
};
