use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;

use crate::config::{ConfigStore, SyncConfig};
use crate::storage::Database;
use crate::storage::StoragePaths;
use crate::sync;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfigInfo {
    pub provider: String,
    pub endpoint: Option<String>,
    pub remote_path: Option<String>,
    pub username: Option<String>,
    pub has_password: bool,
    pub last_sync_ms: Option<i64>,
    pub last_sync_status: Option<String>,
    pub unsynced_count: u64,
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

#[tauri::command]
pub fn get_sync_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    database: tauri::State<'_, Database>,
) -> Result<SyncConfigInfo, String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    let sync = guard.sync_config();
    let unsynced = database.count_unsynced_changelog().unwrap_or(0);
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
        last_sync_ms: sync.last_sync_ms,
        last_sync_status: sync.last_sync_status,
        unsynced_count: unsynced,
    })
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
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<(), String> {
    let mut guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;

    let prev = guard.sync_config();

    let provider = match provider.as_str() {
        "webdav" => crate::config::SyncProvider::Webdav,
        _ => crate::config::SyncProvider::Off,
    };

    let sync = SyncConfig {
        provider,
        endpoint,
        remote_path,
        username,
        password: password.or(prev.password),
        last_sync_ms: prev.last_sync_ms,
        last_sync_status: prev.last_sync_status,
        auto_sync,
        auto_sync_interval_secs: auto_sync_interval_secs.clamp(10, 86400),
        max_remote_oplog_files: max_remote_oplog_files.clamp(3, 100),
        oplog_rollover_entries: oplog_rollover_entries.clamp(10, 10000),
        oplog_rollover_size_bytes: oplog_rollover_size_bytes.clamp(1024, 1048576),
        max_sync_image_bytes,
        max_sync_file_bytes,
        ..Default::default()
    };

    guard.set_sync_config(sync).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn test_sync_connection(
    provider: String,
    endpoint: String,
    remote_path: Option<String>,
    username: Option<String>,
    password: Option<String>,
) -> Result<sync::WebDavTestResult, String> {
    match provider.as_str() {
        "webdav" => Ok(sync::test_webdav_connection(
            &endpoint,
            remote_path.as_deref(),
            username.as_deref(),
            password.as_deref(),
        )),
        _ => Err("unsupported provider".to_string()),
    }
}

#[tauri::command]
pub fn sync_upload_backup(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<SyncUploadResult, String> {
    let mut guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    let sync = guard.sync_config();

    let endpoint = sync.endpoint.ok_or("sync endpoint not configured")?;
    let remote_path = sync.remote_path.unwrap_or_default();
    let device_id = get_device_id();

    let temp_dir = std::env::temp_dir().join("clipboard-sync");
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let mut bytes_uploaded: u64 = 0;
    let mut bytes_downloaded: u64 = 0;

    // === Detect sync state ===
    let has_prior_sync = sync.last_sync_ms.is_some();
    let remote_files = sync::list_webdav_files(
        &endpoint,
        Some(&remote_path),
        sync.username.as_deref(),
        sync.password.as_deref(),
    )?;

    let remote_baseline = remote_files
        .iter()
        .find(|e| !e.is_directory && e.name.starts_with("baseline-") && e.name.ends_with(".zip"));

    // === Phase 0: Baseline handling ===
    if let Some(baseline) = remote_baseline {
        if !has_prior_sync {
            println!("[sync] downloading remote baseline: {}", baseline.name);
            let data = sync::download_from_webdav(
                &endpoint,
                &remote_path,
                &baseline.name,
                sync.username.as_deref(),
                sync.password.as_deref(),
            )?;
            bytes_downloaded += data.len() as u64;

            let baseline_path = temp_dir.join(&baseline.name);
            std::fs::write(&baseline_path, &data).map_err(|e| e.to_string())?;

            let items = sync::read_baseline_items(&baseline_path)?;
            let imported = database
                .import_baseline_items(&items)
                .map_err(|e| e.to_string())?;
            println!("[sync] baseline imported: {} items", imported);
        }
    } else if !has_prior_sync {
        println!("[sync] no remote baseline found, uploading local baseline");
        let filename = format!("baseline-{device_id}-{timestamp}.zip");
        let baseline_path = temp_dir.join(&filename);
        let manifest = sync::create_baseline_backup(&database, &baseline_path)?;
        let data = std::fs::read(&baseline_path).map_err(|e| e.to_string())?;
        bytes_uploaded += data.len() as u64;

        sync::upload_to_webdav(
            &endpoint,
            &remote_path,
            &filename,
            data,
            sync.username.as_deref(),
            sync.password.as_deref(),
        )?;
        println!("[sync] baseline uploaded: {} items", manifest.item_count);
    }

    // === Phase 1: Upload local oplog entries ===
    let rollover_entries = guard.oplog_rollover_entries() as usize;
    let rollover_bytes = guard.oplog_rollover_size_bytes() as usize;
    let max_image_bytes = guard.max_sync_image_bytes();
    let max_file_bytes = guard.max_sync_file_bytes();
    let local_entries = database
        .get_unsynced_changelog(rollover_entries)
        .map_err(|e| e.to_string())?;

    let local_entries =
        filter_entries_by_resource_size(local_entries, max_image_bytes, max_file_bytes, &paths);

    let uploaded_entries = if !local_entries.is_empty() {
        let mut all_entries = local_entries;
        let mut filename = format!("oplog-{device_id}-{timestamp}.json");

        if let Some(existing) = remote_files.iter().find(|e| {
            !e.is_directory
                && e.name.starts_with(&format!("oplog-{device_id}-"))
                && e.name.ends_with(".json")
                && !e.name.contains(timestamp.as_str())
        }) {
            if let Some(size) = existing.size_bytes {
                if size < rollover_bytes as u64 {
                    if let Ok(data) = sync::download_from_webdav(
                        &endpoint,
                        &remote_path,
                        &existing.name,
                        sync.username.as_deref(),
                        sync.password.as_deref(),
                    ) {
                        if let Ok(mut old_entries) =
                            serde_json::from_str::<Vec<crate::storage::SyncChangeLogEntry>>(
                                &String::from_utf8_lossy(&data),
                            )
                        {
                            if old_entries.len() < rollover_entries {
                                filename = existing.name.clone();
                                old_entries.append(&mut all_entries);
                                all_entries = old_entries;
                            }
                        }
                    }
                }
            }
        }

        let new_json = serde_json::to_vec_pretty(&all_entries).map_err(|e| e.to_string())?;
        bytes_uploaded += new_json.len() as u64;

        sync::upload_to_webdav(
            &endpoint,
            &remote_path,
            &filename,
            new_json,
            sync.username.as_deref(),
            sync.password.as_deref(),
        )?;

        let max_seq = all_entries.iter().map(|e| e.sequence).max().unwrap_or(0);
        let _ = database.mark_changelog_synced(max_seq);
        let _ = database.purge_synced_changelog(1000);

        all_entries.len() as u64
    } else {
        0
    };

    // === Phase 2: Download and merge remote oplogs ===
    let mut downloaded_entries: u64 = 0;
    let mut applied_entries: u64 = 0;

    for entry in &remote_files {
        if entry.is_directory || !entry.name.starts_with("oplog-") || !entry.name.ends_with(".json")
        {
            continue;
        }
        if entry.name.contains(&device_id) {
            continue;
        }

        match sync::download_from_webdav(
            &endpoint,
            &remote_path,
            &entry.name,
            sync.username.as_deref(),
            sync.password.as_deref(),
        ) {
            Ok(data) => {
                bytes_downloaded += data.len() as u64;
                let remote_entries: Vec<crate::storage::SyncChangeLogEntry> =
                    match bincode::decode_from_slice(&data, bincode::config::standard()) {
                        Ok((entries, _)) => entries,
                        Err(_) => match serde_json::from_slice(&data) {
                            Ok(entries) => entries,
                            Err(e) => {
                                println!("[sync] failed to parse {}: {}", entry.name, e);
                                continue;
                            }
                        },
                    };
                let count = remote_entries.len() as u64;
                downloaded_entries += count;

                match database.apply_remote_oplog(&remote_entries) {
                    Ok(applied) => applied_entries += applied,
                    Err(e) => println!("[sync] apply error: {}", e),
                }
            }
            Err(e) => println!("[sync] download error for {}: {}", entry.name, e),
        }
    }

    // === Phase 3: Cleanup old remote oplog files ===
    let max_files = max_remote_oplog_files(&guard);
    let deleted_remote_files = cleanup_old_remote_oplogs(
        &endpoint,
        &remote_path,
        &sync.username,
        &sync.password,
        &device_id,
        max_files,
    )?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    guard
        .update_sync_status("success", now_ms)
        .map_err(|e| e.to_string())?;

    Ok(SyncUploadResult {
        uploaded_entries,
        downloaded_entries,
        applied_entries,
        deleted_remote_files,
        bytes_uploaded,
        bytes_downloaded,
    })
}

fn max_remote_oplog_files(config: &ConfigStore) -> usize {
    config.max_remote_oplog_files() as usize
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
    if let Ok(hostname) = std::env::var("COMPUTERNAME") {
        return hostname.to_lowercase();
    }
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        return hostname.to_lowercase();
    }
    "unknown".to_string()
}

fn cleanup_old_remote_oplogs(
    endpoint: &str,
    remote_path: &str,
    username: &Option<String>,
    password: &Option<String>,
    device_id: &str,
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
        .filter(|e| !e.is_directory && e.name.starts_with("oplog-") && e.name.ends_with(".json"))
        .map(|e| (e.name.clone(), e.modified_ms))
        .collect();

    oplog_files.sort_by_key(|a| a.1);

    let mut deleted = 0u64;
    if oplog_files.len() > max_files {
        let to_delete = oplog_files.len() - max_files;
        for (name, _) in &oplog_files[..to_delete] {
            if !name.contains(device_id) {
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
    }

    Ok(deleted)
}

#[tauri::command]
pub fn sync_list_remote_backups(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<Vec<sync::WebDavEntry>, String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    let sync = guard.sync_config();

    let endpoint = sync.endpoint.ok_or("sync endpoint not configured")?;
    let remote_path = sync.remote_path.clone();

    sync::list_webdav_files(
        &endpoint,
        remote_path.as_deref(),
        sync.username.as_deref(),
        sync.password.as_deref(),
    )
}

#[tauri::command]
pub fn sync_download_backup(
    filename: String,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<PathBuf, String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    let sync = guard.sync_config();

    let endpoint = sync.endpoint.ok_or("sync endpoint not configured")?;
    let remote_path = sync.remote_path.unwrap_or_default();

    let data = sync::download_from_webdav(
        &endpoint,
        &remote_path,
        &filename,
        sync.username.as_deref(),
        sync.password.as_deref(),
    )?;

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
