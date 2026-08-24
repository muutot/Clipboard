use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::storage::StorageError;

use super::types::*;

impl ConfigStore {
    pub fn load(project_directory: &Path) -> Result<Self, StorageError> {
        let config_directory = project_directory.join(CONFIG_DIRECTORY_NAME);
        fs::create_dir_all(&config_directory)?;
        let path = config_directory.join(CONFIG_FILE_NAME);
        let (config, general_settings_present) = if path.exists() {
            let bytes = fs::read(&path)?;
            match Self::parse_saved_config(&bytes) {
                Ok(parsed) => parsed,
                Err(error) => {
                    // A truncated or otherwise unparsable file must not leave
                    // the app unable to start. Quarantine the corrupt bytes
                    // for inspection and continue with defaults.
                    let stamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_secs())
                        .unwrap_or(0);
                    let quarantined =
                        config_directory.join(format!("{CONFIG_FILE_NAME}.corrupt-{stamp}"));
                    crate::log_event!(
                        "[config] {} is unreadable ({error}); quarantining it as {} and starting with defaults",
                        path.display(),
                        quarantined.display()
                    );
                    let _ = fs::rename(&path, &quarantined);
                    (AppConfig::default(), true)
                }
            }
        } else {
            (AppConfig::default(), true)
        };
        let store = Self {
            path,
            config,
            general_settings_present,
        };

        if !store.path.exists() {
            store.save()?;
        }

