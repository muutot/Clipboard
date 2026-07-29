use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::cli;
use crate::cli::{CliArgs, CliCommand, LocalApiServer};
use crate::config::ConfigStore;
use crate::content;
use crate::content::{ContentMarkers, QuickAction, TextTransform, TransformOperation};
use crate::performance::{PerformanceSnapshot, PerformanceTracker};
use crate::search::SearchIndex;
use crate::storage::{ClipboardRepository, Database, RepairResult, StorageFileReferences, StoragePaths, RESOURCE_ROOT_MARKER};
use crate::storage::refresh_database_backup;

#[tauri::command]
pub fn list_icon_files(paths: tauri::State<'_, StoragePaths>) -> Result<Vec<IconFileInfo>, String> {
    let icons_dir = paths.storage.join("icons");
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&icons_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "png") {
                if let Ok(meta) = entry.metadata() {
                    files.push(IconFileInfo {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        size_bytes: meta.len(),
                    });
                }
            }
        }
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

#[tauri::command]
pub fn delete_icon_files(
    paths: tauri::State<'_, StoragePaths>,
    names: Vec<String>,
) -> Result<u64, String> {
    let icons_dir = paths.storage.join("icons");
    let mut deleted = 0u64;
    for name in &names {
        let path = icons_dir.join(name);
        if path.extension().is_some_and(|e| e == "png") && path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[tauri::command]
pub fn copy_file_to(src: String, dst: String) -> Result<(), String> {
    std::fs::copy(&src, &dst)
        .map(|_| ())
        .map_err(|e| format!("copy failed: {e}"))
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| format!("failed to open URL: {e}"))
}

#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("file not found".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("explorer: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        open::that(p.parent().unwrap_or(p)).map_err(|e| format!("open: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub fn detect_content_markers(text: String) -> ContentMarkers {
    content::detect_markers(&text)
}

#[tauri::command]
pub fn detect_content_actions(text: String) -> Vec<QuickAction> {
    let markers = content::detect_markers(&text);
    content::detect_actions(&markers)
}

#[tauri::command]
pub fn transform_text(input: String, operation: String) -> Result<TextTransform, String> {
    let op = match operation.as_str() {
        "stripWhitespace" => TransformOperation::StripWhitespace,
        "stripNewlines" => TransformOperation::StripNewlines,
        "toUpperCase" => TransformOperation::ToUpperCase,
        "toLowerCase" => TransformOperation::ToLowerCase,
        "jsonFormat" => TransformOperation::JsonFormat,
        "base64Encode" => TransformOperation::Base64Encode,
        "base64Decode" => TransformOperation::Base64Decode,
        "urlEncode" => TransformOperation::UrlEncode,
        "urlDecode" => TransformOperation::UrlDecode,
        "md5" => TransformOperation::Md5,
        "sha256" => TransformOperation::Sha256,
        "sha512" => TransformOperation::Sha512,
        _ => return Err(format!("unknown transform operation: {}", operation)),
    };

    let result = op.apply(&input);

    Ok(TextTransform {
        input,
        operation: op,
        result,
    })
}

#[tauri::command]
pub fn enforce_history_cleanup(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<u64, String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    enforce_history_cleanup_for(&database, &guard, &paths, Duration::ZERO)
}

pub fn cleanup_orphan_storage_files_with_grace(
    database: &Database,
    paths: &StoragePaths,
    orphan_file_grace: Duration,
) -> Result<StorageCleanupResult, String> {
    let references = database
        .list_storage_file_references()
        .map_err(|error| error.to_string())?;
    let icons = paths.storage.join("icons");
    let referenced_paths = resolve_storage_file_references(paths, &icons, references);

    let mut removed_files = 0u64;
    let mut freed_bytes = 0u64;

    let scan_dirs: &[(&Path, bool)] = &[
        (&paths.images, paths.image_cleanup_enabled),
        (&paths.previews, paths.image_cleanup_enabled),
        (&paths.files, paths.file_cleanup_enabled),
        (&icons, true),
    ];

    for (dir, cleanup_enabled) in scan_dirs {
        if !cleanup_enabled {
            eprintln!(
                "[cleanup] skipping unowned resource directory {}",
                dir.display()
            );
            continue;
        }
        if !dir.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                continue;
            }
            if entry_path
                .file_name()
                .is_some_and(|name| name == RESOURCE_ROOT_MARKER)
            {
                continue;
            }
            if referenced_paths.contains(&normalized_cleanup_path(&entry_path)) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !orphan_file_grace.is_zero() {
                let Ok(modified_at) = metadata.modified() else {
                    continue;
                };
                if modified_at.elapsed().unwrap_or_default() < orphan_file_grace {
                    continue;
                }
            }
            freed_bytes += metadata.len();
            if let Err(e) = std::fs::remove_file(&entry_path) {
                eprintln!(
                    "[cleanup] failed to remove orphan file {}: {e}",
                    entry_path.display()
                );
            } else {
                removed_files += 1;
            }
        }
    }

    Ok(StorageCleanupResult {
        removed_files,
        freed_bytes,
    })
}

pub fn cleanup_orphan_storage_files(
    database: &Database,
    paths: &StoragePaths,
) -> Result<StorageCleanupResult, String> {
    cleanup_orphan_storage_files_with_grace(database, paths, Duration::ZERO)
}

pub fn stop_signal_requested(receiver: &mpsc::Receiver<()>) -> bool {
    matches!(
        receiver.try_recv(),
        Ok(()) | Err(mpsc::TryRecvError::Disconnected)
    )
}

pub fn wait_for_stop(
    receiver: &mpsc::Receiver<()>,
    stop_flag: &AtomicBool,
    duration: Duration,
) -> bool {
    if stop_flag.load(Ordering::SeqCst) {
        return true;
    }
    match receiver.recv_timeout(duration) {
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => true,
        Err(mpsc::RecvTimeoutError::Timeout) => stop_flag.load(Ordering::SeqCst),
    }
}

