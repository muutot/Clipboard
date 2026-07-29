use std::path::PathBuf;

pub const RESOURCE_ROOT_MARKER: &str = ".clipboard-resource-root";
pub(super) const RESOURCE_ROOT_MARKER_HEADER: &str = "clipboard-resource-root-v1";

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub project: PathBuf,
    pub data_directory: PathBuf,
    pub storage: PathBuf,
    pub images: PathBuf,
    pub previews: PathBuf,
    pub files: PathBuf,
    pub database_directory: PathBuf,
    pub database: PathBuf,
    pub search_index: PathBuf,
    /// Whether orphan cleanup is allowed to scan the image resource root.
    ///
    /// Default application directories are owned by the app. A custom
    /// directory is scanned only when it carries the explicit ownership
    /// marker; legacy/non-empty directories without the marker remain usable
    /// but are never treated as disposable storage.
    pub image_cleanup_enabled: bool,
    /// Whether orphan cleanup is allowed to scan the file resource root.
    pub file_cleanup_enabled: bool,
}
