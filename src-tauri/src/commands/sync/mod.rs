use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;

use crate::commands::lock::lock_state;
use crate::config::{ConfigStore, SyncConfig, SyncProvider};
use crate::content::ThumbnailWorker;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::storage::{ClipboardRepository, Database, StoragePaths};
use crate::sync::{self, v1};

mod auto;
mod policy;

pub use policy::SyncCancellation;

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
/// Resource byte limits are clamped to a working window, mirroring the
/// `segment_max_entries` / `auto_sync_interval_secs` treatment: 0 makes the
/// upload side silently skip every binary while materialization hard-fails,
/// and huge values disable protection against runaway downloads.
const MIN_SYNC_RESOURCE_BYTES: u64 = 1024;
const MAX_SYNC_RESOURCE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn resource_materialization_lock(key: &str) -> Result<Arc<Mutex<()>>, String> {
    let locks = RESOURCE_MATERIALIZATION_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = lock_state(&locks, "resource materialization lock map is poisoned")?;
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
    resource_gc_mode: String,
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
    pub skipped_resources: u64,
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
            skipped_resources: result.skipped_resources,
        }
    }
}

/// Immutable snapshot of every value needed by one network run. The config
/// mutex is released before key derivation, hashing, SQLite export, or S3 I/O.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SyncSettings {
    endpoint: String,
    remote_path: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    sync_password: Option<String>,
    segment_max_entries: usize,
    resource_gc_mode: v1::ResourceGcMode,
    max_sync_image_bytes: u64,
    max_sync_file_bytes: u64,
}

// Manual `Debug` so a future `{:?}` log or panic context cannot leak the S3
// secret or the sync password (mirrors `S3ObjectStore`, which deliberately
// does not derive `Debug` for the same reason).
impl std::fmt::Debug for SyncSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncSettings")
            .field("endpoint", &self.endpoint)
            .field("remote_path", &self.remote_path)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key", &self.access_key)
            .field("secret_key", &"<redacted>")
            .field(
                "sync_password",
                &self.sync_password.as_ref().map(|_| "<redacted>"),
            )
            .field("segment_max_entries", &self.segment_max_entries)
            .field("resource_gc_mode", &self.resource_gc_mode)
            .field("max_sync_image_bytes", &self.max_sync_image_bytes)
            .field("max_sync_file_bytes", &self.max_sync_file_bytes)
            .finish()
    }
}

impl Drop for SyncSettings {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret_key.zeroize();
        if let Some(password) = self.sync_password.as_mut() {
            password.zeroize();
        }
    }
}

impl SyncSettings {
    fn from_config(config: &ConfigStore) -> Result<Self, String> {
        Self::from_sync_config(&config.sync_config())
    }

    fn from_sync_config(sync: &SyncConfig) -> Result<Self, String> {
        if sync.provider != SyncProvider::S3 {
            return Err("S3 sync is disabled".to_string());
        }
        Self::validated_from_sync_config(sync)
    }

    fn with_rotated_scope(&self, remote_path: String, sync_password: Option<String>) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            remote_path,
            region: self.region.clone(),
            bucket: self.bucket.clone(),
            access_key: self.access_key.clone(),
            secret_key: self.secret_key.clone(),
            sync_password,
            segment_max_entries: self.segment_max_entries,
            resource_gc_mode: self.resource_gc_mode,
            max_sync_image_bytes: self.max_sync_image_bytes,
            max_sync_file_bytes: self.max_sync_file_bytes,
        }
    }

    /// The connection test must work before the user turns the sync switch
    /// on, so it deliberately skips the provider gate while still validating
    /// every required field. Also used by the real-S3 rotation smoke.
    pub(crate) fn validated_from_sync_config(sync: &SyncConfig) -> Result<Self, String> {
        Ok(Self {
            endpoint: validate_sync_endpoint(&required_value(
                sync.endpoint.as_deref(),
                "S3 endpoint",
            )?)?,
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
            resource_gc_mode: sync
                .resource_gc_mode
                .parse::<v1::ResourceGcMode>()
                .unwrap_or_default(),
            max_sync_image_bytes: sync
                .max_sync_image_bytes
                .clamp(MIN_SYNC_RESOURCE_BYTES, MAX_SYNC_RESOURCE_BYTES),
            max_sync_file_bytes: sync
                .max_sync_file_bytes
                .clamp(MIN_SYNC_RESOURCE_BYTES, MAX_SYNC_RESOURCE_BYTES),
        })
    }

    pub(crate) fn remote_scope_id(&self) -> String {
        let endpoint = self.endpoint.trim().trim_end_matches('/');
        let identity = format!(
            "clipboard-sync-v1-namespace\n{endpoint}\n{}\n{}\n{}",
            self.region, self.bucket, self.remote_path
        );
        hex::encode(Sha256::digest(identity.as_bytes()))
    }

    pub(crate) fn legacy_scope_id(&self) -> String {
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

    pub(crate) fn engine_options(&self) -> v1::SyncEngineOptions {
        v1::SyncEngineOptions {
            segment_max_entries: self.segment_max_entries,
            resource_limits: v1::ResourceLimits {
                image_bytes: self.max_sync_image_bytes,
                file_bytes: self.max_sync_file_bytes,
                icon_bytes: MAX_SYNC_ICON_BYTES,
            },
            resource_gc_mode: self.resource_gc_mode,
        }
    }
}