pub fn enforce_history_cleanup_for(
    database: &Database,
    config: &ConfigStore,
    paths: &StoragePaths,
    orphan_file_grace: Duration,
) -> Result<u64, String> {
    let retention_days = config.retention_days();
    let max_items = config.max_items();
    let recycle_bin_days = config.recycle_bin_days();
    let mut total_deleted = 0u64;
    total_deleted += database
        .delete_older_than(retention_days)
        .map_err(|error| error.to_string())?;
    total_deleted += database
        .enforce_capacity_limit(max_items as u64)
        .map_err(|error| error.to_string())?;
    total_deleted += database
        .permanently_delete_expired(recycle_bin_days)
        .map_err(|error| error.to_string())?;
    total_deleted += database
        .cleanup_orphan_search_index()
        .map_err(|error| error.to_string())?;

    let _ = cleanup_orphan_storage_files_with_grace(database, paths, orphan_file_grace);

    Ok(total_deleted)
}

#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

#[tauri::command]
pub fn run_cli_command(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    command: String,
    query: Option<String>,
    limit: Option<usize>,
    format: Option<String>,
    output_path: Option<String>,
) -> Result<String, String> {
    let command = match command.as_str() {
        "list" => CliCommand::List,
        "search" => CliCommand::Search,
        "copy" => CliCommand::Copy,
        "paste" => CliCommand::Paste,
        "delete" => CliCommand::Delete,
        "export" => CliCommand::Export,
        "stats" => CliCommand::Stats,
        other => return Err(format!("unknown command: {other}")),
    };

    let args = CliArgs {
        command,
        query,
        limit,
        format,
        output_path,
    };

    let page_size_limit = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .page_size_limit();
    let search_page_size_limit = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .search_page_size_limit();

    cli::run_cli_command(
        &args,
        database.inner(),
        page_size_limit,
        search_page_size_limit,
    )
}

#[tauri::command]
pub fn start_local_api(
    api: tauri::State<'_, Mutex<LocalApiServer>>,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    port: Option<u16>,
) -> Result<LocalApiStatus, String> {
    let database = Arc::new(Database::open(&paths.database).map_err(|error| error.to_string())?);
    let mut api = api
        .lock()
        .map_err(|_| "local API server lock is poisoned".to_owned())?;
    if let Some(port) = port {
        api.set_port(port)?;
    }
    {
        let config = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        api.set_limits(config.page_size_limit(), config.search_page_size_limit());
    }
    let bound_port = api.start_with_database(database)?;
    Ok(LocalApiStatus {
        running: true,
        port: bound_port,
    })
}

#[tauri::command]
pub fn stop_local_api(api: tauri::State<'_, Mutex<LocalApiServer>>) -> Result<LocalApiStatus, String> {
    let mut api = api
        .lock()
        .map_err(|_| "local API server lock is poisoned".to_owned())?;
    api.stop()?;
    Ok(LocalApiStatus {
        running: false,
        port: api.port,
    })
}

#[tauri::command]
pub fn get_local_api_status(
    api: tauri::State<'_, Mutex<LocalApiServer>>,
) -> Result<LocalApiStatus, String> {
    let api = api
        .lock()
        .map_err(|_| "local API server lock is poisoned".to_owned())?;
    Ok(LocalApiStatus {
        running: api.is_running(),
        port: api.port,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiStatus {
    running: bool,
    port: u16,
}

#[tauri::command]
pub fn get_performance_metrics(
    performance_tracker: tauri::State<'_, PerformanceTracker>,
) -> Result<PerformanceSnapshot, String> {
    Ok(performance_tracker.snapshot())
}

#[tauri::command]
pub fn repair_database(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<RepairResult, String> {
    let result = database.repair().map_err(|e| e.to_string())?;
    if result.integrity_ok {
        refresh_database_backup(database.inner(), &paths.database).map_err(|e| e.to_string())?;
    }
    Ok(result)
}

#[tauri::command]
pub fn validate_search_index(search_index: tauri::State<'_, SearchIndex>) -> Result<bool, String> {
    Ok(search_index.validate())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconFileInfo {
    name: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCleanupResult {
    pub(crate) removed_files: u64,
    pub(crate) freed_bytes: u64,
}

#[tauri::command]
pub fn cleanup_storage_files(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<StorageCleanupResult, String> {
    cleanup_orphan_storage_files(&database, &paths)
}

fn resolve_storage_file_references(
    paths: &StoragePaths,
    icons: &Path,
    references: StorageFileReferences,
) -> HashSet<PathBuf> {
    let mut resolved = HashSet::new();
    extend_cleanup_references(
        &mut resolved,
        references.resource_paths,
        &[&paths.storage, &paths.images, &paths.files],
    );
    extend_cleanup_references(
        &mut resolved,
        references.preview_paths,
        &[&paths.storage, &paths.images, &paths.previews],
    );
    extend_cleanup_references(
        &mut resolved,
        references.icon_paths,
        &[&paths.storage, icons],
    );
    resolved
}

fn extend_cleanup_references(
    resolved: &mut HashSet<PathBuf>,
    references: Vec<String>,
    relative_bases: &[&Path],
) {
    for reference in references {
        if reference.trim().is_empty() {
            continue;
        }
        let path = Path::new(&reference);
        if path.is_absolute() {
            resolved.insert(normalized_cleanup_path(path));
        } else {
            resolved.extend(
                relative_bases
                    .iter()
                    .map(|base| normalized_cleanup_path(&base.join(&reference))),
            );
        }
    }
}

fn normalized_cleanup_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
