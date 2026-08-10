use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tauri::Manager;

use crate::config::{ConfigStore, SyncConfig};
use crate::storage::Database;
use crate::storage::StoragePaths;
use crate::sync;
use crate::sync::PoolStorage;

mod auto;

/// Serializes sync runs. The manual `sync_upload_backup` command and the
/// auto-sync worker share the same merge/apply logic, which is not reentrant;
/// a `try_lock` in each entry point prevents concurrent runs from
/// interleaving oplog merge/apply/cleanup.
static SYNC_RUN_LOCK: Mutex<()> = Mutex::new(());

pub(crate) use auto::AutoSyncWorker;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfigInfo {
    provider: String,
    endpoint: Option<String>,
    remote_path: Option<String>,
    username: Option<String>,
    has_password: bool,
    s3_region: Option<String>,
    s3_bucket: Option<String>,
    s3_access_key: Option<String>,
    has_s3_secret_key: bool,
    has_sync_password: bool,
    last_sync_ms: Option<i64>,
    last_sync_status: Option<String>,
    unsynced_count: u64,
    auto_sync: bool,
    auto_sync_interval_secs: u64,
    max_remote_oplog_files: u32,
    oplog_rollover_entries: u32,
    oplog_rollover_size_bytes: u32,
    max_sync_image_bytes: u64,
    max_sync_file_bytes: u64,
    compaction_suggested: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBackupEntry {
    name: String,
    is_directory: bool,
    size_bytes: Option<u64>,
    modified_ms: Option<i64>,
}

impl From<sync::WebDavEntry> for RemoteBackupEntry {
    fn from(entry: sync::WebDavEntry) -> Self {
        Self {
            name: entry.name,
            is_directory: entry.is_directory,
            size_bytes: entry.size_bytes,
            modified_ms: entry.modified_ms,
        }
    }
}

impl From<sync::S3Entry> for RemoteBackupEntry {
    fn from(entry: sync::S3Entry) -> Self {
        Self {
            name: entry.name,
            is_directory: entry.is_directory,
            size_bytes: entry.size_bytes,
            modified_ms: entry.modified_ms,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncUploadResult {
    pub uploaded_entries: u64,
    pub downloaded_entries: u64,
    pub applied_entries: u64,
    pub deleted_remote_files: u64,
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
}

/// Immutable snapshot of every sync-relevant config value, captured once under
/// the `ConfigStore` lock so network operations never hold that lock.
#[derive(Debug, Clone)]
struct SyncSettings {
    provider: crate::config::SyncProvider,
    endpoint: Option<String>,
    remote_path: Option<String>,
    username: Option<String>,
    password: Option<String>,
    s3_region: String,
    s3_bucket: Option<String>,
    s3_access_key: Option<String>,
    s3_secret_key: Option<String>,
    sync_password: Option<String>,
    max_remote_oplog_files: usize,
    oplog_rollover_entries: usize,
    max_sync_image_bytes: u64,
    max_sync_file_bytes: u64,
}

impl SyncSettings {
    fn from_config(config: &ConfigStore) -> Self {
        let sync = config.sync_config();
        Self {
            provider: sync.provider,
            endpoint: sync.endpoint.clone(),
            remote_path: sync.remote_path.clone(),
            username: sync.username.clone(),
            password: sync.password.clone(),
            s3_region: config.s3_region(),
            s3_bucket: config.s3_bucket(),
            s3_access_key: config.s3_access_key(),
            s3_secret_key: config.s3_secret_key(),
            sync_password: sync.sync_password.clone(),
            max_remote_oplog_files: config.max_remote_oplog_files() as usize,
            oplog_rollover_entries: config.oplog_rollover_entries() as usize,
            max_sync_image_bytes: config.max_sync_image_bytes(),
            max_sync_file_bytes: config.max_sync_file_bytes(),
        }
    }

    fn remote_scope_id(&self) -> Result<String, String> {
        let endpoint = self
            .endpoint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "sync endpoint not configured".to_string())?
            .trim_end_matches('/');
        let remote_path = self
            .remote_path
            .as_deref()
            .unwrap_or_default()
            .trim_matches('/');
        let identity = match self.provider {
            crate::config::SyncProvider::Webdav => format!(
                "webdav\n{endpoint}\n{remote_path}\n{}",
                self.username.as_deref().unwrap_or_default()
            ),
            crate::config::SyncProvider::S3 => format!(
                "s3\n{endpoint}\n{remote_path}\n{}\n{}\n{}",
                self.s3_region,
                self.s3_bucket.as_deref().unwrap_or_default(),
                self.s3_access_key.as_deref().unwrap_or_default()
            ),
            crate::config::SyncProvider::Off => {
                return Err("sync provider is disabled".to_string());
            }
        };
        Ok(hex::encode(Sha256::digest(identity.as_bytes())))
    }
}

#[tauri::command]
pub fn get_sync_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    database: tauri::State<'_, Database>,
) -> Result<SyncConfigInfo, String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    let settings = SyncSettings::from_config(&guard);
    let remote_scope = settings.remote_scope_id().ok();
    let sync = guard.sync_config();
    let auto_sync = guard.auto_sync();
    let auto_sync_interval_secs = guard.auto_sync_interval_secs();
    let max_remote_oplog_files = guard.max_remote_oplog_files();
    let oplog_rollover_entries = guard.oplog_rollover_entries();
    let oplog_rollover_size_bytes = guard.oplog_rollover_size_bytes();
    let max_sync_image_bytes = guard.max_sync_image_bytes();
    let max_sync_file_bytes = guard.max_sync_file_bytes();
    let unsynced = database.count_unsynced_changelog().unwrap_or(0);
    let remote_oplog_count = remote_scope
        .as_deref()
        .and_then(|scope| database.get_sync_remote_oplog_count(scope).unwrap_or(None));
    let remote_baseline_ms = remote_scope.as_deref().and_then(|scope| {
        database
            .get_sync_remote_baseline_modified_ms(scope)
            .unwrap_or(None)
    });
    let max_files = guard.max_remote_oplog_files() as u64;
    let compaction_suggested = match remote_oplog_count {
        Some(count) if count >= max_files => true,
        _ => match remote_baseline_ms {
            Some(ms) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;
                now_ms.saturating_sub(ms) >= 30 * 24 * 60 * 60 * 1000
            }
            None => false,
        },
    };
    Ok(SyncConfigInfo {
        provider: match sync.provider {
            crate::config::SyncProvider::Off => "off",
            crate::config::SyncProvider::Webdav => "webdav",
            crate::config::SyncProvider::S3 => "s3",
        }
        .to_string(),
        endpoint: sync.endpoint,
        remote_path: sync.remote_path,
        username: sync.username,
        has_password: sync.password.is_some(),
        s3_region: sync.s3_region,
        s3_bucket: sync.s3_bucket,
        s3_access_key: sync.s3_access_key,
        has_s3_secret_key: sync.s3_secret_key.is_some(),
        has_sync_password: sync.sync_password.is_some(),
        last_sync_ms: sync.last_sync_ms,
        last_sync_status: sync.last_sync_status,
        unsynced_count: unsynced,
        auto_sync,
        auto_sync_interval_secs,
        max_remote_oplog_files,
        oplog_rollover_entries,
        oplog_rollover_size_bytes,
        max_sync_image_bytes,
        max_sync_file_bytes,
        compaction_suggested,
    })
}