#[tauri::command]
pub fn get_sync_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    database: tauri::State<'_, Database>,
) -> Result<SyncConfigInfo, String> {
    let guard = lock_state(&config, "configuration lock is poisoned")?;
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
        resource_gc_mode: sync
            .resource_gc_mode
            .parse::<v1::ResourceGcMode>()
            .unwrap_or_default()
            .as_str()
            .to_string(),
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
    resource_gc_mode: String,
    max_sync_image_bytes: u64,
    max_sync_file_bytes: u64,
    s3_region: Option<String>,
    s3_bucket: Option<String>,
    s3_access_key: Option<String>,
    s3_secret_key: Option<String>,
    sync_password: Option<String>,
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<(), String> {
    let mut guard = lock_state(&config, "configuration lock is poisoned")?;
    let previous = guard.sync_config();
    let previous_scope = previous
        .provider
        .eq(&SyncProvider::S3)
        .then(|| SyncSettings::validated_from_sync_config(&previous).ok())
        .flatten()
        .map(|settings| settings.remote_scope_id());
    let mut sync = SyncConfig {
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
        resource_gc_mode: resource_gc_mode
            .parse::<v1::ResourceGcMode>()
            .unwrap_or_default()
            .as_str()
            .to_string(),
        max_sync_image_bytes: max_sync_image_bytes
            .clamp(MIN_SYNC_RESOURCE_BYTES, MAX_SYNC_RESOURCE_BYTES),
        max_sync_file_bytes: max_sync_file_bytes
            .clamp(MIN_SYNC_RESOURCE_BYTES, MAX_SYNC_RESOURCE_BYTES),
    };
    if let Some(endpoint) = sync.endpoint.clone() {
        sync.endpoint = Some(validate_sync_endpoint(&endpoint)?);
    }
    // The outbox is local state shared by every remote target. When the user
    // points sync at a different endpoint/bucket/prefix, the new target must
    // bootstrap the complete local state instead of consuming changes that
    // another target may already have acknowledged. Resetting the target's
    // publication state also covers switching back to an older target after
    // its pending outbox rows were drained elsewhere.
    let next_scope = sync
        .provider
        .eq(&SyncProvider::S3)
        .then(|| SyncSettings::validated_from_sync_config(&sync).ok())
        .flatten()
        .map(|settings| settings.remote_scope_id());
    if let (Some(previous_scope), Some(next_scope)) = (previous_scope, next_scope) {
        if previous_scope != next_scope {
            database
                .reset_sync_remote_state(&next_scope)
                .map_err(|error| error.to_string())?;
        }
    }
    guard
        .set_sync_config(sync)
        .map_err(|error| error.to_string())
}

/// Tests the persisted S3 settings. The settings UI saves its current fields
/// before invoking this command, so write-only secrets never cross back to the
/// frontend after initial entry.
///
/// The synchronous S3 facade runs on the blocking pool, with the same privacy
/// gate and cancellation registration as synchronization and materialization.
#[tauri::command]
pub async fn test_sync_connection(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    cancellation: tauri::State<'_, SyncCancellation>,
) -> Result<sync::S3TestResult, String> {
    let (settings, token) = {
        let guard = lock_state(&config, "configuration lock is poisoned")?;
        let token = cancellation.register(guard.privacy_local_only(), &cancellation.0)?;
        (
            SyncSettings::validated_from_sync_config(&guard.sync_config())?,
            token,
        )
    };
    tauri::async_runtime::spawn_blocking(move || {
        token.run(|| {
            token.check()?;
            settings.object_store()?;
            Ok(sync::test_s3_connection(
                &settings.endpoint,
                &settings.region,
                &settings.bucket,
                &settings.access_key,
                &settings.secret_key,
            ))
        })
    })
    .await
    .map_err(|error| format!("connection test task join failed: {error}"))?
}

/// Async: a full sync run performs blocking S3 I/O and must not occupy the
/// async runtime worker threads; the blocking work therefore runs on the
/// dedicated blocking pool instead of stalling other async commands.
#[tauri::command]
pub async fn sync_now(app: tauri::AppHandle) -> Result<SyncRunResult, String> {
    tauri::async_runtime::spawn_blocking(move || run_sync(&app))
        .await
        .map_err(|error| format!("sync task join failed: {error}"))?
}

/// Sync settings usable for on-demand materialization, or `None` when sync is
/// off or selected-but-not-configured.
///
/// Kept separate from [`SyncSettings::from_sync_config`] so the strict variant
/// stays available to the connection test, which *should* reject an incomplete
/// configuration instead of tolerating it.
fn usable_sync_settings(sync: &SyncConfig) -> Option<SyncSettings> {
    if sync.provider != SyncProvider::S3 {
        return None;
    }
    match SyncSettings::from_sync_config(sync) {
        Ok(settings) => Some(settings),
        Err(error) => {
            crate::log_warn!(
                "[sync] provider is selected but the configuration is incomplete, \
                 so resources cannot be materialized on demand: {error}"
            );
            None
        }
    }
}

/// Materializes the content-addressed resources for one record on demand.
/// Metadata synchronization never performs these GETs; callers invoke this
/// command immediately before an operation that needs a local path.
///
/// Fetching from the cloud is an optimisation, never a precondition: sync can be
/// *selected* without being usable (provider picked, bucket or credentials not
/// filled in yet, or cleared again). Turning that state into an error made every
/// image/file read — copy, paste, preview, fullscreen, save — fail with whatever
/// the caller happened to report, while the local record was perfectly fine. An
/// unusable configuration therefore falls through to the local row and only logs.
///
/// Async: on-demand materialization performs blocking S3 GETs (up to the
/// configured resource limits) plus whole-file hash verification. The work
/// runs on the dedicated blocking pool — a slow or unreachable endpoint would
/// otherwise freeze the window event loop while the user waits on a copy,
/// preview, fullscreen or save action.
#[tauri::command]
pub async fn materialize_clipboard_item(
    id: String,
    app: tauri::AppHandle,
) -> Result<ClipboardItem, String> {
    tauri::async_runtime::spawn_blocking(move || materialize_clipboard_item_blocking(&app, &id))
        .await
        .map_err(|error| format!("materialize task join failed: {error}"))?
}

fn materialize_clipboard_item_blocking(
    app: &tauri::AppHandle,
    id: &str,
) -> Result<ClipboardItem, String> {
    let config = app.state::<Mutex<ConfigStore>>();
    let cancellation = app.state::<SyncCancellation>();
    let database = app.state::<Database>();
    let paths = app.state::<StoragePaths>();
    let thumbnail_worker = app.state::<Mutex<ThumbnailWorker>>();

    if id.trim().is_empty() {
        return Err("clipboard item id is empty".to_string());
    }

    let current = database
        .get_item(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "clipboard item was not found".to_string())?;

    let (sync_config, token) = {
        let guard = lock_state(&config, "configuration lock is poisoned")?;
        if guard.privacy_local_only() {
            return Ok(current);
        }
        (
            guard.sync_config(),
            cancellation.register(false, &cancellation.0)?,
        )
    };
    let Some(settings) = usable_sync_settings(&sync_config) else {
        return Ok(current);
    };
    let stable_scope = settings.remote_scope_id();
    let binding = database
        .sync_namespace_binding(&stable_scope)
        .map_err(|e| e.to_string())?
        .map(|bytes| clipboard_sync::v1::namespace::Namespace::from_bytes(&bytes))
        .transpose()?;
    let remote_scope = binding
        .as_ref()
        .map(|v| v.scope.clone())
        .unwrap_or_else(|| settings.legacy_scope_id());
    let refs = database
        .get_sync_resource_refs(&remote_scope, id)
        .map_err(|error| error.to_string())?;
    if refs.is_empty() {
        return Ok(current);
    }
    let store = match settings.object_store() {
        Ok(store) => store,
        Err(error) => {
            // Same reasoning as above: a store that cannot even be constructed
            // means nothing can be fetched, not that the record is broken.
            crate::log_warn!(
                "[sync] unable to build the object store while materializing: {error}"
            );
            return Ok(current);
        }
    };
    let session_key = match binding {
        Some(binding) => binding.session_key(settings.sync_password.as_deref())?,
        None => settings.session_key(&remote_scope)?,
    };
    let (updated, changed) = token.run(|| {
        materialize_item_resources(
            &store,
            database.inner(),
            paths.inner(),
            &settings,
            &remote_scope,
            id,
            refs,
            session_key.as_ref(),
        )
    })?;

    if changed && updated.kind == ClipboardKind::Image {
        if let Some(resource_path) = updated.resource_path.as_deref() {
            if let Ok(worker) = thumbnail_worker.lock() {
                worker.enqueue(id.to_string(), PathBuf::from(resource_path));
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

/// Cap for the verified-resource memo. Entries are tiny (path + object key +
/// file stat), so the limit only bounds growth across a session touching
/// thousands of distinct synced resources; on overflow the memo is cleared and
/// rebuilt from real verifications.
const LOCAL_RESOURCE_VERIFY_CACHE_LIMIT: usize = 512;

/// Identity of a successful `verify_local_resource` outcome: the file path,
/// the object key (whose embedded digest already encodes the session key
/// generation), the byte limit applied, and the file stat. Any mutation of the
/// file changes size or mtime and misses the memo.
type LocalResourceVerifyKey = (PathBuf, String, u64, u64, Option<SystemTime>);

/// Memoizes successful `verify_local_resource` outcomes, mirroring the
/// `digest_cache` convention in `ocr/models.rs`. Materialize re-verifies the
/// local cache on every paste/copy/preview of a synced record; for a
/// multi-gigabyte file that is a full re-hash per use. Only `true` outcomes
/// are memoized: a failed verification must re-run until the file actually
/// verifies (or gets re-materialized, which changes its stat and misses the
/// memo anyway).
fn local_resource_verified_cached(
    path: &Path,
    object_key: &str,
    max_bytes: u64,
    session_key: Option<&v1::SessionKey>,
) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<LocalResourceVerifyKey, ()>>> = OnceLock::new();
    let stat = match std::fs::metadata(path) {
        Ok(stat) => (stat.len(), stat.modified().ok()),
        Err(_) => return false,
    };
    let cache_key = (
        path.to_path_buf(),
        object_key.to_owned(),
        max_bytes,
        stat.0,
        stat.1,
    );
    if CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains_key(&cache_key)
    {
        return true;
    }
    if !v1::verify_local_resource(path, object_key, max_bytes, session_key).unwrap_or(false) {
        return false;
    }
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.len() >= LOCAL_RESOURCE_VERIFY_CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(cache_key, ());
    true
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
    let _resource_publication = database.begin_resource_write();
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
            local_resource_verified_cached(path, &reference.object_key, max_bytes, session_key)
        }) {
            path.clone()
        } else {
            let lock =
                resource_materialization_lock(&format!("{remote_scope}:{}", reference.object_key))?;
            let _guard = lock_state(&lock, "resource materialization lock is poisoned")?;
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

/// What one scope-wide materialization pass did, for the rotation flow's
/// confirm/progress surface.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ScopeMaterializationStats {
    /// Items that carry at least one resource reference in the scope.
    items: u64,
    /// Items whose local cache this pass wrote (a download happened).
    downloaded_items: u64,
}

/// Materializes every resource reference of one scope into local storage.
///
/// Rotation republishes the complete local state under a new key, and a
/// reference whose body was never materialized would ship a dangling object
/// key into the new namespace (`docs/SYNC_V2.md` constraint 3), so this pass
/// is a hard prerequisite. Items whose references are already local and
/// verifying cost no network I/O; the first failure propagates so a rotation
/// never starts from an incomplete scope.
fn materialize_scope_resources(
    store: &impl v1::ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    settings: &SyncSettings,
    remote_scope: &str,
    session_key: Option<&v1::SessionKey>,
) -> Result<ScopeMaterializationStats, String> {
    let references = database
        .list_sync_resource_refs_for_scope(remote_scope)
        .map_err(|error| error.to_string())?;
    let mut stats = ScopeMaterializationStats::default();
    for (item_id, refs) in references {
        if refs.is_empty() {
            continue;
        }
        stats.items += 1;
        let (_, changed) = materialize_item_resources(
            store,
            database,
            paths,
            settings,
            remote_scope,
            &item_id,
            refs,
            session_key,
        )?;
        if changed {
            stats.downloaded_items += 1;
        }
    }
    Ok(stats)
}

/// What one namespace rotation did, for the caller and the UI.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RotationOutcome {
    pub(crate) new_prefix: String,
    pub(crate) materialized_items: u64,
    pub(crate) uploaded_entries: u64,
    pub(crate) uploaded_resources: u64,
    pub(crate) deleted_objects: u64,
}

/// Rotates the sync namespace to a new prefix and key (`docs/SYNC_V2.md` D1-A
/// with the decided D2 protocol).
///
/// The new prefix is what makes this crash-safe: the old namespace stays
/// intact until the new one is durable. Order is therefore materialize every
/// referenced resource locally (a republished reference without a body would
/// ship a dangling object key), bootstrap-publish the complete local state
/// under the new prefix and key, write the tombstone at the old prefix root,
/// hand the new configuration to `commit_local_config` (the tombstone makes
/// every run at the old prefix refuse, so the local move must happen before
/// the old namespace starts disappearing), and finally delete the old engine
/// namespace. Any earlier failure leaves the old prefix and password
/// configured and the run retryable.
#[allow(clippy::too_many_arguments)]
pub(crate) fn perform_namespace_rotation(
    old_store: &impl v1::ObjectStore,
    new_store: &impl v1::ObjectStore,
    database: &Database,
    paths: &StoragePaths,
    old_settings: &SyncSettings,
    new_settings: &SyncSettings,
    new_password: Option<&str>,
    cancellation: &clipboard_sync::cancellation::CancellationToken,
    commit_local_config: &mut dyn FnMut() -> Result<(), String>,
    progress: &mut dyn FnMut(clipboard_sync::v1::engine::SyncPhase),
) -> Result<RotationOutcome, String> {
    cancellation.check()?;
    // Refuse a second rotation; the same check also covers the case where this
    // prefix was rotated away by another device while we were offline.
    refuse_if_prefix_rotated(old_store)?;

    // --- old namespace: authenticate it and materialize every resource ---
    let old_stable_scope = old_settings.remote_scope_id();
    let previous = database
        .sync_namespace_binding(&old_stable_scope)
        .map_err(|error| error.to_string())?
        .map(|bytes| clipboard_sync::v1::namespace::Namespace::from_bytes(&bytes))
        .transpose()?;
    let (old_binding, old_session_key) = clipboard_sync::v1::namespace::resolve_namespace(
        old_store,
        &old_stable_scope,
        &old_settings.legacy_scope_id(),
        old_settings.sync_password.as_deref(),
        previous.as_ref(),
    )?;
    progress(clipboard_sync::v1::engine::SyncPhase::Preparing);
    let materialized = materialize_scope_resources(
        old_store,
        database,
        paths,
        old_settings,
        &old_binding.scope,
        old_session_key.as_ref(),
    )?;
    cancellation.check()?;

    // --- new namespace: claim it, then bootstrap the complete local state ---
    let new_stable_scope = new_settings.remote_scope_id();
    let (new_binding, new_session_key) = clipboard_sync::v1::namespace::resolve_namespace(
        new_store,
        &new_stable_scope,
        &new_settings.legacy_scope_id(),
        new_password,
        None,
    )?;
    database
        .remember_sync_namespace(&new_stable_scope, &new_binding.to_bytes())
        .map_err(|error| error.to_string())?;
    progress(clipboard_sync::v1::engine::SyncPhase::Uploading);
    let engine_result = clipboard_sync::v1::engine::sync_database_cancellable(
        new_store,
        database,
        &v1::SyncEnginePaths::from(paths),
        &new_binding.scope,
        new_session_key.as_ref(),
        new_settings.engine_options(),
        cancellation,
    )?;
    cancellation.check()?;
    // Initialization without a published head is an engine bug, not a
    // rotation: never tombstone the old namespace on that basis.
    let initialized = database
        .get_or_create_sync_remote_state(&new_binding.scope)
        .map_err(|error| error.to_string())?
        .initialized;
    if !initialized {
        return Err("rotation refused: the new namespace did not initialize".to_string());
    }
    progress(clipboard_sync::v1::engine::SyncPhase::Downloading);

    // --- tombstone at the old prefix root, then delete the old namespace ---
    let device_id = database
        .get_sync_device_id()
        .map_err(|error| error.to_string())?;
    let pointer = serde_json::json!({
        "newPrefix": new_settings.remote_path,
        "rotatedAtMs": current_time_ms(),
        "deviceId": device_id,
        "keyCheck": new_binding.key_check(),
    })
    .to_string();
    // Create-only: a concurrent rotation must not be silently overwritten.
    match old_store.put(
        ROTATED_TO_KEY,
        pointer.into_bytes(),
        v1::PutCondition::IfAbsent,
    )? {
        v1::PutOutcome::Stored { .. } => {}
        v1::PutOutcome::PreconditionFailed => {
            return Err("another device already rotated this prefix".to_string());
        }
    }
    // Move the local configuration onto the new prefix BEFORE deleting the old
    // namespace. The tombstone makes every run at the old prefix refuse, so a
    // crash or cancellation between the two would otherwise strand this
    // device: its config still points at a prefix that now answers "rotated".
    // Leftover old objects after an interrupted deletion are bounded garbage;
    // an unsyncable device is not.
    commit_local_config()?;
    cancellation.check()?;
    let mut deleted_objects = 0u64;
    for object in old_store.list("", None)? {
        // The tombstone is the discovery artifact peers read; only engine
        // objects are replaced by the new prefix's copy.
        if object.key == ROTATED_TO_KEY || !object.key.starts_with("v1/") {
            continue;
        }
        old_store.delete(&object.key)?;
        deleted_objects += 1;
    }
    progress(clipboard_sync::v1::engine::SyncPhase::Compacting);
    Ok(RotationOutcome {
        new_prefix: new_settings.remote_path.clone(),
        materialized_items: materialized.items,
        uploaded_entries: engine_result.uploaded_entries,
        uploaded_resources: engine_result.uploaded_resources,
        deleted_objects,
    })
}

/// Replaces the sync password (or toggles the encryption mode) of an
/// initialized scope by rotating it to a fresh prefix and key.
///
/// The frontend flow is a separate unit; this command is the engine-level
/// surface. `new_password` is explicit: an empty string means plaintext, and
/// no value is ever retained from the previous configuration, unlike the
/// settings save path.
///
/// Async: the rotation downloads every referenced resource and republishes the
/// complete local state, so it runs on the blocking pool with the same
/// operation registry (progress + cancellation) as a manual sync.
#[tauri::command]
pub async fn rotate_sync_namespace(
    new_password: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || run_namespace_rotation(&app, new_password))
        .await
        .map_err(|error| format!("rotation task join failed: {error}"))?
}

fn run_namespace_rotation(app: &tauri::AppHandle, new_password: String) -> Result<(), String> {
    use crate::background_operations::{BackgroundOperations, OperationKind, OperationPhase};
    let cancellation = app.state::<SyncCancellation>().0.child_token();
    let config = app.state::<Mutex<ConfigStore>>();
    let database = app.state::<Database>();
    let paths = app.state::<StoragePaths>();

    let old_settings = {
        let guard = lock_state(&config, "configuration lock is poisoned")?;
        SyncSettings::from_config(&guard)?
    };
    // Derived, never typed: the timestamp makes the prefix unique per rotation
    // and the tombstone carries it to the peers.
    let new_prefix = format!(
        "{}-rotated-{}",
        old_settings.remote_path.trim_matches('/'),
        current_time_ms()
    );
    let new_settings = old_settings.with_rotated_scope(
        new_prefix.clone(),
        Some(new_password).filter(|password| !password.is_empty()),
    );
    if new_settings.sync_password.is_none() && old_settings.sync_password.is_some() {
        // Toggling encryption off is legal but must be deliberate: the caller
        // passes an explicitly empty password, which the UI confirms.
        crate::log_warn!("[sync] rotation clears the namespace encryption password");
    }

    let (operation, token) = {
        let guard = lock_state(&config, "configuration lock is poisoned")?;
        let token = app
            .state::<SyncCancellation>()
            .register(guard.privacy_local_only(), &cancellation)?;
        let operation = app
            .state::<BackgroundOperations>()
            .start(OperationKind::Sync, &token)?;
        (operation, token)
    };
    let mut config_committed = false;
    let outcome = token.run(|| {
        let old_store = old_settings.object_store()?;
        let new_store = new_settings.object_store()?;
        perform_namespace_rotation(
            &old_store,
            &new_store,
            database.inner(),
            paths.inner(),
            &old_settings,
            &new_settings,
            new_settings.sync_password.as_deref(),
            &token,
            &mut || {
                // Invoked right after the tombstone is durable: the old prefix
                // now answers "rotated" to every run, so the local
                // configuration must move before the old namespace disappears.
                // A failure here leaves the old prefix configured and the
                // rotation retryable (the tombstone is create-only, so a
                // later attempt reports "another device already rotated").
                let mut guard = lock_state(&config, "configuration lock is poisoned")?;
                let previous = guard.sync_config();
                let sync = SyncConfig {
                    remote_path: Some(new_settings.remote_path.clone()),
                    sync_password: new_settings.sync_password.clone(),
                    ..previous
                };
                if let Some(endpoint) = sync.endpoint.as_deref() {
                    validate_sync_endpoint(endpoint)?;
                }
                guard
                    .set_sync_config(sync)
                    .map_err(|error| error.to_string())?;
                config_committed = true;
                Ok(())
            },
            &mut |phase| {
                let phase = match phase {
                    clipboard_sync::v1::engine::SyncPhase::Preparing => OperationPhase::Preparing,
                    clipboard_sync::v1::engine::SyncPhase::Discovering => {
                        OperationPhase::Discovering
                    }
                    clipboard_sync::v1::engine::SyncPhase::Uploading => OperationPhase::Uploading,
                    clipboard_sync::v1::engine::SyncPhase::Downloading => {
                        OperationPhase::Downloading
                    }
                    clipboard_sync::v1::engine::SyncPhase::Compacting => OperationPhase::Compacting,
                };
                let _ = operation.progress(phase, 0, None);
            },
        )
    });
    crate::item_operations::invalidate_desktop(app);
    match outcome {
        Ok(outcome) => {
            crate::log_info!(
                "[sync] namespace rotated: materialized {} items, published {} entries / {} resources, deleted {} old objects",
                outcome.materialized_items,
                outcome.uploaded_entries,
                outcome.uploaded_resources,
                outcome.deleted_objects
            );
            Ok(())
        }
        Err(error) => {
            // The configuration moved once the tombstone was durable, so a
            // failure after that point (an interrupted deletion, say) leaves a
            // usable device pointed at the new prefix plus bounded leftover
            // objects under the old one — not an unsyncable one.
            if config_committed {
                crate::log_warn!(
                    "[sync] rotation committed the new prefix but did not finish: {error}"
                );
            }
            Err(error)
        }
    }
}

/// Serialize manual/automatic sync with the storage migration boundary.
pub(crate) fn try_lock_sync_run() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    // Distinguish a real concurrent run from a poisoned lock: a panic inside a
    // previous run must not disable sync forever with a misleading message.
    match SYNC_RUN_LOCK.try_lock() {
        Ok(guard) => Ok(guard),
        Err(std::sync::TryLockError::WouldBlock) => Err("sync already in progress".to_string()),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Ok(poisoned.into_inner()),
    }
}