        Ok(store)
    }

    fn parse_saved_config(bytes: &[u8]) -> Result<(AppConfig, bool), StorageError> {
        let mut raw: Value = serde_json::from_slice(bytes)?;
        let general_settings_present = raw.get("general").map(Value::is_object).unwrap_or(false);

        if !general_settings_present {
            if let Value::Object(object) = &mut raw {
                object.remove("general");
            }
        }

        Ok((serde_json::from_value(raw)?, general_settings_present))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn general_settings(&self) -> &GeneralConfig {
        &self.config.general
    }

    pub fn has_general_settings(&self) -> bool {
        self.general_settings_present
    }

    pub fn set_general_settings(&mut self, settings: GeneralConfig) -> Result<(), StorageError> {
        let previous_settings = self.config.general.clone();
        let previous_present = self.general_settings_present;
        self.config.general = settings;
        self.general_settings_present = true;

        if let Err(error) = self.save() {
            self.config.general = previous_settings;
            self.general_settings_present = previous_present;
            return Err(error);
        }

        Ok(())
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

    pub fn set_resource_storage_paths(
        &mut self,
        image_storage_path: Option<PathBuf>,
        file_storage_path: Option<PathBuf>,
    ) -> Result<(), StorageError> {
        validate_resource_directory("storage.imageStoragePath", image_storage_path.as_deref())?;
        validate_resource_directory("storage.fileStoragePath", file_storage_path.as_deref())?;

        if let (Some(image), Some(file)) = (&image_storage_path, &file_storage_path) {
            let same_path = fs::canonicalize(image).unwrap_or_else(|_| image.clone())
                == fs::canonicalize(file).unwrap_or_else(|_| file.clone());
            if same_path {
                return Err(StorageError::ResourceDirectoriesMustBeDistinct);
            }
        }

        self.config.storage.image_storage_path = image_storage_path;
        self.config.storage.file_storage_path = file_storage_path;
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

    /// When local-only mode is on, the app must not make outbound network
    /// requests on its own (update checks and similar background calls).
    pub fn privacy_local_only(&self) -> bool {
        self.config.privacy.local_only
    }

    pub fn set_privacy_local_only(&mut self, value: bool) -> Result<(), StorageError> {
        self.config.privacy.local_only = value;
        self.save()
    }

    /// When true, content copied from password managers and other sensitive
    /// source apps is still recorded. Explicitly ignored applications keep
    /// being skipped either way.
    pub fn privacy_capture_sensitive_sources(&self) -> bool {
        self.config.privacy.capture_sensitive_sources
    }

    pub fn set_privacy_capture_sensitive_sources(
        &mut self,
        value: bool,
    ) -> Result<(), StorageError> {
        self.config.privacy.capture_sensitive_sources = value;
        self.save()
    }

    pub fn sensitive_patterns(&self) -> &[String] {
        &self.config.privacy.sensitive_patterns
    }

    /// Persists the sensitive-content pattern list after trimming blanks and
    /// dropping duplicates (first occurrence wins). Invalid regexes are kept
    /// as-is here; callers validate before persisting.
    pub fn set_sensitive_patterns(
        &mut self,
        patterns: Vec<String>,
    ) -> Result<Vec<String>, StorageError> {
        let mut normalized: Vec<String> = Vec::new();
        for pattern in patterns {
            let pattern = pattern.trim();
            if pattern.is_empty() || normalized.iter().any(|existing| existing == pattern) {
                continue;
            }
            normalized.push(pattern.to_owned());
        }

        self.config.privacy.sensitive_patterns = normalized.clone();
        self.save()?;
        Ok(normalized)
    }

    pub fn schedule_auto_export(&self) -> Option<&str> {
        self.config.export.schedule_auto_export.as_deref()
    }

    pub fn set_schedule_auto_export(&mut self, value: Option<String>) -> Result<(), StorageError> {
        self.config.export.schedule_auto_export = value;
        self.save()
    }

    pub fn ocr_engine(&self) -> &str {
        &self.config.ocr.engine
    }

    pub fn tesseract_languages(&self) -> &str {
        &self.config.ocr.tesseract_languages
    }

    pub fn models_dir(&self) -> Option<&Path> {
        self.config.ocr.models_dir.as_deref()
    }

    pub fn set_models_dir(&mut self, value: Option<PathBuf>) -> Result<(), StorageError> {
        self.config.ocr.models_dir = value;
        self.save()
    }

    pub fn det_score_threshold(&self) -> f32 {
        self.config.ocr.det_score_threshold
    }

    pub fn ppocr_model_variant(&self) -> &str {
        &self.config.ocr.ppocr_model_variant
    }

    pub fn set_ocr_settings(
        &mut self,
        engine: String,
        ppocr_model_variant: String,
        score_threshold: f32,
        box_threshold: f32,
        unclip_ratio: f32,
    ) -> Result<(), StorageError> {
        let previous = self.config.ocr.clone();
        self.config.ocr.engine = engine;
        self.config.ocr.ppocr_model_variant = ppocr_model_variant;
        self.config.ocr.det_score_threshold = score_threshold;
        self.config.ocr.det_box_threshold = box_threshold;
        self.config.ocr.det_unclip_ratio = unclip_ratio;

        if let Err(error) = self.save() {
            self.config.ocr = previous;
            return Err(error);
        }

        Ok(())
    }

    pub fn page_size_limit(&self) -> u32 {
        self.config.general.page_size_limit.clamp(500, 6_000)
    }

    pub fn search_page_size_limit(&self) -> u32 {
        self.config.general.search_page_size_limit.clamp(50, 1_000)
    }

    pub fn set_page_size_limit(&mut self, value: u32) -> Result<(), StorageError> {
        self.config.general.page_size_limit = value.clamp(500, 6_000);
        self.save()
    }

    pub fn set_search_page_size_limit(&mut self, value: u32) -> Result<(), StorageError> {
        self.config.general.search_page_size_limit = value.clamp(50, 1_000);
        self.save()
    }

    pub fn search_index_sync_mode(&self) -> SearchIndexSyncMode {
        SearchIndexSyncMode::from_str(&self.config.general.search_index_sync_mode)
            .expect("SearchIndexSyncMode::from_str is infallible")
    }

    pub fn update_source(&self) -> UpdateSource {
        UpdateSource::from_str(&self.config.general.update_source)
            .expect("UpdateSource::from_str is infallible")
    }

    pub fn max_text_capture_bytes(&self) -> u64 {
        self.config
            .general
            .max_text_capture_bytes
            .clamp(10_000, 10_000_000)
    }

    pub fn det_box_threshold(&self) -> f32 {
        self.config.ocr.det_box_threshold
    }

    pub fn det_unclip_ratio(&self) -> f32 {
        self.config.ocr.det_unclip_ratio
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

    pub fn sync_config(&self) -> SyncConfig {
        let mut sync = self.config.sync.clone();
        // Secrets are stored as DPAPI envelopes; decrypt on read so callers
        // (S3 client, retain_secret) always work with plaintext in memory.
        if let Some(stored) = &sync.s3_secret_key {
            if let Some(plain) = crate::platform::dpapi::unprotect(stored) {
                sync.s3_secret_key = Some(plain);
            }
        }
        if let Some(stored) = &sync.sync_password {
            if let Some(plain) = crate::platform::dpapi::unprotect(stored) {
                sync.sync_password = Some(plain);
            }
        }
        sync
    }

    pub fn set_sync_config(&mut self, mut sync: SyncConfig) -> Result<(), StorageError> {
        // Encrypt secrets before they reach disk. Failure to protect (e.g.
        // non-Windows) keeps the previous plaintext behavior; legacy plaintext
        // values are upgraded on their next save.
        if let Some(secret) = &sync.s3_secret_key {
            if !secret.is_empty() && !crate::platform::dpapi::is_envelope(secret) {
                if let Some(protected) = crate::platform::dpapi::protect(secret) {
                    sync.s3_secret_key = Some(protected);
                }
            }
        }
        if let Some(password) = &sync.sync_password {
            if !password.is_empty() && !crate::platform::dpapi::is_envelope(password) {
                if let Some(protected) = crate::platform::dpapi::protect(password) {
                    sync.sync_password = Some(protected);
                }
            }
        }
        let previous = self.config.sync.clone();
        self.config.sync = sync;
        if let Err(error) = self.save() {
            self.config.sync = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn auto_sync(&self) -> bool {
        self.config.sync.auto_sync
    }

    pub fn set_auto_sync(&mut self, enabled: bool) -> Result<(), StorageError> {
        self.config.sync.auto_sync = enabled;
        self.save()
    }

    pub fn auto_sync_interval_secs(&self) -> u64 {
        self.config.sync.auto_sync_interval_secs.max(10)
    }

    pub fn set_auto_sync_interval_secs(&mut self, value: u64) -> Result<(), StorageError> {
        self.config.sync.auto_sync_interval_secs = value.clamp(10, 86400);
        self.save()
    }

    pub fn max_sync_image_bytes(&self) -> u64 {
        self.config.sync.max_sync_image_bytes
    }

    pub fn set_max_sync_image_bytes(&mut self, value: u64) -> Result<(), StorageError> {
        self.config.sync.max_sync_image_bytes = value;
        self.save()
    }

    pub fn max_sync_file_bytes(&self) -> u64 {
        self.config.sync.max_sync_file_bytes
    }

    pub fn set_max_sync_file_bytes(&mut self, value: u64) -> Result<(), StorageError> {
        self.config.sync.max_sync_file_bytes = value;
        self.save()
    }

    pub fn s3_region(&self) -> String {
        self.config
            .sync
            .s3_region
            .clone()
            .unwrap_or_else(|| "us-east-1".to_string())
    }

    pub fn s3_bucket(&self) -> Option<String> {
        self.config.sync.s3_bucket.clone()
    }

    pub fn s3_access_key(&self) -> Option<String> {
        self.config.sync.s3_access_key.clone()
    }

    pub fn s3_secret_key(&self) -> Option<String> {
        self.config.sync.s3_secret_key.clone()
    }

    pub fn update_sync_status(
        &mut self,
        status: &str,
        timestamp_ms: i64,
    ) -> Result<(), StorageError> {
        self.config.sync.last_sync_ms = Some(timestamp_ms);
        self.config.sync.last_sync_status = Some(status.to_string());
        self.save()
    }

    fn save(&self) -> Result<(), StorageError> {
        let mut value = serde_json::to_value(&self.config)?;
        if !self.general_settings_present {
            if let Value::Object(object) = &mut value {
                object.remove("general");
            }
        }
        atomic_write_json(&self.path, serde_json::to_vec_pretty(&value)?)
    }
}

fn atomic_write_json(path: &Path, contents: Vec<u8>) -> Result<(), StorageError> {
    use std::io::Write;

    let directory = path.parent().ok_or_else(|| {
        StorageError::Io(std::io::Error::other("config file has no parent directory"))
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| StorageError::Io(std::io::Error::other("config file has no file name")))?;
    let temporary_path = directory.join(format!(".{}.tmp", file_name.to_string_lossy()));

    let result = (|| -> Result<(), StorageError> {
        let mut file = fs::File::create(&temporary_path)?;
        file.write_all(&contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary_path, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn validate_resource_directory(
    field: &'static str,
    path: Option<&Path>,
) -> Result<(), StorageError> {
    if let Some(path) = path.filter(|path| !path.is_absolute()) {
        return Err(StorageError::ResourceDirectoryMustBeAbsolute {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}
