//! Provider-neutral object-store and namespace primitives for sync protocol v1.

pub mod layout;
pub mod remote;

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