/// Shared entry point for the Tauri command and the auto-sync worker.
pub(super) fn run_sync(app: &tauri::AppHandle) -> Result<SyncRunResult, String> {
    let cancellation = app.state::<SyncCancellation>().0.child_token();
    run_sync_cancellable(app, &cancellation)
}

/// Rotation tombstone key at the configured prefix root, deliberately outside
/// the engine's `v1/` namespace (`docs/SYNC_V2.md` D2). A device that finds it
/// was rotated away and must refuse to sync instead of repairing the old
/// binding from its cache, which would fork the namespace.
pub(crate) const ROTATED_TO_KEY: &str = "rotated-to";

/// Public descriptor a rotating device leaves at the old prefix root.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RotationPointer {
    pub(crate) new_prefix: String,
    pub(crate) rotated_at_ms: i64,
    pub(crate) device_id: String,
}

/// Pure decision behind [`refuse_if_prefix_rotated`]. The object bytes are a
/// tombstone whether or not they parse, because only this codebase writes the
/// key — a malformed pointer still means "rotated".
pub(crate) fn rotation_refusal(pointer: Option<&[u8]>) -> Result<(), String> {
    let Some(bytes) = pointer else {
        return Ok(());
    };
    match serde_json::from_slice::<RotationPointer>(bytes) {
        Ok(pointer) => Err(format!(
            "the sync prefix was rotated to {:?} by device {} at {}; sync stays disabled \
             here until the new prefix and password are configured",
            pointer.new_prefix, pointer.device_id, pointer.rotated_at_ms
        )),
        Err(_) => Err(
            "the sync prefix was rotated by another device; sync stays disabled here until \
             the new prefix and password are configured"
                .to_string(),
        ),
    }
}

