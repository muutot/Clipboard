use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const CONFIG_DIRECTORY_NAME: &str = "conf";
pub(crate) const CONFIG_FILE_NAME: &str = "conf.json";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    pub storage: StorageConfig,
    pub history: HistoryConfig,
    pub privacy: PrivacyConfig,
    pub permissions: PermissionConfig,
    pub window: WindowConfig,
    pub general: GeneralConfig,
    pub export: ExportConfig,
    pub ocr: OcrConfig,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FontSizeConfig {
    pub base: u16,
    pub secondary: u16,
    pub tiny: u16,
    pub card_title: u16,
    pub card_preview: u16,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for FontSizeConfig {
    fn default() -> Self {
        Self {
            base: 14,
            secondary: 11,
            tiny: 10,
            card_title: 13,
            card_preview: 11,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DisplayConfig {
    pub show_secondary_text: bool,
    pub max_text_lines: u16,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            show_secondary_text: true,
            max_text_lines: 3,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeneralConfig {
    pub language: String,
    pub font_sizes: FontSizeConfig,
    pub display: DisplayConfig,
    pub window_transparency: u8,
    pub window_effect: String,
    pub compact_mode: bool,
    pub compact_padding_top: u16,
    pub compact_padding_bottom: u16,
    pub compact_card_gap: u16,
    pub compact_text_height: u16,
    pub compact_tall_text_height: u16,
    pub compact_image_height: u16,
    pub compact_custom_title_height: u16,
    pub compact_search_height: u16,
    pub compact_search_font_size: u16,
    pub compact_card_border_radius: u16,
    pub pin_copied_to_top: bool,
    pub use_recycle_bin: bool,
    pub show_toast_notifications: bool,
    pub remember_window_position: bool,
    pub always_on_top: bool,
    pub use_system_title_bar: bool,
    pub theme: String,
    pub image_fullscreen_mode: String,
    pub viewer_backdrop_opacity: u8,
    pub search_suggestion_mode: String,
    pub search_history_enabled: bool,
    pub show_settings_close_button: bool,
    pub page_size_limit: u32,
    pub search_page_size_limit: u32,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            language: "zh-CN".to_owned(),
            font_sizes: FontSizeConfig::default(),
            display: DisplayConfig::default(),
            window_transparency: 95,
            window_effect: "off".to_owned(),
            compact_mode: false,
            compact_padding_top: 6,
            compact_padding_bottom: 4,
            compact_card_gap: 5,
            compact_text_height: 58,
            compact_tall_text_height: 70,
            compact_image_height: 130,
            compact_custom_title_height: 80,
            compact_search_height: 40,
            compact_search_font_size: 14,
            compact_card_border_radius: 10,
            pin_copied_to_top: true,
            use_recycle_bin: true,
            show_toast_notifications: true,
            remember_window_position: false,
            always_on_top: false,
            use_system_title_bar: false,
            theme: "dark".to_owned(),
            image_fullscreen_mode: "overlay".to_owned(),
            viewer_backdrop_opacity: 92,
            search_suggestion_mode: "off".to_owned(),
            search_history_enabled: false,
            show_settings_close_button: true,
            page_size_limit: 500,
            search_page_size_limit: 500,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StorageConfig {
    pub data_directory: Option<PathBuf>,
    pub image_storage_path: Option<PathBuf>,
    pub file_storage_path: Option<PathBuf>,
    pub max_file_copy_size_bytes: u64,
    pub max_screenshot_size_bytes: u64,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_directory: None,
            image_storage_path: None,
            file_storage_path: None,
            max_file_copy_size_bytes: 100 * 1024 * 1024,
            max_screenshot_size_bytes: 50 * 1024 * 1024,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OcrConfig {
    pub engine: String,
    pub tesseract_languages: String,
    pub models_dir: Option<PathBuf>,
    pub ppocr_model_path: Option<String>,
    pub ppocr_model_variant: String,
    pub det_score_threshold: f32,
    pub det_box_threshold: f32,
    pub det_unclip_ratio: f32,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            engine: "ppocr".to_string(),
            tesseract_languages: "chi_sim+eng".to_string(),
            models_dir: None,
            ppocr_model_path: None,
            ppocr_model_variant: "small".to_string(),
            det_score_threshold: 0.3,
            det_box_threshold: 0.6,
            det_unclip_ratio: 1.5,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HistoryConfig {
    pub max_items: u32,
    pub retention_days: u32,
    pub favorites_exempt: bool,
    pub recycle_bin_days: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_items: 10_000,
            retention_days: 30,
            favorites_exempt: true,
            recycle_bin_days: 7,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PrivacyConfig {
    pub local_only: bool,
    pub telemetry_enabled: bool,
    pub capture_sensitive_sources: bool,
    pub ignored_applications: Vec<String>,
    pub paused: bool,
    pub master_password_hash: Option<String>,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            local_only: true,
            telemetry_enabled: false,
            capture_sensitive_sources: false,
            ignored_applications: vec![
                "1Password".to_owned(),
                "Bitwarden".to_owned(),
                "KeePass".to_owned(),
                "KeePassXC".to_owned(),
            ],
            paused: false,
            master_password_hash: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PermissionConfig {
    pub request_accessibility_on_demand: bool,
    pub allow_network_access: bool,
    pub start_at_login: bool,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            request_accessibility_on_demand: true,
            allow_network_access: false,
            start_at_login: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowConfig {
    pub launch_at_startup: bool,
    pub close_to_tray: bool,
    pub single_instance: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            launch_at_startup: false,
            close_to_tray: true,
            single_instance: true,
            x: None,
            y: None,
            width: None,
            height: None,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExportConfig {
    pub schedule_auto_export: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub struct ConfigStore {
    pub(crate) path: PathBuf,
    pub(crate) config: AppConfig,
    pub(crate) general_settings_present: bool,
}
