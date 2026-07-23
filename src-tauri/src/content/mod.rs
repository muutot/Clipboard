pub mod actions;
pub mod clipboard_format;
pub mod detector;
pub mod file_store;
pub mod hash;
pub mod transform;

pub use actions::{detect_actions, QuickAction};
pub use clipboard_format::{
    detect_formats_from_mime_list, parse_mime_types, ClipboardFormat, ClipboardFormatInfo,
    ClipboardFormatReader,
};
pub use detector::{detect_markers, ContentMarkers};
pub use file_store::{FileStore, FileStorageInfo};
pub use hash::{compute_content_hash, icon_key, AppIconStore, DedupResult, SelfTriggerGuard};
pub use transform::{TextTransform, TransformOperation};