/// Refuses a sync run whose configured prefix carries a rotation tombstone.
///
/// Runs before namespace resolution on purpose: a missing `v1/namespace.json`
/// would otherwise be repaired from the local binding cache and bootstrap a
/// second namespace under the old password. An S3 error that is not "absent"
/// propagates — sync fails at the next request anyway, and skipping the check
/// silently would reopen the fork.
pub(crate) fn refuse_if_prefix_rotated(store: &impl v1::ObjectStore) -> Result<(), String> {
    let object = store.get(ROTATED_TO_KEY).map_err(|error| {
        format!("unable to check the rotation pointer for this sync prefix: {error}")
    })?;
    rotation_refusal(object.as_ref().map(|object| object.bytes.as_slice()))
}

pub(super) fn run_sync_cancellable(
    app: &tauri::AppHandle,
    cancellation: &clipboard_sync::cancellation::CancellationToken,
) -> Result<SyncRunResult, String> {
    cancellation.check()?;
    let _run_guard = try_lock_sync_run()?;
    use crate::background_operations::{BackgroundOperations, OperationKind, OperationPhase};
    let config = app.state::<Mutex<ConfigStore>>();
    let database = app.state::<Database>();
    let paths = app.state::<StoragePaths>();

    let (settings, cancellation) = {
        let guard = lock_state(&config, "configuration lock is poisoned")?;
        let token = app
            .state::<SyncCancellation>()
            .register(guard.privacy_local_only(), cancellation)?;
        (SyncSettings::from_config(&guard)?, token)
    };
    // The operation must descend from the registered privacy token so both
    // local-only changes and the current-task cancel button reach all S3 I/O.
    let operation = app
        .state::<BackgroundOperations>()
        .start(OperationKind::Sync, &cancellation)?;
    let store = settings.object_store()?;
    let engine_paths = v1::SyncEnginePaths::from(paths.inner());

    let _resource_publication = database.begin_resource_write();
    let outcome = operation.token().run(|| {
        operation.check()?;
        // The rotation tombstone outranks everything below: without this check
        // a stale peer repairs the old binding from cache and forks the
        // namespace (docs/SYNC_V2.md D2).
        refuse_if_prefix_rotated(&store)?;
        let stable_scope = settings.remote_scope_id();
        let previous = database
            .sync_namespace_binding(&stable_scope)
            .map_err(|e| e.to_string())?
            .map(|bytes| clipboard_sync::v1::namespace::Namespace::from_bytes(&bytes))
            .transpose()?;
        let (binding, session_key) = clipboard_sync::v1::namespace::resolve_namespace(
            &store,
            &stable_scope,
            &settings.legacy_scope_id(),
            settings.sync_password.as_deref(),
            previous.as_ref(),
        )?;
        database
            .remember_sync_namespace(&stable_scope, &binding.to_bytes())
            .map_err(|e| e.to_string())?;
        clipboard_sync::v1::engine::sync_database_with_progress(
            &store,
            database.inner(),
            &engine_paths,
            &binding.scope,
            session_key.as_ref(),
            settings.engine_options(),
            |phase| {
                use clipboard_sync::v1::engine::SyncPhase;
                let phase = match phase {
                    SyncPhase::Preparing => OperationPhase::Preparing,
                    SyncPhase::Discovering => OperationPhase::Discovering,
                    SyncPhase::Uploading => OperationPhase::Uploading,
                    SyncPhase::Downloading => OperationPhase::Downloading,
                    SyncPhase::Compacting => OperationPhase::Compacting,
                };
                // Cancellation is checked by the scoped engine/transport, not swallowed here.
                let _ = operation.progress(phase, 0, None);
            },
        )
    });
    crate::item_operations::invalidate_desktop(app);
    let now_ms = current_time_ms();
    let result = match outcome {
        Ok(engine_result) => {
            if let Ok(mut guard) = config.lock() {
                let status = if engine_result.failed_peers == 0 {
                    "success"
                } else {
                    "partial"
                };
                if let Err(error) = guard.update_sync_status(status, now_ms) {
                    crate::log_error!("[sync] failed to persist sync status: {error}");
                }
            }
            Ok(engine_result.into())
        }
        Err(error) => {
            if let Ok(mut guard) = config.lock() {
                if let Err(status_error) = guard.update_sync_status(
                    if operation.token().is_cancelled() {
                        "cancelled"
                    } else {
                        "failed"
                    },
                    now_ms,
                ) {
                    crate::log_error!(
                        "[sync] failed to persist failed sync status: {status_error}"
                    );
                }
            }
            Err(error)
        }
    };
    operation.finish(result)
}

