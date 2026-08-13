use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};

use crate::config::{ConfigStore, SyncConfig, SyncProvider};
use crate::content::ThumbnailWorker;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::storage::{ClipboardRepository, Database, StoragePaths};
use crate::sync::{self, v1};

mod auto;

/// Manual and automatic runs share one local publication state and therefore
/// cannot overlap. A second caller fails fast instead of queueing behind a
/// potentially long S3 transfer.
static SYNC_RUN_LOCK: Mutex<()> = Mutex::new(());

// One lock per content-addressed object prevents two cards opened at the same
// time from downloading the same S3 blob twice. The map is intentionally
// process-local; the verified cache file is the cross-process coordination
// point and materialize_resource re-checks it before every download.
static RESOURCE_MATERIALIZATION_LOCKS: OnceLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> =
    OnceLock::new();

pub(crate) use auto::AutoSyncWorker;

const DEFAULT_S3_REGION: &str = "us-east-1";
const MAX_SYNC_ICON_BYTES: u64 = 1024 * 1024;

fn resource_materialization_lock(key: &str) -> Result<Arc<Mutex<()>>, String> {
    let locks = RESOURCE_MATERIALIZATION_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .map_err(|_| "resource materialization lock map is poisoned".to_string())?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key.to_string(), Arc::downgrade(&lock));
    Ok(lock)
}

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

/// Materializes the content-addressed resources for one record on demand.
/// Metadata synchronization never performs these GETs; callers invoke this
/// command immediately before an operation that needs a local path.
#[tauri::command]
pub fn materialize_clipboard_item(
    id: String,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    thumbnail_worker: tauri::State<'_, Mutex<ThumbnailWorker>>,
) -> Result<ClipboardItem, String> {
    if id.trim().is_empty() {
        return Err("clipboard item id is empty".to_string());
    }

    let current = database
        .get_item(&id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "clipboard item was not found".to_string())?;

    let sync_config = {
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_string())?;
        guard.sync_config()
    };
    if sync_config.provider != SyncProvider::S3 {
        return Ok(current);
    }
    let settings = SyncSettings::from_sync_config(&sync_config)?;
    let remote_scope = settings.remote_scope_id();
    let refs = database
        .get_sync_resource_refs(&remote_scope, &id)
        .map_err(|error| error.to_string())?;
    if refs.is_empty() {
        return Ok(current);
    }
    let store = settings.object_store()?;
    let session_key = settings.session_key(&remote_scope)?;
    let (updated, changed) = materialize_item_resources(
        &store,
        database.inner(),
        paths.inner(),
        &settings,
        &remote_scope,
        &id,
        refs,
        session_key.as_ref(),
    )?;

    if changed && updated.kind == ClipboardKind::Image {
        if let Some(resource_path) = updated.resource_path.as_deref() {
            if let Ok(worker) = thumbnail_worker.lock() {
                worker.enqueue(id.clone(), PathBuf::from(resource_path));
            }
        }
    }
    Ok(updated)
}

fn resource_destination(
    paths: &StoragePaths,
    category: v1::ResourceCategory,
    settings: &SyncSettings,
) -> (PathBuf, u64) {
    match category {
        v1::ResourceCategory::Image => (paths.images.clone(), settings.max_sync_image_bytes),
        v1::ResourceCategory::File => (paths.files.clone(), settings.max_sync_file_bytes),
        v1::ResourceCategory::Icon => (paths.storage.join("icons"), MAX_SYNC_ICON_BYTES),
    }
}

