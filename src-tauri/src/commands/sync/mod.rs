use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};

use crate::config::{ConfigStore, SyncConfig, SyncProvider};
use crate::storage::{Database, StoragePaths};
use crate::sync::{self, v1};

mod auto;

/// Manual and automatic runs share one local publication state and therefore
/// cannot overlap. A second caller fails fast instead of queueing behind a
/// potentially long S3 transfer.
static SYNC_RUN_LOCK: Mutex<()> = Mutex::new(());

pub(crate) use auto::AutoSyncWorker;

const DEFAULT_S3_REGION: &str = "us-east-1";
const MAX_SYNC_ICON_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfigInfo {
    provider: String,
    endpoint: Option<String>,
    remote_path: Option<String>,
    s3_region: Option<String>,
    s3_bucket: Option<String>,
    s3_access_key: Option<String>,
    has_s3_secret_key: bool,
    has_sync_password: bool,
    last_sync_ms: Option<i64>,
    last_sync_status: Option<String>,
    pending_entries: u64,
    auto_sync: bool,
    auto_sync_interval_secs: u64,
    segment_max_entries: u32,
    max_sync_image_bytes: u64,
    max_sync_file_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncRunResult {
    pub uploaded_entries: u64,
    pub downloaded_entries: u64,
    pub applied_entries: u64,
    pub failed_peers: u64,
    pub uploaded_resources: u64,
    pub downloaded_resources: u64,
    pub deleted_remote_objects: u64,
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
}

impl From<v1::SyncEngineResult> for SyncRunResult {
    fn from(result: v1::SyncEngineResult) -> Self {
        Self {
            uploaded_entries: result.uploaded_entries,
            downloaded_entries: result.downloaded_entries,
            applied_entries: result.applied_entries,
            failed_peers: result.failed_peers,
            uploaded_resources: result.uploaded_resources,
            downloaded_resources: result.downloaded_resources,
            deleted_remote_objects: result.deleted_remote_objects,
            bytes_uploaded: result.bytes_uploaded,
            bytes_downloaded: result.bytes_downloaded,
        }
    }
}

/// Immutable snapshot of every value needed by one network run. The config
/// mutex is released before key derivation, hashing, SQLite export, or S3 I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncSettings {
    endpoint: String,
    remote_path: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    sync_password: Option<String>,
    segment_max_entries: usize,
    max_sync_image_bytes: u64,
    max_sync_file_bytes: u64,
}

impl SyncSettings {
    fn from_config(config: &ConfigStore) -> Result<Self, String> {
        Self::from_sync_config(&config.sync_config())
    }

    fn from_sync_config(sync: &SyncConfig) -> Result<Self, String> {
        if sync.provider != SyncProvider::S3 {
            return Err("S3 sync is disabled".to_string());
        }
        Ok(Self {
            endpoint: required_value(sync.endpoint.as_deref(), "S3 endpoint")?,
            remote_path: sync
                .remote_path
                .as_deref()
                .unwrap_or_default()
                .trim_matches('/')
                .to_string(),
            region: sync
                .s3_region
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(DEFAULT_S3_REGION)
                .to_string(),
            bucket: required_value(sync.s3_bucket.as_deref(), "S3 bucket")?,
            access_key: required_value(sync.s3_access_key.as_deref(), "S3 access key")?,
            secret_key: required_value(sync.s3_secret_key.as_deref(), "S3 secret key")?,
            sync_password: sync
                .sync_password
                .clone()
                .filter(|password| !password.is_empty()),
            segment_max_entries: sync.segment_max_entries.clamp(16, 10_000) as usize,
            max_sync_image_bytes: sync.max_sync_image_bytes,
            max_sync_file_bytes: sync.max_sync_file_bytes,
        })
    }

    fn remote_scope_id(&self) -> String {
        let endpoint = self.endpoint.trim().trim_end_matches('/');
        let identity = format!(
            "clipboard-sync-v1\n{endpoint}\n{}\n{}\n{}\n{}",
            self.region, self.bucket, self.remote_path, self.access_key
        );
        hex::encode(Sha256::digest(identity.as_bytes()))
    }

    fn object_store(&self) -> Result<v1::S3ObjectStore, String> {
        v1::S3ObjectStore::new(
            self.endpoint.clone(),
            self.region.clone(),
            self.bucket.clone(),
            self.access_key.clone(),
            self.secret_key.clone(),
            &self.remote_path,
        )
    }

