use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::StorageError;

const CONFIG_DIRECTORY_NAME: &str = "conf";
const CONFIG_FILE_NAME: &str = "conf.json";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    pub storage: StorageConfig,
    pub history: HistoryConfig,
    pub privacy: PrivacyConfig,
    pub permissions: PermissionConfig,
    pub window: WindowConfig,
    pub export: ExportConfig,
    pub ocr: OcrConfig,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
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
    pub ppocr_model_path: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            engine: "ppocr".to_string(),
            tesseract_languages: "chi_sim+eng".to_string(),
            ppocr_model_path: None,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExportConfig {
    pub schedule_auto_export: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            schedule_auto_export: None,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
    config: AppConfig,
}

impl ConfigStore {
    pub fn load(project_directory: &Path) -> Result<Self, StorageError> {
        let config_directory = project_directory.join(CONFIG_DIRECTORY_NAME);
        fs::create_dir_all(&config_directory)?;
        let path = config_directory.join(CONFIG_FILE_NAME);
        let config = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            AppConfig::default()
        };
        let store = Self { path, config };

        if !store.path.exists() {
            store.save()?;
        }

        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn storage_directory(&self) -> Option<&Path> {
        self.config.storage.data_directory.as_deref()
    }

    pub fn set_storage_directory(
        &mut self,
        data_directory: Option<PathBuf>,
    ) -> Result<(), StorageError> {
        self.config.storage.data_directory = data_directory;
        self.save()
    }

    pub fn ignored_applications(&self) -> &[String] {
        &self.config.privacy.ignored_applications
    }

    pub fn set_ignored_applications(
        &mut self,
        applications: Vec<String>,
    ) -> Result<Vec<String>, StorageError> {
        let mut normalized = BTreeMap::new();
        for application in applications {
            let application = application.trim();
            if application.is_empty() {
                continue;
            }
            normalized
                .entry(application.to_lowercase())
                .or_insert_with(|| application.to_owned());
        }

        let applications = normalized.into_values().collect::<Vec<_>>();
        self.config.privacy.ignored_applications = applications.clone();
        self.save()?;
        Ok(applications)
    }

    pub fn launch_at_startup(&self) -> bool {
        self.config.window.launch_at_startup
    }

    pub fn set_launch_at_startup(&mut self, value: bool) -> Result<(), StorageError> {
        self.config.window.launch_at_startup = value;
        self.save()
    }

    pub fn close_to_tray(&self) -> bool {
        self.config.window.close_to_tray
    }

    pub fn set_close_to_tray(&mut self, value: bool) -> Result<(), StorageError> {
        self.config.window.close_to_tray = value;
        self.save()
    }

    pub fn single_instance(&self) -> bool {
        self.config.window.single_instance
    }

    pub fn set_single_instance(&mut self, value: bool) -> Result<(), StorageError> {
        self.config.window.single_instance = value;
        self.save()
    }

    pub fn window_position(&self) -> Option<(i32, i32, u32, u32)> {
        let x = self.config.window.x?;
        let y = self.config.window.y?;
        let width = self.config.window.width?;
        let height = self.config.window.height?;
        Some((x, y, width, height))
    }

    pub fn set_window_position(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), StorageError> {
        self.config.window.x = Some(x);
        self.config.window.y = Some(y);
        self.config.window.width = Some(width);
        self.config.window.height = Some(height);
        self.save()
    }

    pub fn privacy_paused(&self) -> bool {
        self.config.privacy.paused
    }

    pub fn set_privacy_paused(&mut self, value: bool) -> Result<(), StorageError> {
        self.config.privacy.paused = value;
        self.save()
    }

    pub fn privacy_master_password_hash(&self) -> Option<&str> {
        self.config.privacy.master_password_hash.as_deref()
    }

    pub fn set_privacy_master_password_hash(
        &mut self,
        value: Option<String>,
    ) -> Result<(), StorageError> {
        self.config.privacy.master_password_hash = value;
        self.save()
    }

    pub fn schedule_auto_export(&self) -> Option<&str> {
        self.config.export.schedule_auto_export.as_deref()
    }

    pub fn set_schedule_auto_export(
        &mut self,
        value: Option<String>,
    ) -> Result<(), StorageError> {
        self.config.export.schedule_auto_export = value;
        self.save()
    }

    pub fn ocr_engine(&self) -> &str {
        &self.config.ocr.engine
    }

    pub fn set_ocr_engine(&mut self, value: String) -> Result<(), StorageError> {
        self.config.ocr.engine = value;
        self.save()
    }

    pub fn tesseract_languages(&self) -> &str {
        &self.config.ocr.tesseract_languages
    }

    pub fn image_storage_path(&self) -> Option<&Path> {
        self.config.storage.image_storage_path.as_deref()
    }

    pub fn file_storage_path(&self) -> Option<&Path> {
        self.config.storage.file_storage_path.as_deref()
    }

    pub fn max_file_copy_size_bytes(&self) -> u64 {
        self.config.storage.max_file_copy_size_bytes
    }

    pub fn max_screenshot_size_bytes(&self) -> u64 {
        self.config.storage.max_screenshot_size_bytes
    }

    pub fn retention_days(&self) -> u32 {
        self.config.history.retention_days
    }

    pub fn max_items(&self) -> u32 {
        self.config.history.max_items
    }

    pub fn recycle_bin_days(&self) -> u32 {
        self.config.history.recycle_bin_days
    }