#[allow(clippy::too_many_arguments)]
fn materialize_item_resources(
    store: &impl v1::ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    settings: &SyncSettings,
    remote_scope: &str,
    id: &str,
    refs: Vec<v1::SyncResourceRef>,
    session_key: Option<&v1::SessionKey>,
) -> Result<(ClipboardItem, bool), String> {
    let mut materialized = Vec::with_capacity(refs.len());
    for reference in refs {
        let parsed = v1::parse_resource_key(&reference.object_key)?;
        let (destination_root, max_bytes) = resource_destination(paths, parsed.category, settings);
        let existing = database
            .materialized_sync_resource_path(remote_scope, id, &reference)
            .map_err(|error| error.to_string())?
            .map(|path| {
                if parsed.category == v1::ResourceCategory::Icon {
                    paths.storage.join("icons").join(path)
                } else {
                    PathBuf::from(path)
                }
            });
        let cache_path = if let Some(path) = existing.as_ref().filter(|path| {
            v1::verify_local_resource(path, &reference.object_key, max_bytes, session_key)
                .unwrap_or(false)
        }) {
            path.clone()
        } else {
            let lock =
                resource_materialization_lock(&format!("{remote_scope}:{}", reference.object_key))?;
            let _guard = lock
                .lock()
                .map_err(|_| "resource materialization lock is poisoned".to_string())?;
            let rechecked = database
                .materialized_sync_resource_path(remote_scope, id, &reference)
                .map_err(|error| error.to_string())?
                .map(|path| {
                    if parsed.category == v1::ResourceCategory::Icon {
                        paths.storage.join("icons").join(path)
                    } else {
                        PathBuf::from(path)
                    }
                });
            if let Some(path) = rechecked.as_ref().filter(|path| {
                v1::verify_local_resource(path, &reference.object_key, max_bytes, session_key)
                    .unwrap_or(false)
            }) {
                path.clone()
            } else {
                v1::materialize_resource(
                    store,
                    &reference.object_key,
                    &destination_root,
                    max_bytes,
                    session_key,
                )
                .map_err(|error| {
                    format!("failed to materialize {}: {error}", reference.object_key)
                })?
                .path
            }
        };
        materialized.push((reference, cache_path.to_string_lossy().to_string()));
    }

    let changed = database
        .mark_sync_resources_materialized(remote_scope, id, &materialized)
        .map_err(|error| error.to_string())?;
    let updated = database
        .get_item(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "clipboard item disappeared during materialization".to_string())?;
    Ok((updated, changed))
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
    use std::{
        collections::BTreeMap,
        fs,
        io::Write,
        path::Path,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::{
        domain::ClipboardKind,
        sync::v1::{
            DownloadedFile, DownloadedObject, ObjectInfo, ObjectMetadata, PutCondition, PutOutcome,
            RecordVersion, ReplicatedItem,
        },
    };

    #[derive(Default)]
    struct MaterializationStore {
        objects: Mutex<BTreeMap<String, Vec<u8>>>,
        gets: AtomicU64,
    }

    impl v1::ObjectStore for MaterializationStore {
        fn list(
            &self,
            _prefix: &str,
            _start_after: Option<&str>,
        ) -> Result<Vec<ObjectInfo>, String> {
            Ok(Vec::new())
        }

        fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
            Ok(self
                .objects
                .lock()
                .map_err(|_| "object lock poisoned".to_string())?
                .get(key)
                .cloned()
                .map(|bytes| DownloadedObject { bytes, etag: None }))
        }

        fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String> {
            Ok(self
                .objects
                .lock()
                .map_err(|_| "object lock poisoned".to_string())?
                .get(key)
                .map(|bytes| ObjectMetadata {
                    size_bytes: Some(bytes.len() as u64),
                    etag: None,
                }))
        }

        fn get_to_file(
            &self,
            key: &str,
            destination: &Path,
            max_bytes: u64,
        ) -> Result<Option<DownloadedFile>, String> {
            let Some(bytes) = self
                .objects
                .lock()
                .map_err(|_| "object lock poisoned".to_string())?
                .get(key)
                .cloned()
            else {
                return Ok(None);
            };
            if bytes.len() as u64 > max_bytes {
                return Err("object exceeds limit".to_string());
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|error| error.to_string())?;
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            self.gets.fetch_add(1, Ordering::Relaxed);
            Ok(Some(DownloadedFile {
                size_bytes: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
                etag: None,
            }))
        }

        fn put(
            &self,
            _key: &str,
            _bytes: Vec<u8>,
            _condition: PutCondition,
        ) -> Result<PutOutcome, String> {
            unreachable!()
        }

        fn put_file(
            &self,
            _key: &str,
            _path: &Path,
            _sha256: &str,
            _size_bytes: u64,
            _condition: PutCondition,
        ) -> Result<PutOutcome, String> {
            unreachable!()
        }

        fn delete(&self, _key: &str) -> Result<(), String> {
            unreachable!()
        }
    }

    fn temporary_paths(label: &str) -> StoragePaths {
        let root = std::env::temp_dir().join(format!(
            "clipboard-sync-materialize-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        StoragePaths::initialize(root).unwrap()
    }

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

    #[test]
    fn materialization_downloads_once_and_repairs_a_corrupt_cache() {
        let paths = temporary_paths("retry");
        let database = Database::open(&paths.database).unwrap();
        database.initialize_sync().unwrap();
        let bytes = b"remote image bytes".repeat(64);
        let digest = hex::encode(Sha256::digest(&bytes));
        let object_key =
            v1::resource_object_key(v1::ResourceCategory::Image, &digest, "png").unwrap();
        let item = crate::domain::ClipboardItem {
            id: "remote-image".to_string(),
            kind: ClipboardKind::Image,
            title: "remote image".to_string(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "hash-remote-image".to_string(),
            source_app: None,
            icon_path: None,
            size_bytes: bytes.len() as u64,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: Some("{}".to_string()),
        };
        let reference = v1::SyncResourceRef {
            slot: "image".to_string(),
            ordinal: 0,
            object_key: object_key.clone(),
        };
        database
            .apply_sync_snapshot_with_resources(
                &"a".repeat(64),
                &v1::DeviceCursor {
                    device_id: "11111111-1111-4111-8111-111111111111".to_string(),
                    epoch: "22222222-2222-4222-8222-222222222222".to_string(),
                    sequence: 0,
                    last_segment_key: None,
                },
                &"b".repeat(64),
                &v1::MutationBatch {
                    upserts: vec![ReplicatedItem {
                        item: item.clone(),
                        version: RecordVersion {
                            modified_at_ms: 2,
                            writer_device_id: "11111111-1111-4111-8111-111111111111".to_string(),
                        },
                    }],
                    tombstones: Vec::new(),
                },
                &BTreeMap::from([("remote-image".to_string(), vec![reference.clone()])]),
            )
            .unwrap();
        let settings = SyncSettings::from_sync_config(&configured_sync()).unwrap();
        let store = MaterializationStore::default();
        store
            .objects
            .lock()
            .unwrap()
            .insert(object_key, bytes.clone());

        let (first, changed) = materialize_item_resources(
            &store,
            &database,
            &paths,
            &settings,
            &"a".repeat(64),
            "remote-image",
            vec![reference.clone()],
            None,
        )
        .unwrap();
        assert!(changed);
        assert_eq!(store.gets.load(Ordering::Relaxed), 1);
        let local_path = PathBuf::from(first.resource_path.unwrap());
        assert_eq!(fs::read(&local_path).unwrap(), bytes);

        let (_, changed) = materialize_item_resources(
            &store,
            &database,
            &paths,
            &settings,
            &"a".repeat(64),
            "remote-image",
            vec![reference.clone()],
            None,
        )
        .unwrap();
        assert!(!changed);
        assert_eq!(store.gets.load(Ordering::Relaxed), 1);

        fs::write(&local_path, b"corrupt").unwrap();
        let (repaired, _) = materialize_item_resources(
            &store,
            &database,
            &paths,
            &settings,
            &"a".repeat(64),
            "remote-image",
            vec![reference],
            None,
        )
        .unwrap();
        assert_eq!(store.gets.load(Ordering::Relaxed), 2);
        assert_eq!(fs::read(repaired.resource_path.unwrap()).unwrap(), bytes);
        assert_eq!(database.count_sync_outbox().unwrap(), 0);

        drop(database);
        fs::remove_dir_all(paths.project).unwrap();
    }

    #[test]
    fn concurrent_materialization_shares_one_object_download() {
        let paths = temporary_paths("concurrent");
        let database = Database::open(&paths.database).unwrap();
        database.initialize_sync().unwrap();
        let bytes = b"concurrent image".repeat(128);
        let digest = hex::encode(Sha256::digest(&bytes));
        let object_key =
            v1::resource_object_key(v1::ResourceCategory::Image, &digest, "png").unwrap();
        let reference = v1::SyncResourceRef {
            slot: "image".to_string(),
            ordinal: 0,
            object_key: object_key.clone(),
        };
        let item = crate::domain::ClipboardItem {
            id: "concurrent-image".to_string(),
            kind: ClipboardKind::Image,
            title: "concurrent image".to_string(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "hash-concurrent-image".to_string(),
            source_app: None,
            icon_path: None,
            size_bytes: bytes.len() as u64,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: Some("{}".to_string()),
        };
        let remote_scope = "c".repeat(64);
        database
            .apply_sync_snapshot_with_resources(
                &remote_scope,
                &v1::DeviceCursor {
                    device_id: "11111111-1111-4111-8111-111111111111".to_string(),
                    epoch: "22222222-2222-4222-8222-222222222222".to_string(),
                    sequence: 0,
                    last_segment_key: None,
                },
                &"d".repeat(64),
                &v1::MutationBatch {
                    upserts: vec![ReplicatedItem {
                        item,
                        version: RecordVersion {
                            modified_at_ms: 2,
                            writer_device_id: "11111111-1111-4111-8111-111111111111".to_string(),
                        },
                    }],
                    tombstones: Vec::new(),
                },
                &BTreeMap::from([("concurrent-image".to_string(), vec![reference.clone()])]),
            )
            .unwrap();
        let store = Arc::new(MaterializationStore::default());
        store.objects.lock().unwrap().insert(object_key, bytes);
        let settings = Arc::new(SyncSettings::from_sync_config(&configured_sync()).unwrap());
        let paths = Arc::new(paths);
        let database = Arc::new(database);
        let mut handles = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let settings = Arc::clone(&settings);
            let paths = Arc::clone(&paths);
            let database = Arc::clone(&database);
            let remote_scope = remote_scope.clone();
            let reference = reference.clone();
            handles.push(std::thread::spawn(move || {
                materialize_item_resources(
                    store.as_ref(),
                    database.as_ref(),
                    paths.as_ref(),
                    settings.as_ref(),
                    &remote_scope,
                    "concurrent-image",
                    vec![reference],
                    None,
                )
                .unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(store.gets.load(Ordering::Relaxed), 1);
        drop(database);
        fs::remove_dir_all(&paths.project).unwrap();
    }
}