fn retain_secret(candidate: Option<String>, previous: Option<String>) -> Option<String> {
    candidate.or(previous)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_sync_config(
    provider: String,
    endpoint: Option<String>,
    remote_path: Option<String>,
    username: Option<String>,
    password: Option<String>,
    auto_sync: bool,
    auto_sync_interval_secs: u64,
    max_remote_oplog_files: u32,
    oplog_rollover_entries: u32,
    oplog_rollover_size_bytes: u32,
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
        .map_err(|_| "configuration lock is poisoned".to_owned())?;

    let prev = guard.sync_config();

    let provider = match provider.as_str() {
        "webdav" => crate::config::SyncProvider::Webdav,
        "s3" => crate::config::SyncProvider::S3,
        _ => crate::config::SyncProvider::Off,
    };

    let sync = SyncConfig {
        provider,
        endpoint,
        remote_path,
        username,
        password: retain_secret(password, prev.password),
        last_sync_ms: prev.last_sync_ms,
        last_sync_status: prev.last_sync_status,
        auto_sync,
        auto_sync_interval_secs: auto_sync_interval_secs.clamp(10, 86400),
        max_remote_oplog_files: max_remote_oplog_files.clamp(3, 100),
        oplog_rollover_entries: oplog_rollover_entries.clamp(10, 10000),
        oplog_rollover_size_bytes: oplog_rollover_size_bytes.clamp(1024, 1048576),
        max_sync_image_bytes,
        max_sync_file_bytes,
        s3_region,
        s3_bucket,
        s3_access_key,
        s3_secret_key: retain_secret(s3_secret_key, prev.s3_secret_key),
        sync_password: retain_secret(sync_password, prev.sync_password),
        ..Default::default()
    };

    guard.set_sync_config(sync).map_err(|e| e.to_string())
}

fn resolve_s3_config(settings: &SyncSettings) -> (String, String, String, String) {
    (
        settings.s3_region.clone(),
        settings.s3_bucket.clone().unwrap_or_default(),
        settings.s3_access_key.clone().unwrap_or_default(),
        settings.s3_secret_key.clone().unwrap_or_default(),
    )
}

/// WebDAV-backed remote resource pool. Objects live at
/// `<remote_path>/resources/<rel_path>` and use the same per-payload sync
/// password when encryption is configured.
struct WebDavPool<'a> {
    endpoint: &'a str,
    remote_path: &'a str,
    username: Option<&'a str>,
    password: Option<&'a str>,
    remote_scope: &'a str,
    settings: &'a SyncSettings,
}

impl sync::PoolStorage for WebDavPool<'_> {
    fn scope_key(&self) -> &str {
        self.remote_scope
    }

    fn upload(&self, rel_path: &str, bytes: &[u8]) -> Result<(), String> {
        let object = sync::pool_object_path(rel_path);
        let data = encrypt_if_configured(bytes.to_vec(), self.settings)?;
        sync::upload_to_webdav(
            self.endpoint,
            self.remote_path,
            &object,
            data,
            self.username,
            self.password,
        )
    }
    fn download(&self, rel_path: &str) -> Result<Vec<u8>, String> {
        let object = sync::pool_object_path(rel_path);
        let data = sync::download_from_webdav(
            self.endpoint,
            self.remote_path,
            &object,
            self.username,
            self.password,
        )?;
        decrypt_if_configured(data, self.settings)
    }
}

/// S3-backed pool of remote resources. Objects live at
/// `<remote_path>/resources/<rel_path>` (or `resources/<rel_path>` when no
/// remote path is configured) and are encrypted when a sync password is set.
struct S3Pool<'a> {
    endpoint: &'a str,
    region: &'a str,
    bucket: &'a str,
    access_key: &'a str,
    secret_key: &'a str,
    remote_path: &'a str,
    remote_scope: &'a str,
    settings: &'a SyncSettings,
}