    fn session_key(&self, remote_scope: &str) -> Result<Option<v1::SessionKey>, String> {
        self.sync_password
            .as_deref()
            .map(|password| v1::SessionKey::derive(password, remote_scope))
            .transpose()
    }

    fn engine_options(&self) -> v1::SyncEngineOptions {
        v1::SyncEngineOptions {
            segment_max_entries: self.segment_max_entries,
            resource_limits: v1::ResourceLimits {
                image_bytes: self.max_sync_image_bytes,
                file_bytes: self.max_sync_file_bytes,
                icon_bytes: MAX_SYNC_ICON_BYTES,
            },
        }
    }
}

#[tauri::command]
pub fn get_sync_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    database: tauri::State<'_, Database>,
) -> Result<SyncConfigInfo, String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_string())?;
    let sync = guard.sync_config();
    Ok(SyncConfigInfo {
        provider: match sync.provider {
            SyncProvider::Off => "off",
            SyncProvider::S3 => "s3",
        }
        .to_string(),
        endpoint: sync.endpoint,
        remote_path: sync.remote_path,
        s3_region: sync.s3_region,
        s3_bucket: sync.s3_bucket,
        s3_access_key: sync.s3_access_key,
        has_s3_secret_key: sync.s3_secret_key.is_some(),
        has_sync_password: sync.sync_password.is_some(),
        last_sync_ms: sync.last_sync_ms,
        last_sync_status: sync.last_sync_status,
        pending_entries: database.count_sync_outbox().unwrap_or(0),
        auto_sync: sync.auto_sync,
        auto_sync_interval_secs: sync.auto_sync_interval_secs,
        segment_max_entries: sync.segment_max_entries,
        max_sync_image_bytes: sync.max_sync_image_bytes,
        max_sync_file_bytes: sync.max_sync_file_bytes,
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_sync_config(
    provider: String,
    endpoint: Option<String>,
    remote_path: Option<String>,
    auto_sync: bool,
    auto_sync_interval_secs: u64,
    segment_max_entries: u32,
    max_sync_image_bytes: u64,
    max_sync_file_bytes: u64,
    s3_region: Option<String>,
    s3_bucket: Option<String>,
    s3_access_key: Option<String>,
    s3_secret_key: Option<String>,
    sync_password: Option<String>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<(), String> {
    let mut guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_string())?;
    let previous = guard.sync_config();
    let sync = SyncConfig {
        provider: if provider == "s3" {
            SyncProvider::S3
        } else {
            SyncProvider::Off
        },
        endpoint: normalized_optional(endpoint),
        remote_path: normalized_optional(remote_path),
        s3_region: normalized_optional(s3_region),
        s3_bucket: normalized_optional(s3_bucket),
        s3_access_key: normalized_optional(s3_access_key),
        s3_secret_key: retain_secret(s3_secret_key, previous.s3_secret_key),
        sync_password: retain_secret(sync_password, previous.sync_password),
        last_sync_ms: previous.last_sync_ms,
        last_sync_status: previous.last_sync_status,
        auto_sync,
        auto_sync_interval_secs: auto_sync_interval_secs.clamp(10, 86_400),
        segment_max_entries: segment_max_entries.clamp(16, 10_000),
        max_sync_image_bytes,
        max_sync_file_bytes,
    };
    guard
        .set_sync_config(sync)
        .map_err(|error| error.to_string())
}

/// Tests the persisted S3 settings. The settings UI saves its current fields
/// before invoking this command, so write-only secrets never cross back to the
/// frontend after initial entry.
#[tauri::command]
pub fn test_sync_connection(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<sync::S3TestResult, String> {
    let settings = {
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_string())?;
        SyncSettings::from_config(&guard)?
    };
    settings.object_store()?;
    Ok(sync::test_s3_connection(
        &settings.endpoint,
        &settings.region,
        &settings.bucket,
        &settings.access_key,
        &settings.secret_key,
    ))
}

#[tauri::command]
pub fn sync_now(app: tauri::AppHandle) -> Result<SyncRunResult, String> {
    run_sync(&app)
}

/// Shared entry point for the Tauri command and the auto-sync worker.
pub(super) fn run_sync(app: &tauri::AppHandle) -> Result<SyncRunResult, String> {
    let _run_guard = SYNC_RUN_LOCK
        .try_lock()
        .map_err(|_| "sync already in progress".to_string())?;
    let config = app.state::<Mutex<ConfigStore>>();
    let database = app.state::<Database>();
    let paths = app.state::<StoragePaths>();

    let settings = {
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_string())?;
        SyncSettings::from_config(&guard)?
    };
    let remote_scope = settings.remote_scope_id();
    let session_key = settings.session_key(&remote_scope)?;
    let store = settings.object_store()?;

    let outcome = v1::sync_database(
        &store,
        &database,
        &paths,
        &remote_scope,
        session_key.as_ref(),
        settings.engine_options(),
    );
    let now_ms = current_time_ms();
    match outcome {
        Ok(engine_result) => {
            if engine_result.applied_entries > 0 {
                let _ = app.emit(
                    "clipboard-history-invalidated",
                    crate::commands::clipboard::ClipboardHistoryInvalidated {
                        deleted_ids: Vec::new(),
                    },
                );
            }
            if let Ok(mut guard) = config.lock() {
                let status = if engine_result.failed_peers == 0 {
                    "success"
                } else {
                    "partial"
                };
                let _ = guard.update_sync_status(status, now_ms);
            }
            Ok(engine_result.into())
        }
        Err(error) => {
            if let Ok(mut guard) = config.lock() {
                let _ = guard.update_sync_status("failed", now_ms);
            }
            Err(error)
        }
    }
}

fn required_value(value: Option<&str>, label: &str) -> Result<String, String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{label} is not configured"))
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn retain_secret(candidate: Option<String>, previous: Option<String>) -> Option<String> {
    candidate.filter(|value| !value.is_empty()).or(previous)
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_sync() -> SyncConfig {
        SyncConfig {
            provider: SyncProvider::S3,
            endpoint: Some("http://127.0.0.1:9000/".to_string()),
            remote_path: Some("/clipboard/".to_string()),
            s3_region: Some("us-east-1".to_string()),
            s3_bucket: Some("clipboard".to_string()),
            s3_access_key: Some("access".to_string()),
            s3_secret_key: Some("secret".to_string()),
            sync_password: None,
            last_sync_ms: None,
            last_sync_status: None,
            auto_sync: false,
            auto_sync_interval_secs: 300,
            segment_max_entries: 512,
            max_sync_image_bytes: 5_242_880,
            max_sync_file_bytes: 10_485_760,
        }
    }

    #[test]
    fn remote_scope_is_stable_and_isolates_prefixes() {
        let first = SyncSettings::from_sync_config(&configured_sync()).unwrap();
        let mut equivalent_config = configured_sync();
        equivalent_config.endpoint = Some("http://127.0.0.1:9000".to_string());
        equivalent_config.remote_path = Some("clipboard".to_string());
        let equivalent = SyncSettings::from_sync_config(&equivalent_config).unwrap();
        let mut other_config = configured_sync();
        other_config.remote_path = Some("other".to_string());
        let other = SyncSettings::from_sync_config(&other_config).unwrap();

        assert_eq!(first.remote_scope_id(), equivalent.remote_scope_id());
        assert_ne!(first.remote_scope_id(), other.remote_scope_id());
        assert_eq!(first.remote_scope_id().len(), 64);
    }

    #[test]
    fn disabled_or_incomplete_s3_config_is_rejected() {
        let mut sync = configured_sync();
        sync.provider = SyncProvider::Off;
        assert!(SyncSettings::from_sync_config(&sync).is_err());

        sync.provider = SyncProvider::S3;
        sync.s3_secret_key = None;
        assert!(SyncSettings::from_sync_config(&sync).is_err());
    }

    #[test]
    fn omitted_write_only_secret_keeps_the_stored_value() {
        assert_eq!(
            retain_secret(None, Some("stored".to_string())),
            Some("stored".to_string())
        );
        assert_eq!(
            retain_secret(Some("replacement".to_string()), Some("stored".to_string())),
            Some("replacement".to_string())
        );
    }

    #[test]
    fn run_result_uses_v1_object_and_resource_counters() {
        let value = serde_json::to_value(SyncRunResult::from(v1::SyncEngineResult {
            uploaded_entries: 2,
            downloaded_entries: 3,
            applied_entries: 1,
            failed_peers: 9,
            uploaded_resources: 4,
            downloaded_resources: 5,
            deleted_remote_objects: 6,
            bytes_uploaded: 7,
            bytes_downloaded: 8,
        }))
        .unwrap();

        assert_eq!(value["uploadedResources"], 4);
        assert_eq!(value["deletedRemoteObjects"], 6);
        assert_eq!(value["failedPeers"], 9);
    }
}
