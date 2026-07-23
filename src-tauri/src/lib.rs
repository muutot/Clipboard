pub mod cli;
pub mod config;
pub mod content;
pub mod domain;
pub mod export;
pub mod keyboard;
pub mod ocr;
pub mod performance;
pub mod platform;
pub mod privacy;
pub mod search;
pub mod storage;

use std::{path::PathBuf, sync::Mutex};

use cli::{CliArgs, CliCommand};
use config::ConfigStore;
use content::{ClipboardFormatInfo, ContentMarkers, QuickAction, TextTransform, TransformOperation};
use domain::{ClipboardItem, ClipboardKind, OcrResult};
use export::{export_items, import_from_json, ExportFormat, ExportOptions, ImportSummary};
use keyboard::{KeyboardConfig, KeyboardManager};
use ocr::{OcrWorker, TesseractOcrEngine};
use performance::{PerformanceSnapshot, PerformanceTracker, StartupMetrics, StartupTimer};
use platform::{
    ClipboardMonitor, GlobalShortcutManager, RuntimeInfo, SingleInstanceGuard, SystemTray,
    WindowManager,
};
use privacy::PrivacyManager;
use search::{SearchIndex, SearchSyncSummary, SearchSynchronizer, SEARCH_INDEX_VERSION};
use serde::Serialize;
use storage::{ClipboardRepository, Database, OcrRepository, RepairResult, StoragePaths};
use tauri::{Emitter, Manager};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageStatus {
    schema_version: i64,
    item_count: u64,
    project_path: String,
    config_path: String,
    keyboard_config_path: String,
    data_directory_path: String,
    uses_custom_data_directory: bool,
    storage_path: String,
    database_path: String,
    files_path: String,
    image_path: String,
    search_index_path: String,
    search_index_version: u32,
    search_index_rebuild_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageDirectoryUpdate {
    data_directory_path: String,
    storage_path: String,
    restart_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationFilterSettings {
    discovered_applications: Vec<String>,
    ignored_applications: Vec<String>,
}

#[tauri::command]
fn get_runtime_info() -> RuntimeInfo {
    platform::runtime_info()
}

#[tauri::command]
fn get_storage_status(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    search_index: tauri::State<'_, SearchIndex>,
) -> Result<StorageStatus, String> {
    let config_path = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .path()
        .display()
        .to_string();
    let keyboard_config_path = keyboard
        .lock()
        .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
        .path()
        .display()
        .to_string();

    Ok(StorageStatus {
        schema_version: database
            .schema_version()
            .map_err(|error| error.to_string())?,
        item_count: database.item_count().map_err(|error| error.to_string())?,
        project_path: paths.project.display().to_string(),
        config_path,
        keyboard_config_path,
        data_directory_path: paths.data_directory.display().to_string(),
        uses_custom_data_directory: paths.uses_custom_data_directory(),
        storage_path: paths.storage.display().to_string(),
        database_path: paths.database.display().to_string(),
        files_path: paths.files.display().to_string(),
        image_path: paths.images.display().to_string(),
        search_index_path: paths.search_index.display().to_string(),
        search_index_version: SEARCH_INDEX_VERSION,
        search_index_rebuild_required: search_index.requires_full_rebuild(),
    })
}

#[tauri::command]
fn configure_storage_directory(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    active_paths: tauri::State<'_, StoragePaths>,
    data_directory: Option<String>,
) -> Result<StorageDirectoryUpdate, String> {
    let requested_directory = data_directory.map(PathBuf::from);
    let target_paths = StoragePaths::initialize_with_data_directory(
        active_paths.project.clone(),
        requested_directory,
    )
    .map_err(|error| error.to_string())?;
    let saved_directory = target_paths
        .uses_custom_data_directory()
        .then(|| target_paths.data_directory.clone());

    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_storage_directory(saved_directory)
        .map_err(|error| error.to_string())?;

    Ok(StorageDirectoryUpdate {
        restart_required: target_paths.data_directory != active_paths.data_directory,
        data_directory_path: target_paths.data_directory.display().to_string(),
        storage_path: target_paths.storage.display().to_string(),
    })
}

#[tauri::command]
fn get_application_filter_settings(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<ApplicationFilterSettings, String> {
    let discovered_applications = database
        .list_source_applications()
        .map_err(|error| error.to_string())?;
    let ignored_applications = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .ignored_applications()
        .to_vec();

    Ok(ApplicationFilterSettings {
        discovered_applications,
        ignored_applications,
    })
}

#[tauri::command]
fn configure_ignored_applications(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    applications: Vec<String>,
) -> Result<Vec<String>, String> {
    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_ignored_applications(applications)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_clipboard_items(
    database: tauri::State<'_, Database>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ClipboardItem>, String> {
    database
        .list_recent(limit.unwrap_or(100), offset.unwrap_or(0))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_clipboard_item_favorite(
    database: tauri::State<'_, Database>,
    id: String,
    is_favorite: bool,
) -> Result<bool, String> {
    database
        .set_favorite(&id, is_favorite)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_clipboard_item(database: tauri::State<'_, Database>, id: String) -> Result<bool, String> {
    database.delete_item(&id).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_clipboard_item_ocr(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<Option<OcrResult>, String> {
    database
        .get_ocr_result(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_keyboard_config(
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
) -> Result<KeyboardConfig, String> {
    Ok(keyboard
        .lock()
        .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
        .config())
}

#[tauri::command]
fn configure_keyboard_shortcuts(
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    action: String,
    shortcuts: Vec<String>,
) -> Result<Vec<String>, String> {
    keyboard
        .lock()
        .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
        .set_action_shortcuts(action, shortcuts)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn search_clipboard_items(
    database: tauri::State<'_, Database>,
    search_index: tauri::State<'_, SearchIndex>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ClipboardItem>, String> {
    SearchSynchronizer::default()
        .sync_until_idle(database.inner(), search_index.inner())
        .map_err(|error| error.to_string())?;
    let hits = search_index
        .search(&query, limit.unwrap_or(100))
        .map_err(|error| error.to_string())?;
    let item_ids = hits.into_iter().map(|hit| hit.item_id).collect::<Vec<_>>();

    database
        .get_items_by_ids(&item_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn rebuild_search_index(
    database: tauri::State<'_, Database>,
    search_index: tauri::State<'_, SearchIndex>,
) -> Result<SearchSyncSummary, String> {
    SearchSynchronizer::default()
        .rebuild(database.inner(), search_index.inner())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_clipboard_formats(window: tauri::Window) -> Result<ClipboardFormatInfo, String> {
    let _ = window;
    Ok(ClipboardFormatInfo::empty())
}

#[tauri::command]
fn get_ocr_status(
    database: tauri::State<'_, Database>,
) -> Result<OcrStatusInfo, String> {
    let pending = database.count_pending_ocr().map_err(|e| e.to_string())?;
    let completed = database.count_completed_ocr().map_err(|e| e.to_string())?;
    let tesseract_available = TesseractOcrEngine::is_available();

    Ok(OcrStatusInfo {
        pending_tasks: pending,
        completed_tasks: completed,
        tesseract_available,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrStatusInfo {
    pending_tasks: u64,
    completed_tasks: u64,
    tesseract_available: bool,
}

#[tauri::command]
fn detect_content_markers(text: String) -> ContentMarkers {
    content::detect_markers(&text)
}

#[tauri::command]
fn detect_content_actions(text: String) -> Vec<QuickAction> {
    let markers = content::detect_markers(&text);
    content::detect_actions(&markers)
}

#[tauri::command]
fn soft_delete_clipboard_item(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database
        .soft_delete(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn clear_all_non_favorite_items(
    database: tauri::State<'_, Database>,
) -> Result<u64, String> {
    database
        .clear_all_non_favorite_items()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn restore_clipboard_item(database: tauri::State<'_, Database>, id: String) -> Result<bool, String> {
    database
        .restore_deleted(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn enforce_history_cleanup(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<u64, String> {
    let (retention_days, max_items, recycle_bin_days) = {
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        (guard.retention_days(), guard.max_items(), guard.recycle_bin_days())
    };

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

    Ok(total_deleted)
}

#[tauri::command]
fn transform_text(input: String, operation: String) -> Result<TextTransform, String> {
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
fn toggle_privacy_pause(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
) -> Result<bool, String> {
    let mut privacy = privacy
        .lock()
        .map_err(|_| "privacy manager lock is poisoned".to_owned())?;
    privacy.toggle_pause();

    let paused = privacy.is_paused();
    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_privacy_paused(paused)
        .map_err(|e| e.to_string())?;

    Ok(paused)
}

#[tauri::command]
fn check_sensitive_content(
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    text: String,
) -> Result<bool, String> {
    Ok(privacy
        .lock()
        .map_err(|_| "privacy manager lock is poisoned".to_owned())?
        .is_sensitive_content(&text))
}

#[tauri::command]
fn check_password_manager(
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    app_name: String,
) -> Result<bool, String> {
    Ok(privacy
        .lock()
        .map_err(|_| "privacy manager lock is poisoned".to_owned())?
        .is_password_manager(&app_name))
}

#[tauri::command]
fn get_privacy_status(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
) -> Result<PrivacyStatus, String> {
    let privacy = privacy
        .lock()
        .map_err(|_| "privacy manager lock is poisoned".to_owned())?;
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;

    Ok(PrivacyStatus {
        paused: privacy.is_paused(),
        password_manager_apps: privacy.password_manager_apps.clone(),
        master_password_hash_set: config.privacy_master_password_hash().is_some(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivacyStatus {
    paused: bool,
    password_manager_apps: Vec<String>,
    master_password_hash_set: bool,
}

#[tauri::command]
fn export_clipboard_items(
    database: tauri::State<'_, Database>,
    format: String,
    include_favorites: Option<bool>,
    date_from_ms: Option<i64>,
    date_to_ms: Option<i64>,
    content_types: Option<Vec<String>>,
) -> Result<String, String> {
    let export_format = match format.as_str() {
        "json" => ExportFormat::Json,
        "csv" => ExportFormat::Csv,
        "plainText" => ExportFormat::PlainText,
        other => return Err(format!("unknown export format: {other}")),
    };

    let options = ExportOptions {
        format: export_format,
        include_favorites: include_favorites.unwrap_or(true),
        date_from_ms,
        date_to_ms,
        content_types: content_types.unwrap_or_else(|| {
            vec![
                "text".to_owned(),
                "link".to_owned(),
                "image".to_owned(),
                "file".to_owned(),
            ]
        }),
    };

    let items = database
        .list_recent(10_000, 0)
        .map_err(|e| e.to_string())?;

    export_items(&items, &options)
}

#[tauri::command]
fn import_clipboard_items(
    database: tauri::State<'_, Database>,
    json: String,
) -> Result<ImportSummary, String> {
    import_from_json(&json, database.inner())
}

#[tauri::command]
fn get_export_formats() -> Result<Vec<ExportFormatInfo>, String> {
    Ok(vec![
        ExportFormatInfo {
            id: "json".to_owned(),
            label: "JSON".to_owned(),
            extension: ".json".to_owned(),
        },
        ExportFormatInfo {
            id: "csv".to_owned(),
            label: "CSV".to_owned(),
            extension: ".csv".to_owned(),
        },
        ExportFormatInfo {
            id: "plainText".to_owned(),
            label: "Plain Text".to_owned(),
            extension: ".txt".to_owned(),
        },
    ])
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportFormatInfo {
    id: String,
    label: String,
    extension: String,
}

#[tauri::command]
fn start_clipboard_monitoring(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    paths: tauri::State<'_, StoragePaths>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
) -> Result<bool, String> {
    let mut guard = monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?;
    guard.start()?;

    let receiver = guard
        .take_receiver()
        .ok_or("clipboard monitor started but no receiver available".to_owned())?;

    drop(guard);

    let db_path = paths.database.clone();
    let privacy_arc = {
        let p = privacy
            .lock()
            .map_err(|_| "privacy manager lock is poisoned".to_owned())?;
        Arc::new(Mutex::new(p.paused))
    };

    thread::spawn(move || {
        let database = match Database::open(&db_path) {
            Ok(db) => Arc::new(db),
            Err(e) => {
                eprintln!("[clipboard-worker] failed to open database: {e}");
                return;
            }
        };

        let mut self_trigger_guard = content::hash::SelfTriggerGuard::new();
        let mut consecutive_errors = 0u32;

        loop {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(_change) => {
                    let is_paused = privacy_arc
                        .lock()
                        .map(|g| *g)
                        .unwrap_or(false);

                    if is_paused {
                        continue;
                    }

                    let text = match platform::windows_clipboard::read_clipboard_text() {
                        Some(t) => t,
                        None => continue,
                    };

                    if text.is_empty() || text.len() > 500_000 {
                        continue;
                    }

                    let markers = content::detect_markers(&text);
                    let kind = if markers.is_link || markers.has_url {
                        ClipboardKind::Link
                    } else {
                        ClipboardKind::Text
                    };

                    let content_hash = content::hash::compute_content_hash(
                        if kind == ClipboardKind::Link { "link" } else { "text" },
                        &text,
                        None,
                    );

                    if self_trigger_guard.is_self_triggered(&content_hash) {
                        self_trigger_guard.mark_as_self_triggered(&content_hash);
                        continue;
                    }

                    let title = text
                        .chars()
                        .take(200)
                        .collect::<String>()
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let size_bytes = text.len() as u64;
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;

                    let item = ClipboardItem {
                        id: content_hash.clone(),
                        kind,
                        title,
                        text_content: Some(text),
                        resource_path: None,
                        preview_path: None,
                        content_hash,
                        source_app: None,
                        icon_path: None,
                        size_bytes,
                        created_at_ms: now_ms,
                        last_used_at_ms: None,
                        is_favorite: false,
                    };

                    match database.save_item(&item) {
                        Ok(_) => {
                            consecutive_errors = 0;
                        }
                        Err(e) => {
                            eprintln!("[clipboard-worker] failed to save item: {e}");
                            consecutive_errors += 1;
                            if consecutive_errors >= 10 {
                                eprintln!(
                                    "[clipboard-worker] too many consecutive errors, pausing"
                                );
                                thread::sleep(Duration::from_secs(5));
                                consecutive_errors = 0;
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    eprintln!("[clipboard-worker] monitor channel disconnected, stopping");
                    break;
                }
            }
        }
    });

    Ok(true)
}

#[tauri::command]
fn stop_clipboard_monitoring(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
) -> Result<bool, String> {
    monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?
        .stop()?;
    Ok(true)
}

#[tauri::command]
fn get_clipboard_monitor_status(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
) -> Result<ClipboardMonitorStatus, String> {
    let monitor = monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?;
    Ok(ClipboardMonitorStatus {
        running: monitor.running,
        ignored_applications: monitor.ignored_applications.clone(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardMonitorStatus {
    running: bool,
    ignored_applications: Vec<String>,
}

#[tauri::command]
fn set_clipboard_ignored_apps(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    apps: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut monitor = monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?;
    monitor.set_ignored_apps(apps);
    Ok(monitor.ignored_applications.clone())
}

#[tauri::command]
fn save_window_position(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let mut guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    WindowManager::save_position(
        &mut *guard,
        x,
        y,
        width,
        height,
    )
}

#[tauri::command]
fn restore_window_position(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<Option<WindowPosition>, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    Ok(WindowManager::restore_position(&config).map(|(x, y, w, h)| WindowPosition { x, y, width: w, height: h }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowPosition {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[tauri::command]
fn get_window_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<WindowConfigInfo, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    Ok(WindowConfigInfo {
        launch_at_startup: config.launch_at_startup(),
        close_to_tray: config.close_to_tray(),
        single_instance: config.single_instance(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowConfigInfo {
    launch_at_startup: bool,
    close_to_tray: bool,
    single_instance: bool,
}

#[tauri::command]
fn set_window_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    launch_at_startup: Option<bool>,
    close_to_tray: Option<bool>,
    single_instance: Option<bool>,
) -> Result<(), String> {
    let mut config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    if let Some(v) = launch_at_startup {
        config.set_launch_at_startup(v).map_err(|e| e.to_string())?;
    }
    if let Some(v) = close_to_tray {
        config.set_close_to_tray(v).map_err(|e| e.to_string())?;
    }
    if let Some(v) = single_instance {
        config.set_single_instance(v).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn get_export_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<ExportConfigInfo, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    Ok(ExportConfigInfo {
        schedule_auto_export: config.schedule_auto_export().map(|s| s.to_owned()),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportConfigInfo {
    schedule_auto_export: Option<String>,
}

#[tauri::command]
fn set_export_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    schedule_auto_export: Option<String>,
) -> Result<(), String> {
    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_schedule_auto_export(schedule_auto_export)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn run_cli_command(
    database: tauri::State<'_, Database>,
    command: String,
    query: Option<String>,
    limit: Option<usize>,
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
    };

    cli::run_cli_command(&args, database.inner())
}

#[tauri::command]
fn get_performance_metrics(
    performance_tracker: tauri::State<'_, PerformanceTracker>,
) -> Result<PerformanceSnapshot, String> {
    Ok(performance_tracker.snapshot())
}

#[tauri::command]
fn repair_database(
    database: tauri::State<'_, Database>,
) -> Result<RepairResult, String> {
    database.repair().map_err(|e| e.to_string())
}

#[tauri::command]
fn validate_search_index(
    search_index: tauri::State<'_, SearchIndex>,
) -> Result<bool, String> {
    Ok(search_index.validate())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let startup_timer = &mut StartupTimer::start();
            let project_directory = app.path().app_data_dir()?;
            let config = ConfigStore::load(&project_directory)?;
            let keyboard = KeyboardManager::load(&project_directory)?;
            let paths = StoragePaths::initialize_with_data_directory(
                project_directory.clone(),
                config.storage_directory().map(PathBuf::from),
            )?;
            let database = Database::open(&paths.database)?;

            // Auto-recovery: check and repair if needed
            match database.repair() {
                Ok(result) => {
                    if !result.integrity_ok {
                        eprintln!(
                            "[recovery] database integrity check failed: {}",
                            result.integrity_message
                        );
                    }
                }
                Err(e) => {
                    eprintln!("[recovery] database repair check failed: {e}");
                }
            }

            database.requeue_interrupted_ocr()?;
            let db_open_duration = startup_timer.finish_segment();

            let search_index = SearchIndex::open(&paths.search_index)?;
            if !search_index.validate() {
                eprintln!("[recovery] search index validation failed, will be rebuilt");
            }
            let _search_init_result = SearchSynchronizer::default().initialize(&database, &search_index);
            let search_init_duration = startup_timer.finish_segment();

            let migrations_ms = 0;
            let startup_metrics = StartupMetrics {
                total_startup_ms: db_open_duration.as_millis() as u64
                    + search_init_duration.as_millis() as u64,
                db_open_ms: db_open_duration.as_millis() as u64,
                search_init_ms: search_init_duration.as_millis() as u64,
                migrations_ms,
            };
            startup_metrics.log_summary();

            let performance_tracker = PerformanceTracker::new();
            performance_tracker.record_startup(startup_metrics.clone());

            let ocr_engine = Arc::new(TesseractOcrEngine::new());
            let ocr_database = Database::open(&paths.database)?;
            let ocr_worker = OcrWorker::start(ocr_engine, Arc::new(ocr_database));

            let mut privacy_manager = PrivacyManager::new();
            privacy_manager.sync_with_config(&config);
            let mut clipboard_monitor = ClipboardMonitor::new();
            let shortcut_manager = GlobalShortcutManager::new();

            if config.single_instance() {
                match SingleInstanceGuard::acquire(&project_directory) {
                    Ok(guard) => {
                        app.manage(guard);
                    }
                    Err(e) => {
                        eprintln!("[startup] {}", e);
                    }
                }
            }

            // Graceful shutdown handler
            let ocr_worker_for_shutdown = ocr_worker.clone();
            let paths_for_shutdown = paths.clone();
            ctrlc::set_handler(move || {
                eprintln!("[shutdown] received interrupt signal, cleaning up...");
                ocr_worker_for_shutdown.stop();
                // Config is auto-saved on drop
                let _ = paths_for_shutdown;
                std::process::exit(0);
            })
            .ok();

            // Auto-start clipboard monitoring in background
            let app_handle = app.handle().clone();
            let db_path = paths.database.clone();
            let storage_path = paths.storage.clone();
            let privacy_paused = Arc::new(Mutex::new(false));

            if clipboard_monitor.start().is_ok() {
                if let Some(receiver) = clipboard_monitor.take_receiver() {
                    thread::spawn(move || {
                    let database = match Database::open(&db_path) {
                        Ok(db) => db,
                        Err(e) => {
                            eprintln!("[clipboard-worker] failed to open database: {e}");
                            return;
                        }
                    };

                    let mut self_trigger_guard = content::hash::SelfTriggerGuard::new();
                    let mut consecutive_errors = 0u32;

                    loop {
                        match receiver.recv_timeout(Duration::from_millis(500)) {
                            Ok(_change) => {
                                let is_paused = privacy_paused
                                    .lock()
                                    .map(|g| *g)
                                    .unwrap_or(false);

                                if is_paused {
                                    continue;
                                }

                                let source_app_name = platform::windows_clipboard::get_foreground_app();
                                let source_app = if source_app_name.is_empty() { None } else { Some(source_app_name.clone()) };

                                // Extract and cache app icon
                                let icon_dir = storage_path.join("icons");
                                let icon_path = if !source_app_name.is_empty() {
                                    platform::windows_clipboard::extract_app_icon(&icon_dir, &source_app_name)
                                } else {
                                    None
                                };

                                let text = platform::windows_clipboard::read_clipboard_text();
                                let image_data = platform::windows_clipboard::read_clipboard_image();
                                let file_paths = platform::windows_clipboard::read_clipboard_file_paths();

                                if image_data.is_some() {
                                    let img = image_data.unwrap();
                                    let img_hash = content::hash::compute_content_hash("image", "", None);
                                    let now_ms = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as i64;

                                    let image_dir = storage_path.join("image");
                                    std::fs::create_dir_all(&image_dir).ok();
                                    let img_path = image_dir.join(format!("{}.png", img_hash));
                                    match std::fs::write(&img_path, &img) {
                                        Ok(_) => eprintln!("[clipboard-worker] saved image: {}", img_path.display()),
                                        Err(e) => eprintln!("[clipboard-worker] failed to write image {}: {}", img_path.display(), e),
                                    }

                                    let item = ClipboardItem {
                                        id: format!("img_{}", img_hash),
                                        kind: ClipboardKind::Image,
                                        title: format!("图片 ({})", img_path.file_name().unwrap_or_default().to_string_lossy()),
                                        text_content: None,
                                        resource_path: Some(img_path.to_string_lossy().to_string()),
                                        preview_path: Some(img_path.to_string_lossy().to_string()),
                                        content_hash: img_hash,
                                            source_app: source_app.clone(),
                                            icon_path: icon_path.clone(),
                                            size_bytes: img.len() as u64,
                                        created_at_ms: now_ms,
                                        last_used_at_ms: None,
                                        is_favorite: false,
                                    };

                                    match database.save_item(&item) {
                                        Ok(saved_id) => {
                                            consecutive_errors = 0;
                                            let _ = database.enqueue_ocr(&saved_id);
                                            let mut emit_item = item.clone();
                                            emit_item.id = saved_id;
                                            let _ = app_handle.emit("clipboard-item-added", &emit_item);
                                            continue;
                                        }
                                        Err(e) => {
                                            eprintln!("[clipboard-worker] failed to save image: {e}");
                                        }
                                    }
                                    continue;
                                }

                                if !file_paths.is_empty() {
                                    for file_path in &file_paths {
                                        let file_hash = content::hash::compute_content_hash("file", file_path, None);
                                        let now_ms = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis() as i64;

                                        let file_size = std::fs::metadata(file_path)
                                            .map(|m| m.len())
                                            .unwrap_or(0);

                                        let file_name = std::path::Path::new(file_path)
                                            .file_name()
                                            .map(|n| n.to_string_lossy().to_string())
                                            .unwrap_or_default();

                                        let item = ClipboardItem {
                                            id: format!("file_{}", file_hash),
                                            kind: ClipboardKind::File,
                                            title: file_name.clone(),
                                            text_content: None,
                                            resource_path: Some(file_path.clone()),
                                            preview_path: None,
                                            content_hash: file_hash,
                                        source_app: source_app.clone(),
                                        icon_path: icon_path.clone(),
                                        size_bytes: file_size,
                                            created_at_ms: now_ms,
                                            last_used_at_ms: None,
                                            is_favorite: false,
                                        };

                                        match database.save_item(&item) {
                                            Ok(saved_id) => {
                                                consecutive_errors = 0;
                                                let mut emit_item = item.clone();
                                                emit_item.id = saved_id;
                                                let _ = app_handle.emit("clipboard-item-added", &emit_item);
                                            }
                                            Err(e) => {
                                                eprintln!("[clipboard-worker] failed to save file: {e}");
                                            }
                                        }
                                    }
                                    continue;
                                }

                                let text = match text {
                                    Some(t) => t,
                                    None => continue,
                                };

                                if text.is_empty() || text.len() > 500_000 {
                                    continue;
                                }

                                let markers = content::detect_markers(&text);
                                let kind = if markers.is_link {
                                    ClipboardKind::Link
                                } else {
                                    ClipboardKind::Text
                                };

                                let content_hash = content::hash::compute_content_hash(
                                    if kind == ClipboardKind::Link { "link" } else { "text" },
                                    &text,
                                    None,
                                );

                                if self_trigger_guard.is_self_triggered(&content_hash) {
                                    continue;
                                }

                                let title = text
                                    .chars()
                                    .take(200)
                                    .collect::<String>()
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                                    .to_string();
                                let size_bytes = text.len() as u64;
                                let now_ms = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as i64;

                                let item = ClipboardItem {
                                    id: format!("{}_{}", content_hash, now_ms),
                                    kind,
                                    title: title.clone(),
                                    text_content: Some(text.clone()),
                                    resource_path: None,
                                    preview_path: None,
                                    content_hash: content_hash.clone(),
                                    source_app: source_app.clone(),
                                    icon_path: icon_path.clone(),
                                    size_bytes,
                                    created_at_ms: now_ms,
                                    last_used_at_ms: None,
                                    is_favorite: false,
                                };

                                match database.save_item(&item) {
                                    Ok(saved_id) => {
                                        consecutive_errors = 0;
                                        let mut emit_item = item.clone();
                                        emit_item.id = saved_id;
                                        let _ = app_handle.emit("clipboard-item-added", &emit_item);
                                    }
                                    Err(e) => {
                                        eprintln!("[clipboard-worker] failed to save item: {e}");
                                        consecutive_errors += 1;
                                        if consecutive_errors >= 10 {
                                            eprintln!("[clipboard-worker] too many errors, pausing");
                                            thread::sleep(Duration::from_secs(5));
                                            consecutive_errors = 0;
                                        }
                                    }
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                eprintln!("[clipboard-worker] monitor disconnected, stopping");
                                break;
                            }
                        }
                    }
                });
            } else {
                eprintln!("[startup] clipboard monitor has no receiver");
            }
        } else {
            eprintln!("[startup] failed to start clipboard monitor");
        }


            app.manage(Mutex::new(config));
            app.manage(Mutex::new(keyboard));
            app.manage(paths);
            app.manage(database);
            app.manage(search_index);
            app.manage(performance_tracker);
            app.manage(Mutex::new(privacy_manager));
            app.manage(Mutex::new(clipboard_monitor));
            app.manage(Mutex::new(shortcut_manager));
            app.manage(ocr_worker);

            let _tray = SystemTray::create().ok();

            // Wire global hotkey (Alt+V) to toggle window
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                use platform::windows_clipboard;

                match window.hwnd() {
                    Ok(_hwnd) => {
                        let (tx, rx) = mpsc::channel::<()>();
                        let _hotkey_thread = platform::windows_hotkey::spawn_hotkey_thread(
                            1,
                            windows_clipboard::MOD_ALT,
                            windows_clipboard::VK_V,
                            tx,
                        );

                        let window_clone = window.clone();
                        thread::spawn(move || {
                            loop {
                                match rx.recv() {
                                    Ok(()) => {
                                        let is_visible = window_clone.is_visible().unwrap_or(false);
                                        let is_focused = window_clone.is_focused().unwrap_or(false);
                                        if !is_visible {
                                            let _ = window_clone.show();
                                        }
                                        if !is_focused {
                                            let _ = window_clone.set_focus();
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("[hotkey] failed to get window handle: {e}");
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_info,
            get_storage_status,
            configure_storage_directory,
            get_application_filter_settings,
            configure_ignored_applications,
            list_clipboard_items,
            set_clipboard_item_favorite,
            delete_clipboard_item,
            get_clipboard_item_ocr,
            get_keyboard_config,
            configure_keyboard_shortcuts,
            search_clipboard_items,
            rebuild_search_index,
            detect_content_markers,
            transform_text,
            toggle_privacy_pause,
            check_sensitive_content,
            check_password_manager,
            get_privacy_status,
            export_clipboard_items,
            import_clipboard_items,
            get_export_formats,
            start_clipboard_monitoring,
            stop_clipboard_monitoring,
            get_clipboard_monitor_status,
            set_clipboard_ignored_apps,
            save_window_position,
            restore_window_position,
            get_window_config,
            set_window_config,
            get_export_config,
            set_export_config,
            run_cli_command,
            get_clipboard_formats,
            get_ocr_status,
            detect_content_actions,
            soft_delete_clipboard_item,
            restore_clipboard_item,
            enforce_history_cleanup,
            clear_all_non_favorite_items,
            get_performance_metrics,
            repair_database,
            validate_search_index
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