impl sync::PoolStorage for S3Pool<'_> {
    fn scope_key(&self) -> &str {
        self.remote_scope
    }

    fn upload(&self, rel_path: &str, bytes: &[u8]) -> Result<(), String> {
        let object = sync::pool_object_path(rel_path);
        let key = s3_object_key(self.remote_path, &object);
        let data = encrypt_if_configured(bytes.to_vec(), self.settings)?;
        sync::upload_to_s3(
            self.endpoint,
            self.region,
            self.bucket,
            &key,
            data,
            self.access_key,
            self.secret_key,
        )
    }
    fn download(&self, rel_path: &str) -> Result<Vec<u8>, String> {
        let object = sync::pool_object_path(rel_path);
        let key = s3_object_key(self.remote_path, &object);
        let data = sync::download_from_s3(
            self.endpoint,
            self.region,
            self.bucket,
            &key,
            self.access_key,
            self.secret_key,
        )?;
        decrypt_if_configured(data, self.settings)
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn test_sync_connection(
    provider: String,
    endpoint: String,
    remote_path: Option<String>,
    username: Option<String>,
    password: Option<String>,
    s3_region: Option<String>,
    s3_bucket: Option<String>,
    s3_access_key: Option<String>,
    s3_secret_key: Option<String>,
) -> Result<String, String> {
    match provider.as_str() {
        "webdav" => {
            let result = sync::test_webdav_connection(
                &endpoint,
                remote_path.as_deref(),
                username.as_deref(),
                password.as_deref(),
            );
            serde_json::to_string(&result).map_err(|e| e.to_string())
        }
        "s3" => {
            let region = s3_region.unwrap_or_else(|| "us-east-1".to_string());
            let bucket = s3_bucket.ok_or("S3 bucket not configured")?;
            let access_key = s3_access_key.ok_or("S3 access key not configured")?;
            let secret_key = s3_secret_key.ok_or("S3 secret key not configured")?;
            let result =
                sync::test_s3_connection(&endpoint, &region, &bucket, &access_key, &secret_key);
            serde_json::to_string(&result).map_err(|e| e.to_string())
        }
        _ => Err("unsupported provider".to_string()),
    }
}

#[tauri::command]
pub fn sync_upload_backup(app: tauri::AppHandle) -> Result<SyncUploadResult, String> {
    // Resolve the managed states through the handle so the same path is shared
    // with the auto-sync worker (which only holds an `AppHandle`).
    run_sync(&app)
}

/// Runs one full sync (upload + download + apply + cleanup) using the managed
/// config/database/paths states resolved from `app`. The manual command and the
/// auto-sync worker both enter here.
fn run_sync(app: &tauri::AppHandle) -> Result<SyncUploadResult, String> {
    let config = app.state::<Mutex<ConfigStore>>();
    let database = app.state::<Database>();
    let paths = app.state::<StoragePaths>();

    // Fail fast instead of queueing: a concurrent sync (manual command racing
    // with the auto-sync worker) would interleave the non-reentrant
    // merge/apply/cleanup logic.
    let _run_guard = SYNC_RUN_LOCK
        .try_lock()
        .map_err(|_| "sync already in progress".to_owned())?;

    // Snapshot the config, then release the lock before any network I/O so a
    // slow sync does not block other config access.
    let settings = {
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        SyncSettings::from_config(&guard)
    };

    let endpoint = settings
        .endpoint
        .clone()
        .ok_or("sync endpoint not configured")?;
    let remote_path = settings.remote_path.clone().unwrap_or_default();
    let remote_scope = settings.remote_scope_id()?;
    let device_id = get_device_id();

    let temp_dir = std::env::temp_dir().join("clipboard-sync");
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();

    let provider = settings.provider;
    let result = match provider {
        crate::config::SyncProvider::Webdav => sync_upload_webdav(
            app,
            &settings,
            &database,
            &paths,
            &endpoint,
            &remote_path,
            &remote_scope,
            &device_id,
            &timestamp,
            &temp_dir,
        ),
        crate::config::SyncProvider::S3 => sync_upload_s3(
            app,
            &settings,
            &database,
            &paths,
            &endpoint,
            &remote_path,
            &remote_scope,
            &device_id,
            &timestamp,
            &temp_dir,
        ),
        _ => Err("unsupported provider".to_string()),
    };

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    if result.is_ok() {
        let mut guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        let _ = guard.update_sync_status("success", now_ms);
    }

    result
}

#[allow(clippy::too_many_arguments)]
fn sync_upload_webdav(
    app: &tauri::AppHandle,
    settings: &SyncSettings,
    database: &Database,
    paths: &crate::storage::StoragePaths,
    endpoint: &str,
    remote_path: &str,
    remote_scope: &str,
    device_id: &str,
    timestamp: &str,
    temp_dir: &std::path::Path,
) -> Result<SyncUploadResult, String> {
    let mut bytes_uploaded: u64 = 0;
    let mut bytes_downloaded: u64 = 0;
    let mut applied_remote = false;

    let pool = WebDavPool {
        endpoint,
        remote_path,
        username: settings.username.as_deref(),
        password: settings.password.as_deref(),
        remote_scope,
        settings,
    };

    let has_prior_sync = database
        .is_sync_remote_initialized(remote_scope)
        .map_err(|e| e.to_string())?;
    let remote_files = sync::list_webdav_files(
        endpoint,
        Some(remote_path),
        settings.username.as_deref(),
        settings.password.as_deref(),
    )?;
    record_remote_stats(database, remote_scope, &remote_files);

    let remote_baselines: Vec<_> = remote_files
        .iter()
        .filter(|e| !e.is_directory && e.name.starts_with("baseline-") && e.name.ends_with(".zip"))
        .collect();

    if !remote_baselines.is_empty() {
        if !has_prior_sync {
            // First sync: remote baselines may be several disjoint full
            // snapshots produced by concurrent first syncs of separate devices
            // (no common root, like unrelated histories). Import every one,
            // merged into a superset, so no device's data is dropped.
            let mut merged_payloads: Vec<Vec<u8>> = Vec::new();
            for baseline in &remote_baselines {
                println!("[sync] downloading remote baseline: {}", baseline.name);
                let data = sync::download_from_webdav(
                    endpoint,
                    remote_path,
                    &baseline.name,
                    settings.username.as_deref(),
                    settings.password.as_deref(),
                )?;
                let data = decrypt_if_configured(data, settings)?;
                bytes_downloaded += data.len() as u64;
                merged_payloads.push(data);
            }
            let (mut merged_items, merged_resources) =
                sync::merge_baseline_archives(&merged_payloads)?;

            sync::absorb_pool_paths(paths, &merged_resources, &pool);

            // Self-heal: collapse the redundant disjoint baselines into one
            // superset baseline so the remote converges back to a single source
            // of truth. Done before import so the local copies stay valid.
            if remote_baselines.len() > 1 {
                let mut pooled_resources = merged_resources.clone();
                sync::prepare_pool_refs(paths, &mut pooled_resources, &pool);
                let filename = format!("baseline-{device_id}-{timestamp}.zip");
                let merged_path = temp_dir.join(&filename);
                sync::write_baseline_zip(
                    &merged_path,
                    &merged_items,
                    &pooled_resources,
                    device_id,
                )?;
                let data = std::fs::read(&merged_path).map_err(|e| e.to_string())?;
                let data = encrypt_if_configured(data, settings)?;
                bytes_uploaded += data.len() as u64;
                sync::upload_to_webdav(
                    endpoint,
                    remote_path,
                    &filename,
                    data,
                    settings.username.as_deref(),
                    settings.password.as_deref(),
                )?;
                for baseline in &remote_baselines {
                    let _ = sync::delete_from_webdav(
                        endpoint,
                        remote_path,
                        &baseline.name,
                        settings.username.as_deref(),
                        settings.password.as_deref(),
                    );
                }
                println!(
                    "[sync] consolidated {} baselines into one",
                    remote_baselines.len()
                );
            }

            sync::materialize_resources(&merged_resources, paths, Some(&|rel| pool.download(rel)))?;
            sync::rewrite_item_paths_to_local(&mut merged_items, paths);
            let imported = database
                .import_baseline_items(&merged_items)
                .map_err(|e| e.to_string())?;
            println!("[sync] baseline imported: {} items", imported);
            database
                .mark_sync_remote_initialized(remote_scope)
                .map_err(|e| e.to_string())?;
            applied_remote = true;
        }
    } else if !has_prior_sync {
        println!("[sync] no remote baseline found, uploading local baseline");
        let filename = format!("baseline-{device_id}-{timestamp}.zip");
        let baseline_path = temp_dir.join(&filename);
        let manifest = sync::create_baseline_backup(database, paths, &baseline_path, Some(&pool))?;
        let data = std::fs::read(&baseline_path).map_err(|e| e.to_string())?;
        let data = encrypt_if_configured(data, settings)?;
        bytes_uploaded += data.len() as u64;
        sync::upload_to_webdav(
            endpoint,
            remote_path,
            &filename,
            data,
            settings.username.as_deref(),
            settings.password.as_deref(),
        )?;
        println!("[sync] baseline uploaded: {} items", manifest.item_count);
        database
            .mark_sync_remote_initialized(remote_scope)
            .map_err(|e| e.to_string())?;
    }

    let rollover_entries = settings.oplog_rollover_entries;
    let max_image_bytes = settings.max_sync_image_bytes;
    let max_file_bytes = settings.max_sync_file_bytes;
    let local_entries = database
        .get_unsynced_changelog(rollover_entries)
        .map_err(|e| e.to_string())?;
    let local_entries =
        filter_entries_by_resource_size(local_entries, max_image_bytes, max_file_bytes, paths);

    let mut written_filename: String = String::new();
    let uploaded_entries = if !local_entries.is_empty() {
        let (all_entries, all_resources) = sync::collect_entry_resources(local_entries, paths);
        let plaintext =
            crate::sync::wire::serialize_oplog_with_resources(&all_entries, &all_resources)?;
        let filename = immutable_oplog_filename(device_id, &all_entries, &plaintext);
        let new_data = encrypt_if_configured(plaintext, settings)?;
        bytes_uploaded += new_data.len() as u64;
        sync::upload_to_webdav(
            endpoint,
            remote_path,
            &filename,
            new_data,
            settings.username.as_deref(),
            settings.password.as_deref(),
        )?;
        let max_seq = all_entries.iter().map(|e| e.sequence).max().unwrap_or(0);
        let _ = database.mark_changelog_synced(max_seq);
        let _ = database.purge_synced_changelog(1000);
        written_filename = filename;
        all_entries.len() as u64
    } else {
        0
    };

    let mut downloaded_entries: u64 = 0;
    let mut applied_entries: u64 = 0;
    let mut applied_objects = database
        .get_applied_sync_remote_oplogs(remote_scope)
        .map_err(|e| e.to_string())?;

    for entry in &remote_files {
        if entry.is_directory || !entry.name.starts_with("oplog-") {
            continue;
        }
        if entry.name.starts_with(&format!("oplog-{device_id}-")) {
            continue;
        }
        let revision = remote_oplog_revision(entry);
        if revision.as_ref().is_some_and(|revision| {
            applied_objects
                .get(&entry.name)
                .is_some_and(|applied| applied == revision)
        }) {
            continue;
        }
        match sync::download_from_webdav(
            endpoint,
            remote_path,
            &entry.name,
            settings.username.as_deref(),
            settings.password.as_deref(),
        ) {
            Ok(data) => {
                let data = decrypt_if_configured(data, settings)?;
                bytes_downloaded += data.len() as u64;
                let (mut remote_entries, resources): (
                    Vec<crate::storage::SyncChangeLogEntry>,
                    Vec<crate::sync::wire::OplogResource>,
                ) = match crate::sync::wire::deserialize_oplog_with_resources(&data) {
                    Ok(e) => e,
                    Err(e) => {
                        println!("[sync] failed to parse {}: {}", entry.name, e);
                        continue;
                    }
                };
                if let Err(e) =
                    sync::materialize_resources(&resources, paths, Some(&|rel| pool.download(rel)))
                {
                    println!("[sync] failed to materialize {}: {}", entry.name, e);
                    continue;
                }
                sync::rewrite_to_local(&mut remote_entries, paths);
                downloaded_entries += remote_entries.len() as u64;
                match database.apply_remote_oplog(&remote_entries) {
                    Ok(applied) => {
                        applied_entries += applied;
                        if let Some(revision) = revision.as_deref() {
                            database
                                .mark_sync_remote_oplog_applied(remote_scope, &entry.name, revision)
                                .map_err(|e| e.to_string())?;
                            applied_objects.insert(entry.name.clone(), revision.to_string());
                        }
                        if applied > 0 {
                            applied_remote = true;
                        }
                    }
                    Err(e) => println!("[sync] apply error: {}", e),
                }
            }
            Err(e) => println!("[sync] download error for {}: {}", entry.name, e),
        }
    }

    let max_files = settings.max_remote_oplog_files;
    let deleted_remote_files = cleanup_old_remote_oplogs(
        endpoint,
        remote_path,
        &settings.username,
        &settings.password,
        &written_filename,
        max_files,
    )?;

    if applied_remote {
        let _ = app.emit(
            "clipboard-history-invalidated",
            crate::commands::clipboard::ClipboardHistoryInvalidated {
                deleted_ids: Vec::new(),
            },
        );
    }

    Ok(SyncUploadResult {
        uploaded_entries,
        downloaded_entries,
        applied_entries,
        deleted_remote_files,
        bytes_uploaded,
        bytes_downloaded,
    })
}

#[allow(clippy::too_many_arguments)]
fn sync_upload_s3(
    app: &tauri::AppHandle,
    settings: &SyncSettings,
    database: &Database,
    paths: &crate::storage::StoragePaths,
    endpoint: &str,
    remote_path: &str,
    remote_scope: &str,
    device_id: &str,
    timestamp: &str,
    temp_dir: &std::path::Path,
) -> Result<SyncUploadResult, String> {
    let (region, bucket, access_key, secret_key) = resolve_s3_config(settings);
    let mut bytes_uploaded: u64 = 0;
    let mut bytes_downloaded: u64 = 0;
    let mut applied_remote = false;

    let pool = S3Pool {
        endpoint,
        region: &region,
        bucket: &bucket,
        access_key: &access_key,
        secret_key: &secret_key,
        remote_path,
        remote_scope,
        settings,
    };

    let prefix: Option<&str> = if remote_path.is_empty() {
        None
    } else {
        Some(remote_path)
    };
    let has_prior_sync = database
        .is_sync_remote_initialized(remote_scope)
        .map_err(|e| e.to_string())?;
    let remote_objects =
        sync::list_s3_objects(endpoint, &region, &bucket, prefix, &access_key, &secret_key)?;
    record_remote_stats(database, remote_scope, &remote_objects);

    let remote_baselines: Vec<_> = remote_objects
        .iter()
        .filter(|e| !e.is_directory && e.name.starts_with("baseline-") && e.name.ends_with(".zip"))
        .collect();

    if !remote_baselines.is_empty() {
        if !has_prior_sync {
            // First sync: remote baselines may be several disjoint full
            // snapshots produced by concurrent first syncs of separate devices
            // (no common root, like unrelated histories). Import every one,
            // merged into a superset, so no device's data is dropped.
            let mut merged_payloads: Vec<Vec<u8>> = Vec::new();
            for baseline in &remote_baselines {
                println!("[sync] downloading S3 baseline: {}", baseline.name);
                let data = sync::download_from_s3(
                    endpoint,
                    &region,
                    &bucket,
                    &s3_object_key(remote_path, &baseline.name),
                    &access_key,
                    &secret_key,
                )?;
                let data = decrypt_if_configured(data, settings)?;
                bytes_downloaded += data.len() as u64;
                merged_payloads.push(data);
            }
            let (mut merged_items, merged_resources) =
                sync::merge_baseline_archives(&merged_payloads)?;

            sync::absorb_pool_paths(paths, &merged_resources, &pool);

            // Self-heal: collapse the redundant disjoint baselines into one
            // superset baseline so the remote converges back to a single source
            // of truth.
            if remote_baselines.len() > 1 {
                let mut pooled_resources = merged_resources.clone();
                sync::prepare_pool_refs(paths, &mut pooled_resources, &pool);
                let filename = format!("baseline-{device_id}-{timestamp}.zip");
                let merged_path = temp_dir.join(&filename);
                sync::write_baseline_zip(
                    &merged_path,
                    &merged_items,
                    &pooled_resources,
                    device_id,
                )?;
                let data = std::fs::read(&merged_path).map_err(|e| e.to_string())?;
                let data = encrypt_if_configured(data, settings)?;
                bytes_uploaded += data.len() as u64;
                sync::upload_to_s3(
                    endpoint,
                    &region,
                    &bucket,
                    &s3_object_key(remote_path, &filename),
                    data,
                    &access_key,
                    &secret_key,
                )?;
                for baseline in &remote_baselines {
                    let _ = sync::delete_from_s3(
                        endpoint,
                        &region,
                        &bucket,
                        &s3_object_key(remote_path, &baseline.name),
                        &access_key,
                        &secret_key,
                    );
                }
                println!(
                    "[sync] consolidated {} baselines into one",
                    remote_baselines.len()
                );
            }

            sync::materialize_resources(&merged_resources, paths, Some(&|rel| pool.download(rel)))?;
            sync::rewrite_item_paths_to_local(&mut merged_items, paths);
            let imported = database
                .import_baseline_items(&merged_items)
                .map_err(|e| e.to_string())?;
            println!("[sync] baseline imported: {} items", imported);
            database
                .mark_sync_remote_initialized(remote_scope)
                .map_err(|e| e.to_string())?;
            applied_remote = true;
        }
    } else if !has_prior_sync {
        println!("[sync] no remote baseline found, uploading local baseline to S3");
        let filename = format!("baseline-{device_id}-{timestamp}.zip");
        let baseline_path = temp_dir.join(&filename);
        let manifest = sync::create_baseline_backup(database, paths, &baseline_path, Some(&pool))?;
        let data = std::fs::read(&baseline_path).map_err(|e| e.to_string())?;
        let data = encrypt_if_configured(data, settings)?;
        bytes_uploaded += data.len() as u64;
        sync::upload_to_s3(
            endpoint,
            &region,
            &bucket,
            &s3_object_key(remote_path, &filename),
            data,
            &access_key,
            &secret_key,
        )?;
        println!("[sync] baseline uploaded: {} items", manifest.item_count);
        database
            .mark_sync_remote_initialized(remote_scope)
            .map_err(|e| e.to_string())?;
    }

    let rollover_entries = settings.oplog_rollover_entries;
    let max_image_bytes = settings.max_sync_image_bytes;
    let max_file_bytes = settings.max_sync_file_bytes;
    let local_entries = database
        .get_unsynced_changelog(rollover_entries)
        .map_err(|e| e.to_string())?;
    let local_entries =
        filter_entries_by_resource_size(local_entries, max_image_bytes, max_file_bytes, paths);

    let mut written_filename: String = String::new();
    let uploaded_entries = if !local_entries.is_empty() {
        let (all_entries, all_resources) = sync::collect_entry_resources(local_entries, paths);
        let plaintext =
            crate::sync::wire::serialize_oplog_with_resources(&all_entries, &all_resources)?;
        let filename = immutable_oplog_filename(device_id, &all_entries, &plaintext);
        let new_data = encrypt_if_configured(plaintext, settings)?;
        bytes_uploaded += new_data.len() as u64;
        sync::upload_to_s3(
            endpoint,
            &region,
            &bucket,
            &s3_object_key(remote_path, &filename),
            new_data,
            &access_key,
            &secret_key,
        )?;
        let max_seq = all_entries.iter().map(|e| e.sequence).max().unwrap_or(0);
        let _ = database.mark_changelog_synced(max_seq);
        let _ = database.purge_synced_changelog(1000);
        written_filename = filename;
        all_entries.len() as u64
    } else {
        0
    };

    let mut downloaded_entries: u64 = 0;
    let mut applied_entries: u64 = 0;
    let mut applied_objects = database
        .get_applied_sync_remote_oplogs(remote_scope)
        .map_err(|e| e.to_string())?;

    for entry in &remote_objects {
        if entry.is_directory || !entry.name.starts_with("oplog-") {
            continue;
        }
        if entry.name.starts_with(&format!("oplog-{device_id}-")) {
            continue;
        }
        let revision = remote_oplog_revision(entry);
        if revision.as_ref().is_some_and(|revision| {
            applied_objects
                .get(&entry.name)
                .is_some_and(|applied| applied == revision)
        }) {
            continue;
        }
        match sync::download_from_s3(
            endpoint,
            &region,
            &bucket,
            &s3_object_key(remote_path, &entry.name),
            &access_key,
            &secret_key,
        ) {
            Ok(data) => {
                let data = decrypt_if_configured(data, settings)?;
                bytes_downloaded += data.len() as u64;
                let (mut remote_entries, resources): (
                    Vec<crate::storage::SyncChangeLogEntry>,
                    Vec<crate::sync::wire::OplogResource>,
                ) = match crate::sync::wire::deserialize_oplog_with_resources(&data) {
                    Ok(e) => e,
                    Err(e) => {
                        println!("[sync] failed to parse {}: {}", entry.name, e);
                        continue;
                    }
                };
                if let Err(e) =
                    sync::materialize_resources(&resources, paths, Some(&|rel| pool.download(rel)))
                {
                    println!("[sync] failed to materialize {}: {}", entry.name, e);
                    continue;
                }
                sync::rewrite_to_local(&mut remote_entries, paths);
                downloaded_entries += remote_entries.len() as u64;
                match database.apply_remote_oplog(&remote_entries) {
                    Ok(applied) => {
                        applied_entries += applied;
                        if let Some(revision) = revision.as_deref() {
                            database
                                .mark_sync_remote_oplog_applied(remote_scope, &entry.name, revision)
                                .map_err(|e| e.to_string())?;
                            applied_objects.insert(entry.name.clone(), revision.to_string());
                        }
                        if applied > 0 {
                            applied_remote = true;
                        }
                    }
                    Err(e) => println!("[sync] apply error: {}", e),
                }
            }
            Err(e) => println!("[sync] download error for {}: {}", entry.name, e),
        }
    }

    let max_files = settings.max_remote_oplog_files;
    let deleted_remote_files = cleanup_old_s3_oplogs(
        endpoint,
        &region,
        &bucket,
        &access_key,
        &secret_key,
        prefix,
        &written_filename,
        max_files,
    )?;

    if applied_remote {
        let _ = app.emit(
            "clipboard-history-invalidated",
            crate::commands::clipboard::ClipboardHistoryInvalidated {
                deleted_ids: Vec::new(),
            },
        );
    }

    Ok(SyncUploadResult {
        uploaded_entries,
        downloaded_entries,
        applied_entries,
        deleted_remote_files,
        bytes_uploaded,
        bytes_downloaded,
    })
}

fn s3_object_key(remote_path: &str, name: &str) -> String {
    let remote_path = remote_path.trim_matches('/');
    if remote_path.is_empty() {
        name.to_string()
    } else {
        format!("{remote_path}/{name}")
    }
}

fn immutable_oplog_filename(
    device_id: &str,
    entries: &[crate::storage::SyncChangeLogEntry],
    plaintext: &[u8],
) -> String {
    let first_sequence = entries.first().map_or(0, |entry| entry.sequence);
    let last_sequence = entries.last().map_or(0, |entry| entry.sequence);
    let digest = Sha256::digest(plaintext);
    let content_id = hex::encode(digest);
    format!("oplog-{device_id}-s{first_sequence}-e{last_sequence}-{content_id}")
}

fn is_immutable_oplog_filename(name: &str) -> bool {
    let mut parts = name.rsplitn(4, '-');
    let Some(content_id) = parts.next() else {
        return false;
    };
    let Some(last_sequence) = parts.next().and_then(|part| part.strip_prefix('e')) else {
        return false;
    };
    let Some(first_sequence) = parts.next().and_then(|part| part.strip_prefix('s')) else {
        return false;
    };
    let Some(prefix) = parts.next() else {
        return false;
    };
    prefix.starts_with("oplog-")
        && first_sequence.parse::<i64>().is_ok()
        && last_sequence.parse::<i64>().is_ok()
        && content_id.len() == 64
        && content_id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[allow(clippy::too_many_arguments)]
fn cleanup_old_s3_oplogs(
    endpoint: &str,
    region: &str,
    bucket: &str,
    access_key: &str,
    secret_key: &str,
    prefix: Option<&str>,
    keep_name: &str,
    max_files: usize,
) -> Result<u64, String> {
    let entries = sync::list_s3_objects(endpoint, region, bucket, prefix, access_key, secret_key)?;
    let mut oplog_files: Vec<(String, Option<i64>)> = entries
        .iter()
        .filter(|e| !e.is_directory && e.name.starts_with("oplog-"))
        .map(|e| (e.name.clone(), e.modified_ms))
        .collect();
    oplog_files.sort_by_key(|a| a.1);

    let mut deleted = 0u64;
    if oplog_files.len() > max_files {
        let to_delete = oplog_files.len() - max_files;
        for (name, _) in &oplog_files[..to_delete] {
            if name == keep_name {
                continue;
            }
            // list_s3_objects returns the basename only; re-prepend the
            // remote_path prefix so the delete targets the real object key.
            let key = match prefix {
                Some(p) if !p.is_empty() => format!("{p}/{name}"),
                _ => name.clone(),
            };
            sync::delete_from_s3(endpoint, region, bucket, &key, access_key, secret_key)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Filters oplog entries by resource size threshold.
/// Entries with resource files exceeding the threshold have their paths cleared.
fn filter_entries_by_resource_size(
    entries: Vec<crate::storage::SyncChangeLogEntry>,
    max_image_bytes: u64,
    max_file_bytes: u64,
    paths: &crate::storage::StoragePaths,
) -> Vec<crate::storage::SyncChangeLogEntry> {
    entries
        .into_iter()
        .map(|mut entry| {
            let max_bytes = match entry.kind.as_str() {
                "image" => max_image_bytes,
                "file" => max_file_bytes,
                _ => return entry,
            };

            if max_bytes == 0 {
                entry.resource_path = None;
                entry.preview_path = None;
                return entry;
            }

            if let Some(ref resource_path) = entry.resource_path {
                let full_path = paths.storage.join(resource_path);
                if let Ok(metadata) = std::fs::metadata(&full_path) {
                    if metadata.len() > max_bytes {
                        entry.resource_path = None;
                        entry.preview_path = None;
                    }
                }
            }
            entry
        })
        .collect()
}

fn get_device_id() -> String {
    sync::device_id()
}

/// Common shape shared by the WebDAV and S3 remote-listing entries so the
/// sync/compaction helpers can treat both providers uniformly.
trait RemoteFileLike {
    fn name(&self) -> &str;
    fn is_directory(&self) -> bool;
    fn modified_ms(&self) -> Option<i64>;
}

impl RemoteFileLike for sync::WebDavEntry {
    fn name(&self) -> &str {
        &self.name
    }
    fn is_directory(&self) -> bool {
        self.is_directory
    }
    fn modified_ms(&self) -> Option<i64> {
        self.modified_ms
    }
}

impl RemoteFileLike for sync::S3Entry {
    fn name(&self) -> &str {
        &self.name
    }
    fn is_directory(&self) -> bool {
        self.is_directory
    }
    fn modified_ms(&self) -> Option<i64> {
        self.modified_ms
    }
}

fn remote_oplog_revision<T: RemoteFileLike>(entry: &T) -> Option<String> {
    if is_immutable_oplog_filename(entry.name()) {
        return Some(format!("immutable:{}", entry.name()));
    }
    // Legacy clients may overwrite an existing timestamp-named object. Its
    // provider mtime can be coarse or preserved, so no metadata-only revision
    // is safe enough to skip it; always download legacy names again.
    None
}

/// Records the remote file layout observed during a sync so `get_sync_config`
/// can suggest a manual compaction without an extra network round-trip.
fn record_remote_stats<T: RemoteFileLike>(
    database: &Database,
    remote_scope: &str,
    remote_files: &[T],
) {
    let oplog_count = remote_files
        .iter()
        .filter(|e| !e.is_directory() && e.name().starts_with("oplog-"))
        .count() as u64;
    let baseline_ms = remote_files
        .iter()
        .filter(|e| {
            !e.is_directory() && e.name().starts_with("baseline-") && e.name().ends_with(".zip")
        })
        .filter_map(RemoteFileLike::modified_ms)
        .max();
    let _ = database.set_sync_remote_oplog_count(remote_scope, oplog_count);
    let _ = database.set_sync_remote_baseline_modified_ms(remote_scope, baseline_ms);
}

fn cleanup_old_remote_oplogs(
    endpoint: &str,
    remote_path: &str,
    username: &Option<String>,
    password: &Option<String>,
    keep_name: &str,
    max_files: usize,
) -> Result<u64, String> {
    let entries = sync::list_webdav_files(
        endpoint,
        Some(remote_path),
        username.as_deref(),
        password.as_deref(),
    )?;

    let mut oplog_files: Vec<(String, Option<i64>)> = entries
        .iter()
        .filter(|e| !e.is_directory && e.name.starts_with("oplog-"))
        .map(|e| (e.name.clone(), e.modified_ms))
        .collect();

    oplog_files.sort_by_key(|a| a.1);

    let mut deleted = 0u64;
    if oplog_files.len() > max_files {
        let to_delete = oplog_files.len() - max_files;
        for (name, _) in &oplog_files[..to_delete] {
            if name == keep_name {
                continue;
            }
            let _ = sync::delete_from_webdav(
                endpoint,
                remote_path,
                name,
                username.as_deref(),
                password.as_deref(),
            );
            deleted += 1;
        }
    }

    Ok(deleted)
}

#[tauri::command]
pub fn sync_list_remote_backups(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<Vec<RemoteBackupEntry>, String> {
    let settings = {
        let guard = config.lock().map_err(|_| "lock poisoned".to_owned())?;
        SyncSettings::from_config(&guard)
    };
    let endpoint = settings
        .endpoint
        .clone()
        .ok_or("sync endpoint not configured")?;

    match settings.provider {
        crate::config::SyncProvider::Webdav => {
            let files = sync::list_webdav_files(
                &endpoint,
                settings.remote_path.as_deref(),
                settings.username.as_deref(),
                settings.password.as_deref(),
            )?;
            Ok(files.into_iter().map(RemoteBackupEntry::from).collect())
        }
        crate::config::SyncProvider::S3 => {
            let (region, bucket, access_key, secret_key) = resolve_s3_config(&settings);
            let prefix = settings.remote_path.as_deref();
            let files = sync::list_s3_objects(
                &endpoint,
                &region,
                &bucket,
                prefix,
                &access_key,
                &secret_key,
            )?;
            Ok(files.into_iter().map(RemoteBackupEntry::from).collect())
        }
        _ => Err("unsupported provider".to_string()),
    }
}

#[tauri::command]
pub fn sync_download_backup(
    filename: String,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<PathBuf, String> {
    let settings = {
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        SyncSettings::from_config(&guard)
    };

    let endpoint = settings
        .endpoint
        .clone()
        .ok_or("sync endpoint not configured")?;
    let remote_path = settings.remote_path.clone().unwrap_or_default();

    let data = match settings.provider {
        crate::config::SyncProvider::Webdav => sync::download_from_webdav(
            &endpoint,
            &remote_path,
            &filename,
            settings.username.as_deref(),
            settings.password.as_deref(),
        )?,
        crate::config::SyncProvider::S3 => {
            let (region, bucket, access_key, secret_key) = resolve_s3_config(&settings);
            let key = if remote_path.is_empty() {
                filename.clone()
            } else {
                format!("{remote_path}/{filename}")
            };
            sync::download_from_s3(&endpoint, &region, &bucket, &key, &access_key, &secret_key)?
        }
        _ => return Err("unsupported provider".to_string()),
    };

    let data = decrypt_if_configured(data, &settings)?;

    let temp_dir = std::env::temp_dir().join("clipboard-sync");
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    let target = temp_dir.join(&filename);
    std::fs::write(&target, data).map_err(|e| e.to_string())?;

    Ok(target)
}

#[tauri::command]
pub fn verify_backup_file(path: PathBuf) -> Result<sync::BackupManifest, String> {
    sync::read_manifest_from_backup(&path)
}

/// Compacts the remote sync history: refreshes the baseline snapshot from the
/// local database (which is a superset of the remote after the pre-flight sync)
/// and then removes every older baseline plus the older oplogs that are fully
/// covered by the fresh snapshot. Manual only; no scheduled/automatic path.
#[tauri::command]
pub fn sync_compact_remote(app: tauri::AppHandle) -> Result<SyncUploadResult, String> {
    // 1. Run a full sync first so the local DB reflects every remote change.
    let preflight = run_sync(&app)?;

    // 2. Compact: the run-task lock is re-acquired now that run_sync (which
    // holds it internally) has finished.
    let config = app.state::<Mutex<ConfigStore>>();
    let database = app.state::<Database>();
    let paths = app.state::<StoragePaths>();

    let _run_guard = SYNC_RUN_LOCK
        .try_lock()
        .map_err(|_| "sync already in progress".to_owned())?;

    let settings = {
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        SyncSettings::from_config(&guard)
    };

    let endpoint = settings
        .endpoint
        .clone()
        .ok_or("sync endpoint not configured")?;
    let remote_path = settings.remote_path.clone().unwrap_or_default();
    let remote_scope = settings.remote_scope_id()?;
    let device_id = get_device_id();

    let temp_dir = std::env::temp_dir().join("clipboard-sync");
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();

    let (baseline_bytes, deleted) = match settings.provider {
        crate::config::SyncProvider::Webdav => compact_webdav(
            &settings,
            &database,
            &paths,
            &endpoint,
            &remote_path,
            &remote_scope,
            &device_id,
            &timestamp,
            &temp_dir,
        )?,
        crate::config::SyncProvider::S3 => compact_s3(
            &settings,
            &database,
            &paths,
            &endpoint,
            &remote_path,
            &remote_scope,
            &device_id,
            &timestamp,
            &temp_dir,
        )?,
        _ => return Err("unsupported provider".to_string()),
    };

    Ok(SyncUploadResult {
        uploaded_entries: preflight.uploaded_entries,
        downloaded_entries: preflight.downloaded_entries,
        applied_entries: preflight.applied_entries,
        deleted_remote_files: preflight.deleted_remote_files + deleted,
        bytes_uploaded: preflight.bytes_uploaded + baseline_bytes,
        bytes_downloaded: preflight.bytes_downloaded,
    })
}

#[allow(clippy::too_many_arguments)]
fn compact_webdav(
    settings: &SyncSettings,
    database: &Database,
    paths: &crate::storage::StoragePaths,
    endpoint: &str,
    remote_path: &str,
    remote_scope: &str,
    device_id: &str,
    timestamp: &str,
    temp_dir: &std::path::Path,
) -> Result<(u64, u64), String> {
    let username = settings.username.as_deref();
    let password = settings.password.as_deref();

    let pool = WebDavPool {
        endpoint,
        remote_path,
        username,
        password,
        remote_scope,
        settings,
    };

    let filename = format!("baseline-{device_id}-{timestamp}.zip");
    let baseline_path = temp_dir.join(&filename);
    sync::create_baseline_backup(database, paths, &baseline_path, Some(&pool))?;
    let data = std::fs::read(&baseline_path).map_err(|e| e.to_string())?;
    let data = encrypt_if_configured(data, settings)?;
    sync::upload_to_webdav(endpoint, remote_path, &filename, data, username, password)?;

    // Delete older baselines (the fresh one above now covers their data).
    let mut deleted = 0u64;
    for entry in sync::list_webdav_files(endpoint, Some(remote_path), username, password)? {
        if entry.is_directory {
            continue;
        }
        if !(entry.name.starts_with("baseline-") && entry.name.ends_with(".zip"))
            || entry.name == filename
        {
            continue;
        }
        if sync::delete_from_webdav(endpoint, remote_path, &entry.name, username, password).is_ok()
        {
            deleted += 1;
        }
    }

    // Trim oplogs to the retention ceiling. Files dropped here are fully
    // covered by the freshly written baseline.
    let oplog_deleted = cleanup_old_remote_oplogs(
        endpoint,
        remote_path,
        &settings.username,
        &settings.password,
        "",
        settings.max_remote_oplog_files,
    )?;
    deleted += oplog_deleted;

    let baseline_bytes = std::fs::metadata(&baseline_path)
        .map(|m| m.len())
        .unwrap_or(0) as u64;

    // Re-record the post-compaction layout so `get_sync_config` stops flagging
    // `compaction_suggested` on the next open (no extra round-trip otherwise).
    if let Ok(files) = sync::list_webdav_files(endpoint, Some(remote_path), username, password) {
        record_remote_stats(database, remote_scope, &files);
    }

    Ok((baseline_bytes, deleted))
}

#[allow(clippy::too_many_arguments)]
fn compact_s3(
    settings: &SyncSettings,
    database: &Database,
    paths: &crate::storage::StoragePaths,
    endpoint: &str,
    remote_path: &str,
    remote_scope: &str,
    device_id: &str,
    timestamp: &str,
    temp_dir: &std::path::Path,
) -> Result<(u64, u64), String> {
    let (region, bucket, access_key, secret_key) = resolve_s3_config(settings);
    let prefix = if remote_path.is_empty() {
        None
    } else {
        Some(remote_path)
    };

    let pool = S3Pool {
        endpoint,
        region: &region,
        bucket: &bucket,
        access_key: &access_key,
        secret_key: &secret_key,
        remote_path,
        remote_scope,
        settings,
    };

    let filename = format!("baseline-{device_id}-{timestamp}.zip");
    let baseline_path = temp_dir.join(&filename);
    sync::create_baseline_backup(database, paths, &baseline_path, Some(&pool))?;
    let data = std::fs::read(&baseline_path).map_err(|e| e.to_string())?;
    let data = encrypt_if_configured(data, settings)?;
    sync::upload_to_s3(
        endpoint,
        &region,
        &bucket,
        &s3_object_key(remote_path, &filename),
        data,
        &access_key,
        &secret_key,
    )?;

    let mut deleted = 0u64;
    for entry in
        sync::list_s3_objects(endpoint, &region, &bucket, prefix, &access_key, &secret_key)?
    {
        if entry.is_directory {
            continue;
        }
        if !(entry.name.starts_with("baseline-") && entry.name.ends_with(".zip"))
            || entry.name == filename
        {
            continue;
        }
        if sync::delete_from_s3(
            endpoint,
            &region,
            &bucket,
            &s3_object_key(remote_path, &entry.name),
            &access_key,
            &secret_key,
        )
        .is_ok()
        {
            deleted += 1;
        }
    }

    let oplog_deleted = cleanup_old_s3_oplogs(
        endpoint,
        &region,
        &bucket,
        &access_key,
        &secret_key,
        prefix,
        "",
        settings.max_remote_oplog_files,
    )?;
    deleted += oplog_deleted;

    // Re-record the post-compaction layout so `get_sync_config` stops flagging
    // `compaction_suggested` on the next open (no extra round-trip otherwise).
    if let Ok(objects) =
        sync::list_s3_objects(endpoint, &region, &bucket, prefix, &access_key, &secret_key)
    {
        record_remote_stats(database, remote_scope, &objects);
    }

    let baseline_bytes = std::fs::metadata(&baseline_path)
        .map(|m| m.len())
        .unwrap_or(0) as u64;
    Ok((baseline_bytes, deleted))
}

fn get_sync_password(settings: &SyncSettings) -> Option<String> {
    settings.sync_password.clone()
}

fn encrypt_if_configured(data: Vec<u8>, settings: &SyncSettings) -> Result<Vec<u8>, String> {
    match get_sync_password(settings) {
        Some(pwd) if !pwd.is_empty() => sync::crypto::encrypt(&data, &pwd),
        _ => Ok(data),
    }
}

fn decrypt_if_configured(data: Vec<u8>, settings: &SyncSettings) -> Result<Vec<u8>, String> {
    match get_sync_password(settings) {
        Some(pwd) if !pwd.is_empty() => match sync::crypto::decrypt(&data, &pwd) {
            Ok(decrypted) => Ok(decrypted),
            Err(e) => {
                // Keep the documented fallback (remote files uploaded before a
                // sync password was set, or a changed password, leave bytes
                // unreadable), but surface the failure instead of silently
                // skipping the payload downstream.
                println!("[sync] warning: {e}; falling back to raw payload bytes");
                Ok(data)
            }
        },
        _ => Ok(data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn webdav_settings(endpoint: &str, remote_path: &str) -> SyncSettings {
        SyncSettings {
            provider: crate::config::SyncProvider::Webdav,
            endpoint: Some(endpoint.to_string()),
            remote_path: Some(remote_path.to_string()),
            username: Some("user".to_string()),
            password: None,
            s3_region: "us-east-1".to_string(),
            s3_bucket: None,
            s3_access_key: None,
            s3_secret_key: None,
            sync_password: None,
            max_remote_oplog_files: 10,
            oplog_rollover_entries: 100,
            max_sync_image_bytes: 5_242_880,
            max_sync_file_bytes: 10_485_760,
        }
    }

    fn sample_sync_entry(sequence: i64) -> crate::storage::SyncChangeLogEntry {
        crate::storage::SyncChangeLogEntry {
            sequence,
            item_id: format!("item-{sequence}"),
            operation: "insert".to_string(),
            kind: "text".to_string(),
            title: "title".to_string(),
            content_hash: format!("hash-{sequence}"),
            resource_path: None,
            preview_path: None,
            icon_path: None,
            text_content: Some("text".to_string()),
            html_content: None,
            rtf_content: None,
            metadata_json: None,
            is_favorite: false,
            source_app: None,
            size_bytes: 0,
            last_used_at_ms: None,
            created_at_ms: 100,
            modified_at_ms: 100,
            device_id: "device".to_string(),
        }
    }

    #[test]
    fn remote_scope_is_stable_but_isolates_different_paths() {
        let first = webdav_settings("https://dav.example.test/", "/clipboard/");
        let equivalent = webdav_settings("https://dav.example.test", "clipboard");
        let other = webdav_settings("https://dav.example.test", "other");

        assert_eq!(
            first.remote_scope_id().unwrap(),
            equivalent.remote_scope_id().unwrap()
        );
        assert_ne!(
            first.remote_scope_id().unwrap(),
            other.remote_scope_id().unwrap()
        );
    }

    #[test]
    fn immutable_oplog_names_include_sequence_range_and_content_id() {
        let entries = vec![sample_sync_entry(4), sample_sync_entry(9)];
        let first = immutable_oplog_filename("device-a", &entries, b"payload");
        let same = immutable_oplog_filename("device-a", &entries, b"payload");
        let changed = immutable_oplog_filename("device-a", &entries, b"changed");

        assert_eq!(first, same);
        assert_ne!(first, changed);
        assert!(first.starts_with("oplog-device-a-s4-e9-"));
        assert!(is_immutable_oplog_filename(&first));
        assert!(!is_immutable_oplog_filename(
            "oplog-device-a-20260810_120000"
        ));
    }

    #[test]
    fn legacy_mutable_oplogs_are_never_skipped_by_metadata() {
        let immutable = sync::WebDavEntry {
            name: immutable_oplog_filename("device-a", &[sample_sync_entry(1)], b"payload"),
            is_directory: false,
            size_bytes: None,
            modified_ms: None,
        };
        assert!(remote_oplog_revision(&immutable)
            .unwrap()
            .starts_with("immutable:"));

        let legacy = sync::WebDavEntry {
            name: "oplog-device-a-20260810_120000".to_string(),
            is_directory: false,
            size_bytes: Some(20),
            modified_ms: Some(10),
        };
        assert_eq!(remote_oplog_revision(&legacy), None);
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
    fn sync_config_info_serializes_policy_fields() {
        let value = serde_json::to_value(SyncConfigInfo {
            provider: "off".to_string(),
            endpoint: None,
            remote_path: None,
            username: None,
            has_password: false,
            s3_region: None,
            s3_bucket: None,
            s3_access_key: None,
            has_s3_secret_key: false,
            has_sync_password: false,
            last_sync_ms: None,
            last_sync_status: None,
            unsynced_count: 0,
            auto_sync: true,
            auto_sync_interval_secs: 300,
            max_remote_oplog_files: 10,
            oplog_rollover_entries: 100,
            oplog_rollover_size_bytes: 51_200,
            max_sync_image_bytes: 5_242_880,
            max_sync_file_bytes: 10_485_760,
            compaction_suggested: false,
        })
        .unwrap();

        assert_eq!(value["autoSync"], true);
        assert_eq!(value["autoSyncIntervalSecs"], 300);
        assert_eq!(value["maxRemoteOplogFiles"], 10);
        assert_eq!(value["oplogRolloverEntries"], 100);
        assert_eq!(value["oplogRolloverSizeBytes"], 51_200);
        assert_eq!(value["maxSyncImageBytes"], 5_242_880);
        assert_eq!(value["maxSyncFileBytes"], 10_485_760);
    }

    #[test]
    fn remote_backup_entries_serialize_as_a_direct_array() {
        let value = serde_json::to_value(vec![RemoteBackupEntry {
            name: "baseline.zip".to_string(),
            is_directory: false,
            size_bytes: Some(42),
            modified_ms: Some(100),
        }])
        .unwrap();

        assert!(value.is_array());
        assert_eq!(value[0]["name"], "baseline.zip");
        assert_eq!(value[0]["isDirectory"], false);
    }

    #[test]
    fn s3_object_key_without_remote_path_keeps_basename() {
        assert_eq!(
            s3_object_key("", "oplog-dev-20260808"),
            "oplog-dev-20260808"
        );
    }

    #[test]
    fn s3_object_key_with_remote_path_prepends_prefix() {
        assert_eq!(
            s3_object_key("backups", "oplog-dev-20260808"),
            "backups/oplog-dev-20260808"
        );
    }

    #[test]
    fn s3_object_key_with_trailing_slash_avoids_double_separator() {
        assert_eq!(
            s3_object_key("backups/", "oplog-dev-20260808"),
            "backups/oplog-dev-20260808"
        );
    }

    #[test]
    fn s3_object_key_with_leading_and_trailing_slash_is_normalized() {
        assert_eq!(
            s3_object_key("/backups/", "oplog-dev-20260808"),
            "backups/oplog-dev-20260808"
        );
    }
}