fn required_value(value: Option<&str>, label: &str) -> Result<String, String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{label} is not configured"))
}

/// Rejects a plaintext `http://` S3 endpoint unless it points at loopback,
/// where credentials and clipboard data never leave the machine. Everything
/// else must use `https://` (a scheme-less host is upgraded to https).
fn validate_sync_endpoint(endpoint: &str) -> Result<String, String> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    // Scheme-less endpoints are upgraded to https by the transport, so they
    // stay plaintext-safe. Explicit schemes are normalized and parsed rather
    // than string-split: `http://127.0.0.1:80@evil.com` is userinfo plus a
    // remote host, not a loopback endpoint.
    let lower = trimmed.to_ascii_lowercase();
    let normalized = if lower.starts_with("http://") {
        format!("http://{}", &trimmed["http://".len()..])
    } else if lower.starts_with("https://") {
        format!("https://{}", &trimmed["https://".len()..])
    } else {
        return Ok(trimmed.to_string());
    };
    let parsed = reqwest::Url::parse(&normalized)
        .map_err(|error| format!("invalid S3 endpoint: {error}"))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("S3 endpoint must not contain credentials".to_string());
    }
    if parsed.scheme() == "http" {
        let host = parsed
            .host_str()
            .ok_or_else(|| "S3 endpoint host is empty".to_string())?;
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host == "127.0.0.1"
            || host == "::1"
            || host == "[::1]";
        if !loopback {
            return Err(
                "S3 endpoint must use https (plaintext http is only allowed for localhost)"
                    .to_string(),
            );
        }
    }
    Ok(normalized)
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
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
    };

    use super::*;
    use crate::{
        domain::ClipboardKind,
        sync::v1::{
            DownloadedFile, DownloadedObject, ObjectInfo, ObjectMetadata, PutCondition, PutOutcome,
            RecordVersion, ReplicatedItem,
        },
    };
    use v1::ObjectStore as _;

    #[test]
    fn usable_sync_settings_tolerates_a_selected_but_unconfigured_provider() {
        // The exact state that broke every media read: the provider is S3 while
        // nothing has been filled in yet.
        let mut config = SyncConfig {
            provider: SyncProvider::S3,
            ..SyncConfig::default()
        };
        assert!(usable_sync_settings(&config).is_none());

        // Partially filled is still unusable, and must not be mistaken for a
        // working configuration that happens to have a bucket.
        config.s3_bucket = Some("bucket".to_owned());
        assert!(usable_sync_settings(&config).is_none());

        // Off stays off.
        let off = SyncConfig::default();
        assert!(usable_sync_settings(&off).is_none());

        // Complete configuration is usable again — the strict check the
        // connection test relies on still rejects the states above.
        let mut complete = config.clone();
        complete.endpoint = Some("https://s3.example.com".to_owned());
        complete.s3_access_key = Some("key".to_owned());
        complete.s3_secret_key = Some("secret".to_owned());
        assert!(SyncSettings::from_sync_config(&complete).is_ok());
        assert!(usable_sync_settings(&complete).is_some());
        assert!(SyncSettings::from_sync_config(&config).is_err());
    }

    #[test]
    fn sync_endpoint_requires_https_except_loopback() {
        assert_eq!(
            validate_sync_endpoint("s3.amazonaws.com").unwrap(),
            "s3.amazonaws.com"
        );
        assert_eq!(
            validate_sync_endpoint("https://minio.example.com").unwrap(),
            "https://minio.example.com"
        );
        assert!(validate_sync_endpoint("http://127.0.0.1:9000").is_ok());
        assert!(validate_sync_endpoint("http://localhost:9000").is_ok());
        assert_eq!(
            validate_sync_endpoint("HTTP://[::1]:9000").unwrap(),
            "http://[::1]:9000"
        );
        assert!(validate_sync_endpoint("http://minio.example.com").is_err());
        assert!(validate_sync_endpoint("http://10.0.0.5:9000").is_err());

        // Userinfo must not be mistaken for a loopback host.
        assert!(validate_sync_endpoint("http://127.0.0.1:80@evil.com").is_err());
        assert!(validate_sync_endpoint("http://localhost:9000@evil.com").is_err());
        assert!(validate_sync_endpoint("http://[::1]:80@evil.com").is_err());
        assert!(validate_sync_endpoint("https://user:pass@example.com").is_err());
    }

    #[test]
    fn rotation_pointer_refuses_the_run_while_absence_passes() {
        assert!(rotation_refusal(None).is_ok());

        let pointer = serde_json::to_vec(&serde_json::json!({
            "newPrefix": "clipboard/rotated-abc",
            "rotatedAtMs": 1_700_000_000_000i64,
            "deviceId": "device-a",
        }))
        .unwrap();
        let error = rotation_refusal(Some(&pointer)).unwrap_err();
        assert!(error.contains("device-a"), "{error}");
        assert!(error.contains("clipboard/rotated-abc"), "{error}");

        // A malformed tombstone still refuses: only this codebase writes the key.
        let error = rotation_refusal(Some(b"not json")).unwrap_err();
        assert!(error.contains("rotated"), "{error}");
    }

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
            resource_gc_mode: "off".to_string(),
            max_sync_image_bytes: 5_242_880,
            max_sync_file_bytes: 10_485_760,
        }
    }

    #[test]
    fn resource_byte_limits_are_clamped_to_a_working_window() {
        let mut sync = configured_sync();
        sync.max_sync_image_bytes = 0;
        sync.max_sync_file_bytes = u64::MAX;
        let settings = SyncSettings::from_sync_config(&sync).unwrap();
        assert_eq!(settings.max_sync_image_bytes, MIN_SYNC_RESOURCE_BYTES);
        assert_eq!(settings.max_sync_file_bytes, MAX_SYNC_RESOURCE_BYTES);
    }

    #[test]
    fn remote_scope_is_stable_and_isolates_prefixes() {
        let first = SyncSettings::from_sync_config(&configured_sync()).unwrap();
        let mut equivalent_config = configured_sync();
        equivalent_config.endpoint = Some("http://127.0.0.1:9000".to_string());
        equivalent_config.remote_path = Some("clipboard".to_string());
        let equivalent = SyncSettings::from_sync_config(&equivalent_config).unwrap();
        equivalent_config.s3_access_key = Some("rotated-access-key".into());
        equivalent_config.s3_secret_key = Some("rotated-secret-key".into());
        let rotated = SyncSettings::from_sync_config(&equivalent_config).unwrap();
        assert_eq!(first.remote_scope_id(), rotated.remote_scope_id());
        assert_ne!(first.legacy_scope_id(), rotated.legacy_scope_id());
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
    fn connection_test_works_while_the_sync_switch_is_off() {
        let mut sync = configured_sync();
        sync.provider = SyncProvider::Off;
        // The test must validate the entered fields without requiring the
        // switch: users test before enabling.
        let settings = SyncSettings::validated_from_sync_config(&sync).unwrap();
        assert_eq!(settings.bucket, "clipboard");

        // Incomplete fields are still rejected by the test path.
        sync.s3_bucket = None;
        assert!(SyncSettings::validated_from_sync_config(&sync).is_err());
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let mut sync = configured_sync();
        sync.s3_secret_key = Some("s3-secret-value-xyz".to_string());
        sync.sync_password = Some("sync-password-xyz".to_string());
        let rendered = format!("{:?}", SyncSettings::from_sync_config(&sync).unwrap());
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("s3-secret-value-xyz"));
        assert!(!rendered.contains("sync-password-xyz"));
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
            skipped_resources: 0,
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
                        item: item.clone().into(),
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
    fn scope_materialization_downloads_missing_bodies_and_reuses_local_ones() {
        let paths = temporary_paths("scope-materialize");
        let database = Database::open(&paths.database).unwrap();
        database.initialize_sync().unwrap();
        let bytes = b"scope image bytes".repeat(64);
        let digest = hex::encode(Sha256::digest(&bytes));
        let object_key =
            v1::resource_object_key(v1::ResourceCategory::Image, &digest, "png").unwrap();
        let reference = v1::SyncResourceRef {
            slot: "image".to_string(),
            ordinal: 0,
            object_key: object_key.clone(),
        };
        let image_item = crate::domain::ClipboardItem {
            id: "scope-image".to_string(),
            kind: ClipboardKind::Image,
            title: "scope image".to_string(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "hash-scope-image".to_string(),
            source_app: None,
            icon_path: None,
            size_bytes: bytes.len() as u64,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: Some("{}".to_string()),
        };
        // A text record without references must be absent from the pass: the
        // bulk loader only reports items the scope actually carries refs for.
        let text_item = crate::domain::ClipboardItem {
            id: "scope-text".to_string(),
            kind: ClipboardKind::Text,
            title: "scope text".to_string(),
            text_content: Some("no resources here".to_string()),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "hash-scope-text".to_string(),
            source_app: None,
            icon_path: None,
            size_bytes: 17,
            created_at_ms: 2,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: Some("{}".to_string()),
        };
        let remote_scope = "d".repeat(64);
        database
            .apply_sync_snapshot_with_resources(
                &remote_scope,
                &v1::DeviceCursor {
                    device_id: "11111111-1111-4111-8111-111111111111".to_string(),
                    epoch: "22222222-2222-4222-8222-222222222222".to_string(),
                    sequence: 0,
                    last_segment_key: None,
                },
                &"b".repeat(64),
                &v1::MutationBatch {
                    upserts: vec![
                        ReplicatedItem {
                            item: image_item.into(),
                            version: RecordVersion {
                                modified_at_ms: 2,
                                writer_device_id: "11111111-1111-4111-8111-111111111111"
                                    .to_string(),
                            },
                        },
                        ReplicatedItem {
                            item: text_item.into(),
                            version: RecordVersion {
                                modified_at_ms: 2,
                                writer_device_id: "11111111-1111-4111-8111-111111111111"
                                    .to_string(),
                            },
                        },
                    ],
                    tombstones: Vec::new(),
                },
                &BTreeMap::from([("scope-image".to_string(), vec![reference.clone()])]),
            )
            .unwrap();
        let settings = SyncSettings::from_sync_config(&configured_sync()).unwrap();
        let store = MaterializationStore::default();
        store
            .objects
            .lock()
            .unwrap()
            .insert(object_key, bytes.clone());

        let first =
            materialize_scope_resources(&store, &database, &paths, &settings, &remote_scope, None)
                .unwrap();
        assert_eq!(first.items, 1);
        assert_eq!(first.downloaded_items, 1);
        assert_eq!(store.gets.load(Ordering::Relaxed), 1);
        let local_path = PathBuf::from(
            database
                .get_item("scope-image")
                .unwrap()
                .unwrap()
                .resource_path
                .unwrap(),
        );
        assert_eq!(fs::read(&local_path).unwrap(), bytes);

        // A second pass must be pure local verification: the rotation flow
        // runs this before every attempt and cannot re-download.
        let second =
            materialize_scope_resources(&store, &database, &paths, &settings, &remote_scope, None)
                .unwrap();
        assert_eq!(second.items, 1);
        assert_eq!(second.downloaded_items, 0);
        // The GET counter is cumulative across both passes: still the single
        // download from the first pass, i.e. the second pass did no I/O.
        assert_eq!(store.gets.load(Ordering::Relaxed), 1);

        drop(database);
        fs::remove_dir_all(paths.project).unwrap();
    }

    #[derive(Default)]
    struct RotationStore {
        /// Key → (bytes, raw quoted ETag), so `IfMatch` and head-cache
        /// comparison have something stable to compare against.
        objects: Mutex<BTreeMap<String, (Vec<u8>, String)>>,
        puts: AtomicU64,
        deletes: AtomicU64,
        gets: AtomicU64,
        /// Injects a deletion failure to model an interrupted cleanup.
        fail_deletes: AtomicBool,
    }

    fn fake_etag(bytes: &[u8]) -> String {
        format!("\"{}\"", hex::encode(&Sha256::digest(bytes)[..16]))
    }

    impl v1::ObjectStore for RotationStore {
        fn list(
            &self,
            prefix: &str,
            _start_after: Option<&str>,
        ) -> Result<Vec<ObjectInfo>, String> {
            let objects = self.objects.lock().map_err(|_| "lock poisoned")?;
            Ok(objects
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .map(|(key, (bytes, etag))| ObjectInfo {
                    key: key.clone(),
                    size_bytes: Some(bytes.len() as u64),
                    modified_ms: Some(1),
                    etag: Some(etag.clone()),
                })
                .collect())
        }

        fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
            self.gets.fetch_add(1, Ordering::Relaxed);
            Ok(self
                .objects
                .lock()
                .map_err(|_| "lock poisoned")?
                .get(key)
                .map(|(bytes, etag)| DownloadedObject {
                    bytes: bytes.clone(),
                    etag: Some(etag.clone()),
                }))
        }

        fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String> {
            Ok(self
                .objects
                .lock()
                .map_err(|_| "lock poisoned")?
                .get(key)
                .map(|(bytes, etag)| ObjectMetadata {
                    size_bytes: Some(bytes.len() as u64),
                    etag: Some(etag.clone()),
                }))
        }

        fn get_to_file(
            &self,
            key: &str,
            destination: &Path,
            max_bytes: u64,
        ) -> Result<Option<DownloadedFile>, String> {
            let Some((bytes, etag)) = self
                .objects
                .lock()
                .map_err(|_| "lock poisoned")?
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
                etag: Some(etag),
            }))
        }

        fn put(
            &self,
            key: &str,
            bytes: Vec<u8>,
            condition: PutCondition,
        ) -> Result<PutOutcome, String> {
            let mut objects = self.objects.lock().map_err(|_| "lock poisoned")?;
            match &condition {
                PutCondition::IfAbsent => {
                    if objects.contains_key(key) {
                        return Ok(PutOutcome::PreconditionFailed);
                    }
                }
                PutCondition::IfMatch(etag) => match objects.get(key) {
                    Some((_, current)) if current == etag => {}
                    _ => return Ok(PutOutcome::PreconditionFailed),
                },
                PutCondition::Unconditional => {}
            }
            let etag = fake_etag(&bytes);
            objects.insert(key.to_string(), (bytes, etag.clone()));
            self.puts.fetch_add(1, Ordering::Relaxed);
            Ok(PutOutcome::Stored { etag: Some(etag) })
        }

        fn put_file(
            &self,
            key: &str,
            path: &Path,
            sha256: &str,
            size_bytes: u64,
            condition: PutCondition,
        ) -> Result<PutOutcome, String> {
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            if bytes.len() as u64 != size_bytes || hex::encode(Sha256::digest(&bytes)) != sha256 {
                return Err("staged file does not match its digest".to_string());
            }
            self.put(key, bytes, condition)
        }

        fn delete(&self, key: &str) -> Result<(), String> {
            if self.fail_deletes.load(Ordering::Relaxed) {
                return Err("injected deletion failure".to_string());
            }
            if self
                .objects
                .lock()
                .map_err(|_| "lock poisoned")?
                .remove(key)
                .is_some()
            {
                self.deletes.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        }
    }

    #[test]
    fn namespace_rotation_moves_state_tombstones_the_old_prefix_and_refuses_twice() {
        fn image_record(id: &str, path: &Path) -> crate::domain::ClipboardItem {
            let path = path.to_string_lossy().to_string();
            crate::domain::ClipboardItem {
                id: id.to_string(),
                kind: ClipboardKind::Image,
                title: id.to_string(),
                text_content: None,
                html_content: None,
                rtf_content: None,
                resource_path: Some(path.clone()),
                preview_path: None,
                content_hash: format!("hash-{id}"),
                source_app: None,
                icon_path: None,
                size_bytes: fs::metadata(&path).unwrap().len(),
                created_at_ms: 1,
                last_used_at_ms: None,
                is_favorite: false,
                metadata_json: Some("{}".to_string()),
            }
        }

        // Source device captures the history, so the old namespace holds the
        // only copy of the image body.
        let source_paths = temporary_paths("rotate-source");
        fs::create_dir_all(&source_paths.images).unwrap();
        let image_bytes = b"rotating image payload".repeat(256);
        let source_image = source_paths.images.join("source.png");
        fs::write(&source_image, &image_bytes).unwrap();
        let source = Database::open(&source_paths.database).unwrap();
        source.initialize_sync().unwrap();
        source
            .save_item(&image_record("rotate-image", &source_image))
            .unwrap();
        source
            .save_item(&crate::domain::ClipboardItem {
                id: "rotate-text".to_string(),
                kind: ClipboardKind::Text,
                title: "rotate text".to_string(),
                text_content: Some("rotated record".to_string()),
                html_content: None,
                rtf_content: None,
                resource_path: None,
                preview_path: None,
                content_hash: "hash-rotate-text".to_string(),
                source_app: None,
                icon_path: None,
                size_bytes: 14,
                created_at_ms: 2,
                last_used_at_ms: None,
                is_favorite: false,
                metadata_json: Some("{}".to_string()),
            })
            .unwrap();

        let old_settings = SyncSettings::from_sync_config(&configured_sync()).unwrap();
        let old_scope = old_settings.remote_scope_id();
        let old_store = RotationStore::default();
        let (binding, key) = clipboard_sync::v1::namespace::resolve_namespace(
            &old_store,
            &old_scope,
            &old_settings.legacy_scope_id(),
            old_settings.sync_password.as_deref(),
            None,
        )
        .unwrap();
        clipboard_sync::v1::engine::sync_database(
            &old_store,
            &source,
            &v1::SyncEnginePaths::from(&source_paths),
            &binding.scope,
            key.as_ref(),
            old_settings.engine_options(),
        )
        .unwrap();

        // Target device synced the metadata but never materialized the body —
        // the exact precondition rotation must repair.
        let target_paths = temporary_paths("rotate-target");
        let target = Database::open(&target_paths.database).unwrap();
        target.initialize_sync().unwrap();
        clipboard_sync::v1::engine::sync_database(
            &old_store,
            &target,
            &v1::SyncEnginePaths::from(&target_paths),
            &binding.scope,
            key.as_ref(),
            old_settings.engine_options(),
        )
        .unwrap();
        assert!(target
            .get_item("rotate-image")
            .unwrap()
            .unwrap()
            .resource_path
            .is_none());

        let new_password = "brand-new-rotation-password";
        let new_prefix = "clipboard-rotated-42";
        let new_settings =
            old_settings.with_rotated_scope(new_prefix.to_string(), Some(new_password.to_string()));
        let new_store = RotationStore::default();
        let token = clipboard_sync::cancellation::CancellationToken::default();
        let mut config_committed = false;
        let outcome = perform_namespace_rotation(
            &old_store,
            &new_store,
            &target,
            &target_paths,
            &old_settings,
            &new_settings,
            Some(new_password),
            &token,
            &mut || {
                config_committed = true;
                Ok(())
            },
            &mut |_| {},
        )
        .unwrap();
        assert!(
            config_committed,
            "the local configuration must move before the old namespace is deleted"
        );

        assert_eq!(outcome.new_prefix, new_prefix);
        assert_eq!(outcome.materialized_items, 1);
        assert!(outcome.uploaded_entries >= 2);
        assert!(outcome.uploaded_resources >= 1);
        assert!(outcome.deleted_objects >= 3);

        // Old prefix: tombstone survives, the whole engine namespace is gone.
        let old_keys: Vec<String> = old_store
            .list("", None)
            .unwrap()
            .into_iter()
            .map(|object| object.key)
            .collect();
        assert_eq!(old_keys, vec!["rotated-to".to_string()]);
        let tombstone: serde_json::Value =
            serde_json::from_slice(&old_store.get("rotated-to").unwrap().unwrap().bytes).unwrap();
        assert_eq!(tombstone["newPrefix"], serde_json::json!(new_prefix));
        assert!(tombstone["keyCheck"].is_string());
        assert!(tombstone["deviceId"].is_string());

        // New prefix: a complete namespace, including the image body under the
        // new keyed digest.
        let new_keys: Vec<String> = new_store
            .list("", None)
            .unwrap()
            .into_iter()
            .map(|object| object.key)
            .collect();
        assert!(new_keys.iter().any(|key| key == "v1/namespace.json"));
        assert!(new_keys
            .iter()
            .any(|key| key.starts_with("v1/resources/image/")));
        assert!(new_keys.iter().any(|key| key.starts_with("v1/heads/")));

        // The materialized body is local, byte-identical, and the record kept
        // its canonical reference.
        let rotated = target.get_item("rotate-image").unwrap().unwrap();
        assert_eq!(
            fs::read(rotated.resource_path.as_deref().unwrap()).unwrap(),
            image_bytes
        );
        assert_eq!(
            target
                .list_sync_resource_refs_for_scope(&new_settings.remote_scope_id())
                .unwrap()
                .len(),
            1,
            "the new scope carries the image reference"
        );

        // A second rotation on the same old prefix refuses at the tombstone.
        let error = perform_namespace_rotation(
            &old_store,
            &new_store,
            &target,
            &target_paths,
            &old_settings,
            &new_settings,
            Some(new_password),
            &token,
            &mut || Ok(()),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.contains("rotated"), "{error}");

        drop(source);
        drop(target);
        fs::remove_dir_all(source_paths.project).unwrap();
        fs::remove_dir_all(target_paths.project).unwrap();
    }

    /// The crash window that motivated the commit ordering: when the old
    /// namespace cannot be deleted (crash, provider error), the rotation must
    /// have already moved the local configuration. Otherwise the tombstone —
    /// which makes every run at the old prefix refuse — would strand this
    /// device forever.
    #[test]
    fn rotation_commits_the_configuration_before_deleting_the_old_namespace() {
        let paths = temporary_paths("rotate-interrupted");
        let database = Database::open(&paths.database).unwrap();
        database.initialize_sync().unwrap();
        let image_bytes = b"interrupted rotation image".repeat(64);
        let digest = hex::encode(Sha256::digest(&image_bytes));
        let object_key =
            v1::resource_object_key(v1::ResourceCategory::Image, &digest, "png").unwrap();
        let reference = v1::SyncResourceRef {
            slot: "image".to_string(),
            ordinal: 0,
            object_key: object_key.clone(),
        };
        let item = crate::domain::ClipboardItem {
            id: "interrupted-image".to_string(),
            kind: ClipboardKind::Image,
            title: "interrupted image".to_string(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "hash-interrupted-image".to_string(),
            source_app: None,
            icon_path: None,
            size_bytes: image_bytes.len() as u64,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: Some("{}".to_string()),
        };
        let settings = SyncSettings::from_sync_config(&configured_sync()).unwrap();
        // The rotation materializes the references recorded under the scope
        // its namespace resolution derives from the settings.
        let old_scope = settings.remote_scope_id();
        database
            .apply_sync_snapshot_with_resources(
                &old_scope,
                &v1::DeviceCursor {
                    device_id: "11111111-1111-4111-8111-111111111111".to_string(),
                    epoch: "22222222-2222-4222-8222-222222222222".to_string(),
                    sequence: 0,
                    last_segment_key: None,
                },
                &"b".repeat(64),
                &v1::MutationBatch {
                    upserts: vec![ReplicatedItem {
                        item: item.into(),
                        version: RecordVersion {
                            modified_at_ms: 2,
                            writer_device_id: "11111111-1111-4111-8111-111111111111".to_string(),
                        },
                    }],
                    tombstones: Vec::new(),
                },
                &BTreeMap::from([("interrupted-image".to_string(), vec![reference.clone()])]),
            )
            .unwrap();
        let old_store = RotationStore::default();
        let etag = fake_etag(&image_bytes);
        old_store
            .objects
            .lock()
            .unwrap()
            .insert(object_key, (image_bytes.clone(), etag));

        let new_password = "brand-new-rotation-password";
        let new_prefix = "clipboard-rotated-interrupted";
        let new_settings =
            settings.with_rotated_scope(new_prefix.to_string(), Some(new_password.to_string()));
        // The old namespace cannot be cleaned up.
        old_store.fail_deletes.store(true, Ordering::Relaxed);
        let new_store = RotationStore::default();
        let token = clipboard_sync::cancellation::CancellationToken::default();
        let mut config_committed = false;
        let error = perform_namespace_rotation(
            &old_store,
            &new_store,
            &database,
            &paths,
            &settings,
            &new_settings,
            Some(new_password),
            &token,
            &mut || {
                config_committed = true;
                Ok(())
            },
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.contains("injected deletion failure"), "{error}");
        assert!(
            config_committed,
            "the configuration must already point at the new prefix when deletion fails"
        );

        // The tombstone is durable and the new namespace is complete, so a
        // re-pointed device can sync; the old objects are bounded leftovers.
        let old_keys: Vec<String> = old_store
            .list("", None)
            .unwrap()
            .into_iter()
            .map(|object| object.key)
            .collect();
        assert!(old_keys.contains(&"rotated-to".to_string()));
        assert!(
            old_keys.iter().any(|key| key.starts_with("v1/")),
            "the interrupted deletion leaves the old objects behind, which is recoverable"
        );
        let new_keys: Vec<String> = new_store
            .list("", None)
            .unwrap()
            .into_iter()
            .map(|object| object.key)
            .collect();
        assert!(new_keys.iter().any(|key| key == "v1/namespace.json"));
        assert!(new_keys
            .iter()
            .any(|key| key.starts_with("v1/resources/image/")));

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
                        item: item.into(),
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
