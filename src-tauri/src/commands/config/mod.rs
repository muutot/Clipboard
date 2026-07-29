pub mod storage;
pub mod settings;
pub mod misc;

use serde::Serialize;

use crate::config::GeneralConfig;
use crate::storage::KindStorageStats;

pub use misc::*;
pub use settings::*;
pub use storage::*;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    item_count: u64,
    image_count: u64,
    image_size_bytes: u64,
    file_count: u64,
    file_size_bytes: u64,
    text_count: u64,
    link_count: u64,
    project_path: String,
    config_path: String,
    keyboard_config_path: String,
    data_directory_path: String,
    uses_custom_data_directory: bool,
    storage_path: String,
    icons_dir: String,
    database_path: String,
    database_size_bytes: u64,
    files_path: String,
    image_path: String,
    image_cleanup_enabled: bool,
    file_cleanup_enabled: bool,
    search_index_path: String,
    search_index_size_bytes: u64,
    search_index_version: u32,
    search_index_rebuild_required: bool,
    disk_total_bytes: Option<u64>,
    disk_available_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageKindStats {
    item_count: u64,
    size_bytes: u64,
}

impl From<KindStorageStats> for StorageKindStats {
    fn from(stats: KindStorageStats) -> Self {
        Self {
            item_count: stats.item_count,
            size_bytes: stats.size_bytes,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageDirectoryUpdate {
    data_directory_path: String,
    storage_path: String,
    restart_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStorageUpdate {
    image_storage_path: String,
    file_storage_path: String,
    restart_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveredApplication {
    name: String,
    icon_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationFilterSettings {
    discovered_applications: Vec<String>,
    discovered_applications_with_icons: Vec<DiscoveredApplication>,
    ignored_applications: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowConfigInfo {
    launch_at_startup: bool,
    close_to_tray: bool,
    single_instance: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfigInfo {
    schedule_auto_export: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsInfo {
    settings: GeneralConfig,
    legacy_migration_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryConfigInfo {
    max_items: u32,
    retention_days: u32,
    recycle_bin_days: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfigInfo {
    max_file_copy_size_bytes: u64,
    max_screenshot_size_bytes: u64,
    image_storage_path: Option<String>,
    file_storage_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyStatus {
    paused: bool,
    password_manager_apps: Vec<String>,
    master_password_hash_set: bool,
}
