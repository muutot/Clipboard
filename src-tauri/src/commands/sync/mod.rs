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
        s3_region: sync.s3_region,
        s3_bucket: sync.s3_bucket,
        s3_access_key: sync.s3_access_key,
        has_s3_secret_key: sync.s3_secret_key.is_some(),
        has_sync_password: sync.sync_password.is_some(),
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
        s3_region,
        s3_bucket,
        s3_access_key,
        s3_secret_key,
        sync_password,
        ..Default::default()
    };

    guard.set_sync_config(sync).map_err(|e| e.to_string())
}

fn resolve_s3_config(guard: &ConfigStore) -> (String, String, String, String) {
    let region = guard.s3_region();
    let bucket = guard.s3_bucket().unwrap_or_default();
    let access_key = guard.s3_access_key().unwrap_or_default();
    let secret_key = guard.s3_secret_key().unwrap_or_default();
    (region, bucket, access_key, secret_key)
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

    let provider = sync.provider;
    let result = match provider {
        crate::config::SyncProvider::Webdav => sync_upload_webdav(
            &guard,
            &database,
            &paths,
            &endpoint,
            &remote_path,
            &device_id,
            &timestamp,
            &temp_dir,
        ),
        crate::config::SyncProvider::S3 => sync_upload_s3(
            &guard,
            &database,
            &paths,
            &endpoint,
            &remote_path,
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
        let _ = guard.update_sync_status("success", now_ms);
    }

    result
}

#[allow(clippy::too_many_arguments)]
fn sync_upload_webdav(
    guard: &ConfigStore,
    database: &Database,
    paths: &crate::storage::StoragePaths,
    endpoint: &str,
    remote_path: &str,
    device_id: &str,
    timestamp: &str,
    temp_dir: &std::path::Path,
) -> Result<SyncUploadResult, String> {
    let mut bytes_uploaded: u64 = 0;
    let mut bytes_downloaded: u64 = 0;

    let has_prior_sync = guard.sync_config().last_sync_ms.is_some();
    let remote_files = sync::list_webdav_files(
        endpoint,
        Some(remote_path),
        guard.sync_config().username.as_deref(),
        guard.sync_config().password.as_deref(),
    )?;

    let remote_baseline = remote_files
        .iter()
        .find(|e| !e.is_directory && e.name.starts_with("baseline-") && e.name.ends_with(".zip"));

    if let Some(baseline) = remote_baseline {
        if !has_prior_sync {
            println!("[sync] downloading remote baseline: {}", baseline.name);
            let data = sync::download_from_webdav(
                endpoint,
                remote_path,
                &baseline.name,
                guard.sync_config().username.as_deref(),
                guard.sync_config().password.as_deref(),
            )?;
            let data = decrypt_if_configured(data, guard)?;
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
        let manifest = sync::create_baseline_backup(database, &baseline_path)?;
        let data = std::fs::read(&baseline_path).map_err(|e| e.to_string())?;
        let data = encrypt_if_configured(data, guard)?;
        bytes_uploaded += data.len() as u64;
        sync::upload_to_webdav(
            endpoint,
            remote_path,
            &filename,
            data,
            guard.sync_config().username.as_deref(),
            guard.sync_config().password.as_deref(),
        )?;
        println!("[sync] baseline uploaded: {} items", manifest.item_count);
    }

    let rollover_entries = guard.oplog_rollover_entries() as usize;
    let rollover_bytes = guard.oplog_rollover_size_bytes() as usize;
    let max_image_bytes = guard.max_sync_image_bytes();
    let max_file_bytes = guard.max_sync_file_bytes();
    let local_entries = database
        .get_unsynced_changelog(rollover_entries)
        .map_err(|e| e.to_string())?;
    let local_entries =
        filter_entries_by_resource_size(local_entries, max_image_bytes, max_file_bytes, paths);

    let uploaded_entries = if !local_entries.is_empty() {
        let (mut all_entries, mut all_resources) =
            sync::collect_entry_resources(local_entries, paths);
        let mut filename = format!("oplog-{device_id}-{timestamp}.json");

        if let Some(existing) = remote_files.iter().find(|e| {
            !e.is_directory
                && e.name.starts_with(&format!("oplog-{device_id}-"))
                && e.name.ends_with(".json")
                && !e.name.contains(timestamp)
        }) {
            if let Some(size) = existing.size_bytes {
                if size < rollover_bytes as u64 {
                    if let Ok(data) = sync::download_from_webdav(
                        endpoint,
                        remote_path,
                        &existing.name,
                        guard.sync_config().username.as_deref(),
                        guard.sync_config().password.as_deref(),
                    ) {
                        let data = decrypt_if_configured(data, guard)?;
                        if let Ok((mut old_entries, old_resources)) =
                            crate::sync::proto::deserialize_oplog_with_resources(&data).or_else(
                                |_| {
                                    serde_json::from_str::<Vec<crate::storage::SyncChangeLogEntry>>(
                                        &String::from_utf8_lossy(&data),
                                    )
                                    .map(|entries| (entries, Vec::new()))
                                },
                            )
                        {
                            if old_entries.len() < rollover_entries {
                                filename = existing.name.clone();
                                old_entries.append(&mut all_entries);
                                all_entries = old_entries;
                                all_resources.extend(old_resources);
                            }
                        }
                    }
                }
            }
        }

        let new_data =
            crate::sync::proto::serialize_oplog_with_resources(&all_entries, &all_resources)?;
        let new_data = encrypt_if_configured(new_data, guard)?;
        bytes_uploaded += new_data.len() as u64;
        sync::upload_to_webdav(
            endpoint,
            remote_path,
            &filename,
            new_data,
            guard.sync_config().username.as_deref(),
            guard.sync_config().password.as_deref(),
        )?;
        let max_seq = all_entries.iter().map(|e| e.sequence).max().unwrap_or(0);
        let _ = database.mark_changelog_synced(max_seq);
        let _ = database.purge_synced_changelog(1000);
        all_entries.len() as u64
    } else {
        0
    };

    let mut downloaded_entries: u64 = 0;
    let mut applied_entries: u64 = 0;

    for entry in &remote_files {
        if entry.is_directory || !entry.name.starts_with("oplog-") || !entry.name.ends_with(".json")
        {
            continue;
        }
        if entry.name.contains(device_id) {
            continue;
        }
        match sync::download_from_webdav(
            endpoint,
            remote_path,
            &entry.name,
            guard.sync_config().username.as_deref(),
            guard.sync_config().password.as_deref(),
        ) {
            Ok(data) => {
                let data = decrypt_if_configured(data, guard)?;
                bytes_downloaded += data.len() as u64;
                let (mut remote_entries, resources): (
                    Vec<crate::storage::SyncChangeLogEntry>,
                    Vec<crate::sync::proto::OplogResource>,
                ) = match crate::sync::proto::deserialize_oplog_with_resources(&data).or_else(
                    |_| {
                        serde_json::from_str::<Vec<crate::storage::SyncChangeLogEntry>>(
                            &String::from_utf8_lossy(&data),
                        )
                        .map(|entries| (entries, Vec::new()))
                    },
                ) {
                    Ok(e) => e,
                    Err(e) => {
                        println!("[sync] failed to parse {}: {}", entry.name, e);
                        continue;
                    }
                };
                if let Err(e) = sync::materialize_resources(&resources, paths) {
                    println!("[sync] failed to materialize {}: {}", entry.name, e);
                    continue;
                }
                sync::rewrite_to_local(&mut remote_entries, paths);
                downloaded_entries += remote_entries.len() as u64;
                match database.apply_remote_oplog(&remote_entries) {
                    Ok(applied) => applied_entries += applied,
                    Err(e) => println!("[sync] apply error: {}", e),
                }
            }
            Err(e) => println!("[sync] download error for {}: {}", entry.name, e),
        }
    }

    let max_files = max_remote_oplog_files(guard);
    let deleted_remote_files = cleanup_old_remote_oplogs(
        endpoint,
        remote_path,
        &guard.sync_config().username,
        &guard.sync_config().password,
        device_id,
        max_files,
    )?;

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
    guard: &ConfigStore,
    database: &Database,
    paths: &crate::storage::StoragePaths,
    endpoint: &str,
    remote_path: &str,
    device_id: &str,
    timestamp: &str,
    temp_dir: &std::path::Path,
) -> Result<SyncUploadResult, String> {
    let (region, bucket, access_key, secret_key) = resolve_s3_config(guard);
    let mut bytes_uploaded: u64 = 0;
    let mut bytes_downloaded: u64 = 0;

    let prefix: Option<&str> = if remote_path.is_empty() {
        None
    } else {
        Some(remote_path)
    };
    let has_prior_sync = guard.sync_config().last_sync_ms.is_some();
    let remote_objects =
        sync::list_s3_objects(endpoint, &region, &bucket, prefix, &access_key, &secret_key)?;

    let s3_key = |name: &str| {
        if remote_path.is_empty() {
            name.to_string()
        } else {
            format!("{remote_path}/{name}")
        }
    };

    let remote_baseline = remote_objects
        .iter()
        .find(|e| !e.is_directory && e.name.starts_with("baseline-") && e.name.ends_with(".zip"));

    if let Some(baseline) = remote_baseline {
        if !has_prior_sync {
            println!("[sync] downloading S3 baseline: {}", baseline.name);
            let data = sync::download_from_s3(
                endpoint,
                &region,
                &bucket,
                &s3_key(&baseline.name),
                &access_key,
                &secret_key,
            )?;
            let data = decrypt_if_configured(data, guard)?;
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
        println!("[sync] no remote baseline found, uploading local baseline to S3");
        let filename = format!("baseline-{device_id}-{timestamp}.zip");
        let baseline_path = temp_dir.join(&filename);
        let manifest = sync::create_baseline_backup(database, &baseline_path)?;
        let data = std::fs::read(&baseline_path).map_err(|e| e.to_string())?;
        let data = encrypt_if_configured(data, guard)?;
        bytes_uploaded += data.len() as u64;
        sync::upload_to_s3(
            endpoint,
            &region,
            &bucket,
            &s3_key(&filename),
            data,
            &access_key,
            &secret_key,
        )?;
        println!("[sync] baseline uploaded: {} items", manifest.item_count);
    }

    let rollover_entries = guard.oplog_rollover_entries() as usize;
    let rollover_bytes = guard.oplog_rollover_size_bytes() as usize;
    let max_image_bytes = guard.max_sync_image_bytes();
    let max_file_bytes = guard.max_sync_file_bytes();
    let local_entries = database
        .get_unsynced_changelog(rollover_entries)
        .map_err(|e| e.to_string())?;
    let local_entries =
        filter_entries_by_resource_size(local_entries, max_image_bytes, max_file_bytes, paths);

    let uploaded_entries = if !local_entries.is_empty() {
        let (mut all_entries, mut all_resources) =
            sync::collect_entry_resources(local_entries, paths);
        let mut filename = format!("oplog-{device_id}-{timestamp}.json");

        if let Some(existing) = remote_objects.iter().find(|e| {
            !e.is_directory
                && e.name.starts_with(&format!("oplog-{device_id}-"))
                && e.name.ends_with(".json")
                && !e.name.contains(timestamp)
        }) {
            if let Some(size) = existing.size_bytes {
                if size < rollover_bytes as u64 {
                    if let Ok(data) = sync::download_from_s3(
                        endpoint,
                        &region,
                        &bucket,
                        &s3_key(&existing.name),
                        &access_key,
                        &secret_key,
                    ) {
                        let data = decrypt_if_configured(data, guard)?;
                        if let Ok((mut old_entries, old_resources)) =
                            crate::sync::proto::deserialize_oplog_with_resources(&data).or_else(
                                |_| {
                                    serde_json::from_str::<Vec<crate::storage::SyncChangeLogEntry>>(
                                        &String::from_utf8_lossy(&data),
                                    )
                                    .map(|entries| (entries, Vec::new()))
                                },
                            )
                        {
                            if old_entries.len() < rollover_entries {
                                filename = existing.name.clone();
                                old_entries.append(&mut all_entries);
                                all_entries = old_entries;
                                all_resources.extend(old_resources);
                            }
                        }
                    }
                }
            }
        }

        let new_data =
            crate::sync::proto::serialize_oplog_with_resources(&all_entries, &all_resources)?;
        let new_data = encrypt_if_configured(new_data, guard)?;
        bytes_uploaded += new_data.len() as u64;
        sync::upload_to_s3(
            endpoint,
            &region,
            &bucket,
            &s3_key(&filename),
            new_data,
            &access_key,
            &secret_key,
        )?;
        let max_seq = all_entries.iter().map(|e| e.sequence).max().unwrap_or(0);
        let _ = database.mark_changelog_synced(max_seq);
        let _ = database.purge_synced_changelog(1000);
        all_entries.len() as u64
    } else {
        0
    };

    let mut downloaded_entries: u64 = 0;
    let mut applied_entries: u64 = 0;

    for entry in &remote_objects {
        if entry.is_directory || !entry.name.starts_with("oplog-") || !entry.name.ends_with(".json")
        {
            continue;
        }
        if entry.name.contains(device_id) {
            continue;
        }
        match sync::download_from_s3(
            endpoint,
            &region,
            &bucket,
            &s3_key(&entry.name),
            &access_key,
            &secret_key,
        ) {
            Ok(data) => {
                let data = decrypt_if_configured(data, guard)?;
                bytes_downloaded += data.len() as u64;
                let (mut remote_entries, resources): (
                    Vec<crate::storage::SyncChangeLogEntry>,
                    Vec<crate::sync::proto::OplogResource>,
                ) = match crate::sync::proto::deserialize_oplog_with_resources(&data).or_else(
                    |_| {
                        serde_json::from_str::<Vec<crate::storage::SyncChangeLogEntry>>(
                            &String::from_utf8_lossy(&data),
                        )
                        .map(|entries| (entries, Vec::new()))
                    },
                ) {
                    Ok(e) => e,
                    Err(e) => {
                        println!("[sync] failed to parse {}: {}", entry.name, e);
                        continue;
                    }
                };
                if let Err(e) = sync::materialize_resources(&resources, paths) {
                    println!("[sync] failed to materialize {}: {}", entry.name, e);
                    continue;
                }
                sync::rewrite_to_local(&mut remote_entries, paths);
                downloaded_entries += remote_entries.len() as u64;
                match database.apply_remote_oplog(&remote_entries) {
                    Ok(applied) => applied_entries += applied,
                    Err(e) => println!("[sync] apply error: {}", e),
                }
            }
            Err(e) => println!("[sync] download error for {}: {}", entry.name, e),
        }
    }

    let max_files = max_remote_oplog_files(guard);
    let deleted_remote_files = cleanup_old_s3_oplogs(
        endpoint,
        &region,
        &bucket,
        &access_key,
        &secret_key,
        prefix,
        device_id,
        max_files,
    )?;

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
fn cleanup_old_s3_oplogs(
    endpoint: &str,
    region: &str,
    bucket: &str,
    access_key: &str,
    secret_key: &str,
    prefix: Option<&str>,
    device_id: &str,
    max_files: usize,
) -> Result<u64, String> {
    let entries = sync::list_s3_objects(endpoint, region, bucket, prefix, access_key, secret_key)?;
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
                let _ =
                    sync::delete_from_s3(endpoint, region, bucket, name, access_key, secret_key);
                deleted += 1;
            }
        }
    }
    Ok(deleted)
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
) -> Result<String, String> {
    let guard = config.lock().map_err(|_| "lock poisoned".to_owned())?;
    let sync = guard.sync_config();
    let endpoint = sync.endpoint.ok_or("sync endpoint not configured")?;

    match sync.provider {
        crate::config::SyncProvider::Webdav => {
            let files = sync::list_webdav_files(
                &endpoint,
                sync.remote_path.as_deref(),
                sync.username.as_deref(),
                sync.password.as_deref(),
            )?;
            serde_json::to_string(&files).map_err(|e| e.to_string())
        }
        crate::config::SyncProvider::S3 => {
            let (region, bucket, access_key, secret_key) = resolve_s3_config(&guard);
            let prefix = sync.remote_path.as_deref();
            let files = sync::list_s3_objects(
                &endpoint,
                &region,
                &bucket,
                prefix,
                &access_key,
                &secret_key,
            )?;
            serde_json::to_string(&files).map_err(|e| e.to_string())
        }
        _ => Err("unsupported provider".to_string()),
    }
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

    let data = match sync.provider {
        crate::config::SyncProvider::Webdav => sync::download_from_webdav(
            &endpoint,
            &remote_path,
            &filename,
            sync.username.as_deref(),
            sync.password.as_deref(),
        )?,
        crate::config::SyncProvider::S3 => {
            let (region, bucket, access_key, secret_key) = resolve_s3_config(&guard);
            let key = if remote_path.is_empty() {
                filename.clone()
            } else {
                format!("{remote_path}/{filename}")
            };
            sync::download_from_s3(&endpoint, &region, &bucket, &key, &access_key, &secret_key)?
        }
        _ => return Err("unsupported provider".to_string()),
    };

    let data = decrypt_if_configured(data, &guard)?;

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

fn get_sync_password(guard: &ConfigStore) -> Option<String> {
    guard.sync_config().sync_password.clone()
}

fn encrypt_if_configured(data: Vec<u8>, guard: &ConfigStore) -> Result<Vec<u8>, String> {
    match get_sync_password(guard) {
        Some(pwd) if !pwd.is_empty() => sync::crypto::encrypt(&data, &pwd),
        _ => Ok(data),
    }
}

fn decrypt_if_configured(data: Vec<u8>, guard: &ConfigStore) -> Result<Vec<u8>, String> {
    match get_sync_password(guard) {
        Some(pwd) if !pwd.is_empty() => sync::crypto::decrypt(&data, &pwd).or(Ok(data)),
        _ => Ok(data),
    }
}
