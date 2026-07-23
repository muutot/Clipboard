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
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StorageConfig {
    pub data_directory: Option<PathBuf>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HistoryConfig {
    pub max_items: u32,
    pub retention_days: u32,
    pub favorites_exempt: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_items: 10_000,
            retention_days: 30,
            favorites_exempt: true,
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
        assert_eq!(saved["history"]["maxItems"], 10_000);
        assert_eq!(saved["history"]["retentionDays"], 30);
        assert_eq!(saved["history"]["favoritesExempt"], true);
        assert_eq!(saved["privacy"]["localOnly"], true);
        assert_eq!(saved["privacy"]["telemetryEnabled"], false);
        assert_eq!(saved["permissions"]["allowNetworkAccess"], false);
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
