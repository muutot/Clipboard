pub mod actions;
pub mod detector;
pub mod file_store;
pub mod hash;
pub mod resource_metadata;
pub mod self_trigger;
pub mod thumbnail;
pub mod transform;

pub use actions::{detect_actions, QuickAction};
pub use detector::{detect_markers, ContentMarkers};
pub use file_store::{FileStorageInfo, FileStore};
pub use hash::{
    compute_content_hash, compute_media_hash, compute_media_write_hashes,
    compute_normalized_media_hash, icon_key, AppIconStore,
};
pub use resource_metadata::{
    created_at_ms, extension_for_path, mime_type_for_path, mime_type_from_bytes, modified_at_ms,
    RESOURCE_METADATA_SCHEMA_VERSION,
};
pub use self_trigger::{DedupResult, SelfTriggerGuard};
pub use thumbnail::{ThumbnailGenerator, ThumbnailInfo, ThumbnailQueue, ThumbnailWorker};
pub use transform::{TextTransform, TransformOperation};