    pub fn set_retention_days(&mut self, value: u32) -> Result<(), StorageError> {
        self.config.history.retention_days = value;
        self.save()
    }

    pub fn set_max_items(&mut self, value: u32) -> Result<(), StorageError> {
        self.config.history.max_items = value;
        self.save()
    }

    pub fn set_recycle_bin_days(&mut self, value: u32) -> Result<(), StorageError> {
        self.config.history.recycle_bin_days = value;
        self.save()
    }

    pub fn set_max_file_copy_size_bytes(&mut self, value: u64) -> Result<(), StorageError> {
        self.config.storage.max_file_copy_size_bytes = value;
        self.save()
    }

    fn save(&self) -> Result<(), StorageError> {
        fs::write(&self.path, serde_json::to_vec_pretty(&self.config)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use serde_json::{json, Value};

    use super::ConfigStore;

    #[test]
    fn creates_the_single_project_configuration_file() {
        let project = temporary_test_directory("default");

        let store = ConfigStore::load(&project).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();

        assert_eq!(store.path(), project.join("conf/conf.json"));
        assert_eq!(saved["storage"]["dataDirectory"], Value::Null);
        assert_eq!(saved["storage"]["maxFileCopySizeBytes"], 104_857_600);
        assert_eq!(saved["storage"]["maxScreenshotSizeBytes"], 52_428_800);
        assert_eq!(saved["history"]["maxItems"], 10_000);
        assert_eq!(saved["history"]["retentionDays"], 30);
        assert_eq!(saved["history"]["favoritesExempt"], true);
        assert_eq!(saved["history"]["recycleBinDays"], 7);
        assert_eq!(saved["privacy"]["localOnly"], true);
        assert_eq!(saved["privacy"]["telemetryEnabled"], false);
        assert_eq!(saved["privacy"]["paused"], false);
        assert_eq!(saved["permissions"]["allowNetworkAccess"], false);
        assert_eq!(saved["window"]["launchAtStartup"], false);
        assert_eq!(saved["window"]["closeToTray"], true);
        assert_eq!(saved["window"]["singleInstance"], true);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn preserves_other_settings_when_the_storage_directory_changes() {
        let project = temporary_test_directory("preserve");
        let config_directory = project.join("conf");
        let custom_data = project.join("custom-data");
        fs::create_dir_all(&config_directory).unwrap();
        fs::write(
            config_directory.join("conf.json"),
            serde_json::to_vec_pretty(&json!({
                "storage": {
                    "dataDirectory": null,
                    "copySizeLimitMb": 128
                },
                "window": {
                    "useSystemTitlebar": false
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let mut store = ConfigStore::load(&project).unwrap();

        store
            .set_storage_directory(Some(custom_data.clone()))
            .unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();

        assert_eq!(store.storage_directory(), Some(custom_data.as_path()));
        assert_eq!(saved["storage"]["copySizeLimitMb"], 128);
        assert_eq!(saved["window"]["useSystemTitlebar"], false);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn normalizes_and_persists_ignored_applications() {
        let project = temporary_test_directory("ignored-apps");
        let mut store = ConfigStore::load(&project).unwrap();

        let saved = store
            .set_ignored_applications(vec![
                " KeePass ".to_owned(),
                "keepass".to_owned(),
                "Bitwarden".to_owned(),
                "".to_owned(),
            ])
            .unwrap();
        let persisted: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();

        assert_eq!(saved, vec!["Bitwarden", "KeePass"]);
        assert_eq!(store.ignored_applications(), saved);
        assert_eq!(
            persisted["privacy"]["ignoredApplications"],
            json!(["Bitwarden", "KeePass"])
        );
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn persists_window_position() {
        let project = temporary_test_directory("window-pos");
        let mut store = ConfigStore::load(&project).unwrap();

        store.set_window_position(100, 200, 800, 600).unwrap();

        assert_eq!(store.window_position(), Some((100, 200, 800, 600)));

        let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert_eq!(saved["window"]["x"], 100);
        assert_eq!(saved["window"]["y"], 200);
        assert_eq!(saved["window"]["width"], 800);
        assert_eq!(saved["window"]["height"], 600);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn persists_privacy_settings() {
        let project = temporary_test_directory("privacy");
        let mut store = ConfigStore::load(&project).unwrap();

        assert!(!store.privacy_paused());

        store.set_privacy_paused(true).unwrap();
        assert!(store.privacy_paused());

        store
            .set_privacy_master_password_hash(Some("hash123".to_owned()))
            .unwrap();
        assert_eq!(store.privacy_master_password_hash(), Some("hash123"));

        let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert_eq!(saved["privacy"]["paused"], true);
        assert_eq!(saved["privacy"]["masterPasswordHash"], "hash123");
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn persists_export_settings() {
        let project = temporary_test_directory("export");
        let mut store = ConfigStore::load(&project).unwrap();

        assert!(store.schedule_auto_export().is_none());

        store
            .set_schedule_auto_export(Some("0 */6 * * *".to_owned()))
            .unwrap();
        assert_eq!(
            store.schedule_auto_export(),
            Some("0 */6 * * *")
        );

        store.set_schedule_auto_export(None).unwrap();
        assert!(store.schedule_auto_export().is_none());
        fs::remove_dir_all(project).unwrap();
    }

    fn temporary_test_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "clipboard-config-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
