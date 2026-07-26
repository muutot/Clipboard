pub mod cli;
pub mod config;
pub mod content;
pub mod domain;
pub mod export;
pub mod keyboard;
pub mod memory;
pub mod ocr;
pub mod performance;
pub mod platform;
pub mod privacy;
pub mod search;
pub mod storage;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

use cli::{CliArgs, CliCommand, LocalApiServer};
use config::{ConfigStore, GeneralConfig};
use content::{
    accessed_at_ms, created_at_ms, extension_for_path, mime_type_for_path, modified_at_ms,
    ClipboardFormatInfo, ContentMarkers, FileStore, QuickAction, TextTransform, ThumbnailWorker,
    TransformOperation, RESOURCE_METADATA_SCHEMA_VERSION,
};
use domain::{ClipboardItem, ClipboardKind, OcrResult};
use export::{
    export_database, import_from_json, import_from_plain_text, ExportFormat, ExportOptions,
    ImportSummary,
};
use keyboard::{KeyboardConfig, KeyboardManager};
use ocr::{NoopOcrEngine, OcrEngine, OcrWorkerManager, PpOcrEngine, TesseractOcrEngine};
use performance::{PerformanceSnapshot, PerformanceTracker, StartupMetrics, StartupTimer};
use platform::windows_hotkey::{
    shortcut_bindings_to_double_modifiers, shortcut_bindings_to_windows_hotkeys, HotkeyManager,
};
use platform::{
    show_main_window, sync_autostart, ClipboardMonitor, GlobalShortcutManager, RuntimeInfo,
    SingleInstanceError, SingleInstanceGuard, SystemTray, WindowManager,
};
use privacy::PrivacyManager;
use rusqlite::params;
use search::{SearchIndex, SearchSyncSummary, SearchSynchronizer, SEARCH_INDEX_VERSION};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::str::FromStr;
use std::time::{Duration, Instant};
use storage::{
    quarantine_search_index, recover_database_if_needed, refresh_database_backup,
    ClipboardRepository, Database, KindDeleteResult, KindDeleteScope, KindStorageStats,
    OcrRepository, RepairResult, StorageFileReferences, StoragePaths, TextItemUpdate,
    RESOURCE_ROOT_MARKER,
};
use tauri::{Emitter, Manager};

const TOGGLE_WINDOW_ACTION: &str = "toggleWindow";
const STORAGE_KIND_DELETE_SCOPE: KindDeleteScope = KindDeleteScope {
    include_favorites: false,
    include_deleted: true,
};
const MAIN_WINDOW_MIN_WIDTH: u32 = 730;
const MAIN_WINDOW_MIN_HEIGHT: u32 = 500;

fn resolve_toggle_hotkeys(config: &KeyboardConfig) -> (Vec<(u32, u32)>, Vec<keyboard::Modifier>) {
    let Some(shortcuts) = config.shortcuts.get(TOGGLE_WINDOW_ACTION) else {
        return (Vec::new(), Vec::new());
    };

    let bindings = shortcuts
        .iter()
        .filter_map(|shortcut| keyboard::ShortcutBinding::from_str(shortcut).ok())
        .collect::<Vec<_>>();

    (
        shortcut_bindings_to_windows_hotkeys(&bindings),
        shortcut_bindings_to_double_modifiers(&bindings),
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageStatus {
    item_count: u64,
    image_count: u64,
    image_size_bytes: u64,
    file_count: u64,
    file_size_bytes: u64,
    text_count: u64,
    link_count: u64,
    project_path: String,
    config_path: String,
    keyboard_config_path: String,
    data_directory_path: String,
    uses_custom_data_directory: bool,
    storage_path: String,
    icons_dir: String,
    database_path: String,
    database_size_bytes: u64,
    files_path: String,
    image_path: String,
    image_cleanup_enabled: bool,
    file_cleanup_enabled: bool,
    search_index_path: String,
    search_index_size_bytes: u64,
    search_index_version: u32,
    search_index_rebuild_required: bool,
    disk_total_bytes: Option<u64>,
    disk_available_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageKindStats {
    item_count: u64,
    size_bytes: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageKindDeleteExpectation {
    item_count: u64,
    size_bytes: u64,
}

impl From<KindStorageStats> for StorageKindStats {
    fn from(stats: KindStorageStats) -> Self {
        Self {
            item_count: stats.item_count,
            size_bytes: stats.size_bytes,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageKindDeleteResult {
    deleted_count: u64,
    deleted_size_bytes: u64,
    removed_files: u64,
    search_sync: Option<SearchSyncSummary>,
    warnings: Vec<String>,
    #[serde(skip_serializing)]
    deleted_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardHistoryInvalidated {
    deleted_ids: Vec<String>,
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
struct ResourceStorageUpdate {
    image_storage_path: String,
    file_storage_path: String,
    restart_required: bool,
}

/// Shared state for ignored applications list, synced between capture thread and Tauri commands.
/// Capture policy shared by every clipboard ingestion path.
///
/// The command handlers and background workers do not share a Tauri `State`
/// reference directly.  Instead they hold this small, thread-safe snapshot so
/// pause/ignore changes take effect without restarting a monitor thread.
#[derive(Clone)]
struct CaptureState {
    paused: Arc<AtomicBool>,
    max_file_copy_size_bytes: Arc<AtomicU64>,
    ignored_apps: Arc<Mutex<Vec<String>>>,
    policy: Arc<CapturePolicy>,
    ingestion_guard: Arc<Mutex<()>>,
    worker: Arc<Mutex<Option<CaptureWorker>>>,
}

#[derive(Clone)]
struct CapturePolicy {
    sensitive_patterns: Arc<Vec<regex_lite::Regex>>,
    password_manager_apps: Arc<Vec<String>>,
}

struct CaptureWorker {
    stop_flag: Arc<AtomicBool>,
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

const HISTORY_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const SCHEDULED_ORPHAN_FILE_GRACE: Duration = Duration::from_secs(10 * 60);

struct CleanupWorker {
    stop_flag: Arc<AtomicBool>,
    stop_sender: Mutex<Option<mpsc::Sender<()>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl CleanupWorker {
    fn start(
        project_directory: PathBuf,
        database: Database,
        paths: StoragePaths,
    ) -> Result<Self, String> {
        Self::start_with_interval(project_directory, database, paths, HISTORY_CLEANUP_INTERVAL)
    }

    fn start_with_interval(
        project_directory: PathBuf,
        database: Database,
        paths: StoragePaths,
        interval: Duration,
    ) -> Result<Self, String> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let (stop_sender, stop_receiver) = mpsc::channel();
        let worker_flag = Arc::clone(&stop_flag);
        let handle = thread::Builder::new()
            .name("history-cleanup".to_owned())
            .spawn(move || loop {
                if worker_flag.load(Ordering::SeqCst) {
                    break;
                }

                match ConfigStore::load(&project_directory) {
                    Ok(config) => match enforce_history_cleanup_for(
                        &database,
                        &config,
                        &paths,
                        SCHEDULED_ORPHAN_FILE_GRACE,
                    ) {
                        Ok(total_deleted) if total_deleted > 0 => {
                            eprintln!("[cleanup] removed {total_deleted} expired history entries");
                        }
                        Ok(_) => {}
                        Err(error) => eprintln!("[cleanup] scheduled cleanup failed: {error}"),
                    },
                    Err(error) => eprintln!("[cleanup] failed to load configuration: {error}"),
                }

                if wait_for_stop(&stop_receiver, &worker_flag, interval) {
                    break;
                }
            })
            .map_err(|error| format!("failed to start history cleanup worker: {error}"))?;

        Ok(Self {
            stop_flag,
            stop_sender: Mutex::new(Some(stop_sender)),
            handle: Mutex::new(Some(handle)),
        })
    }

    fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(sender) = self
            .stop_sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = sender.send(());
        }

        let handle = self
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(handle) = handle {
            if handle.thread().id() != thread::current().id() && handle.join().is_err() {
                eprintln!("[cleanup] history cleanup thread terminated with a panic");
            }
        }
    }
}

impl Drop for CleanupWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

impl CaptureState {
    fn new(
        privacy: &PrivacyManager,
        ignored_apps: Vec<String>,
        max_file_copy_size_bytes: u64,
    ) -> Self {
        let sensitive_patterns = privacy
            .sensitive_patterns
            .iter()
            .filter_map(|pattern| match regex_lite::Regex::new(pattern) {
                Ok(regex) => Some(regex),
                Err(error) => {
                    eprintln!("[privacy] ignoring invalid sensitive pattern {pattern:?}: {error}");
                    None
                }
            })
            .collect();
        Self {
            paused: Arc::new(AtomicBool::new(privacy.is_paused())),
            max_file_copy_size_bytes: Arc::new(AtomicU64::new(max_file_copy_size_bytes)),
            ignored_apps: Arc::new(Mutex::new(normalize_app_list(&ignored_apps))),
            policy: Arc::new(CapturePolicy {
                sensitive_patterns: Arc::new(sensitive_patterns),
                password_manager_apps: Arc::new(privacy.password_manager_apps.clone()),
            }),
            ingestion_guard: Arc::new(Mutex::new(())),
            worker: Arc::new(Mutex::new(None)),
        }
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    fn set_max_file_copy_size_bytes(&self, value: u64) {
        self.max_file_copy_size_bytes.store(value, Ordering::SeqCst);
    }

    fn max_file_copy_size_bytes(&self) -> u64 {
        self.max_file_copy_size_bytes.load(Ordering::SeqCst)
    }

    fn set_ignored_apps(&self, apps: Vec<String>) -> Vec<String> {
        let normalized = normalize_app_list(&apps);
        if let Ok(mut ignored) = self.ignored_apps.lock() {
            *ignored = normalized.clone();
        }
        normalized
    }

    fn ignored_apps(&self) -> Vec<String> {
        self.ignored_apps
            .lock()
            .map(|apps| apps.clone())
            .unwrap_or_default()
    }

    fn should_skip(&self, source_app: Option<&str>, text: Option<&str>) -> bool {
        if self.is_paused() {
            return true;
        }

        let ignored = self
            .ignored_apps
            .lock()
            .map(|apps| apps.clone())
            .unwrap_or_default();
        self.policy.should_skip(&ignored, source_app, text)
    }

    fn install_worker(&self, worker: CaptureWorker) {
        self.stop_worker();
        let mut slot = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(worker);
    }

    fn stop_worker(&self) {
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(mut worker) = worker {
            worker.stop();
        }
    }

    fn worker_running(&self) -> bool {
        self.worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .and_then(|worker| worker.handle.as_ref())
            .is_some_and(|handle| !handle.is_finished())
    }
}

impl CapturePolicy {
    fn should_skip(
        &self,
        ignored_apps: &[String],
        source_app: Option<&str>,
        text: Option<&str>,
    ) -> bool {
        if source_app.is_some_and(|app| {
            app_matches(app, ignored_apps) || app_matches(app, &self.password_manager_apps)
        }) {
            return true;
        }

        text.is_some_and(|text| {
            self.sensitive_patterns
                .iter()
                .any(|pattern| pattern.is_match(text))
        })
    }
}

impl CaptureWorker {
    fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }

        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                if handle.join().is_err() {
                    eprintln!("[clipboard-worker] capture thread terminated with a panic");
                }
            } else {
                // A worker must not join itself.  This path is defensive (the
                // normal stop command runs on the Tauri thread).
                self.handle = Some(handle);
            }
        }
    }
}

impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn normalize_app_list(apps: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for app in apps {
        let app = app.trim().to_owned();
        if !app.is_empty()
            && !normalized
                .iter()
                .any(|existing| normalize_app_name(existing) == normalize_app_name(&app))
        {
            normalized.push(app);
        }
    }
    normalized
}

fn app_matches(app: &str, candidates: &[String]) -> bool {
    let app = normalize_app_name(app);
    !app.is_empty()
        && candidates
            .iter()
            .map(|candidate| normalize_app_name(candidate))
            .any(|candidate| candidate == app)
}

fn normalize_app_name(app: &str) -> String {
    let trimmed = app.trim();
    let leaf = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let path_name = Path::new(leaf)
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| leaf.to_owned());
    path_name.to_lowercase()
}

fn foreground_app_name(app: &platform::windows_clipboard::ForegroundApp) -> Option<String> {
    if !app.exe_path.trim().is_empty() {
        let leaf = app
            .exe_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&app.exe_path);
        Path::new(leaf)
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
    } else if !app.name.trim().is_empty() {
        Some(app.name.trim().to_owned())
    } else {
        None
    }
}

fn should_skip_self_triggered_hash(
    guard: &mut content::hash::SelfTriggerGuard,
    content_hash: &str,
) -> bool {
    guard.is_self_triggered(content_hash)
}

fn should_skip_self_triggered_text(
    guard: &mut content::hash::SelfTriggerGuard,
    kind: ClipboardKind,
    text: &str,
) -> bool {
    let kind_name = match kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image | ClipboardKind::File => return false,
    };
    guard.is_text_write_self_triggered(kind_name, text)
}

fn should_skip_self_triggered_media(
    guard: &mut content::hash::SelfTriggerGuard,
    kind: &str,
    data: &[u8],
) -> bool {
    guard.is_media_write_self_triggered(kind, data)
}

fn register_image_self_trigger(
    guard: &mut content::hash::SelfTriggerGuard,
    resource_path: Option<&str>,
    fallback_hash: Option<&str>,
) -> Result<(), String> {
    let mut registered = false;

    if let Some(path) = resource_path.filter(|path| !path.trim().is_empty()) {
        match std::fs::read(path) {
            Ok(data) => {
                guard.mark_media_write("image", &data);
                registered = true;
            }
            Err(error) if fallback_hash.is_none() => {
                return Err(format!("failed to read image for self-trigger: {error}"));
            }
            Err(_) => {}
        }
    }

    if let Some(content_hash) = fallback_hash.filter(|hash| !hash.trim().is_empty()) {
        guard.mark_as_self_triggered(content_hash);
        registered = true;
    }

    if registered {
        Ok(())
    } else {
        Err("image self-trigger has no readable resource or content hash".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedFileReference {
    original_path: String,
    storage_path: String,
    original_name: String,
    size_bytes: u64,
    content_hash: Option<String>,
    extension: Option<String>,
    mime_type: String,
    created_at_ms: Option<i64>,
    modified_at_ms: Option<i64>,
    accessed_at_ms: Option<i64>,
    read_only: bool,
    is_directory: bool,
    copied: bool,
}

fn store_captured_file_references(
    file_paths: &[String],
    file_storage_dir: &Path,
    max_copy_size_bytes: u64,
) -> Vec<CapturedFileReference> {
    file_paths
        .iter()
        .map(|file_path| {
            let source_path = Path::new(file_path);
            match FileStore::save_file(source_path, file_storage_dir, max_copy_size_bytes) {
                Ok(info) => CapturedFileReference {
                    original_path: file_path.clone(),
                    copied: Path::new(&info.storage_path) != source_path,
                    storage_path: info.storage_path,
                    original_name: info.original_name,
                    size_bytes: info.size_bytes,
                    content_hash: Some(info.content_hash),
                    extension: info.extension,
                    mime_type: info.mime_type,
                    created_at_ms: info.created_at_ms,
                    modified_at_ms: info.modified_at_ms,
                    accessed_at_ms: info.accessed_at_ms,
                    read_only: info.read_only,
                    is_directory: info.is_directory,
                },
                Err(error) => {
                    eprintln!(
                        "[clipboard-worker] failed to store file {}: {error}",
                        source_path.display()
                    );
                    let metadata = std::fs::metadata(source_path).ok();
                    CapturedFileReference {
                        original_path: file_path.clone(),
                        storage_path: file_path.clone(),
                        original_name: source_path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| file_path.clone()),
                        size_bytes: metadata.as_ref().map_or(0, std::fs::Metadata::len),
                        content_hash: None,
                        extension: extension_for_path(source_path),
                        mime_type: mime_type_for_path(source_path),
                        created_at_ms: metadata.as_ref().and_then(created_at_ms),
                        modified_at_ms: metadata.as_ref().and_then(modified_at_ms),
                        accessed_at_ms: metadata.as_ref().and_then(accessed_at_ms),
                        read_only: metadata
                            .as_ref()
                            .is_some_and(|metadata| metadata.permissions().readonly()),
                        is_directory: metadata.as_ref().is_some_and(std::fs::Metadata::is_dir),
                        copied: false,
                    }
                }
            }
        })
        .collect()
}

fn captured_file_metadata(files: &[CapturedFileReference]) -> String {
    let first = files.first();
    serde_json::json!({
        "schemaVersion": RESOURCE_METADATA_SCHEMA_VERSION,
        "mimeType": first.map(|file| file.mime_type.as_str()),
        "extension": if files.len() == 1 {
            first.and_then(|file| file.extension.as_deref())
        } else {
            None
        },
        "sizeBytes": files.iter().map(|file| file.size_bytes).sum::<u64>(),
        "resourcePath": first.map(|file| file.storage_path.as_str()),
        "storagePath": first.map(|file| file.storage_path.as_str()),
        "originalPath": if files.len() == 1 {
            first.map(|file| file.original_path.as_str())
        } else {
            None
        },
        "files": files
            .iter()
            .map(|file| serde_json::json!({
                "name": file.original_name,
                "extension": file.extension,
                "mimeType": file.mime_type,
                "size": file.size_bytes,
                "sizeBytes": file.size_bytes,
                "path": file.storage_path,
                "storagePath": file.storage_path,
                "originalPath": file.original_path,
                "contentHash": file.content_hash,
                "copied": file.copied,
                "createdAtMs": file.created_at_ms,
                "modifiedAtMs": file.modified_at_ms,
                "accessedAtMs": file.accessed_at_ms,
                "readOnly": file.read_only,
                "isDirectory": file.is_directory,
            }))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

fn stop_signal_requested(receiver: &mpsc::Receiver<()>) -> bool {
    matches!(
        receiver.try_recv(),
        Ok(()) | Err(mpsc::TryRecvError::Disconnected)
    )
}

fn wait_for_stop(
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

/// Shared state for self-trigger guard to prevent capturing app's own clipboard writes.
#[derive(Clone)]
struct SelfTriggerState(Arc<Mutex<content::hash::SelfTriggerGuard>>);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageCleanupResult {
    removed_files: u64,
    freed_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveredApplication {
    name: String,
    icon_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationFilterSettings {
    discovered_applications: Vec<String>,
    discovered_applications_with_icons: Vec<DiscoveredApplication>,
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

    let disk_space = platform::disk_space(&paths.data_directory);

    Ok(StorageStatus {
        item_count: database.item_count().map_err(|error| error.to_string())?,
        image_count: database.count_by_kind("image").unwrap_or(0),
        image_size_bytes: database.size_by_kind("image").unwrap_or(0),
        file_count: database.count_by_kind("file").unwrap_or(0),
        file_size_bytes: database.size_by_kind("file").unwrap_or(0),
        text_count: database.count_by_kind("text").unwrap_or(0),
        link_count: database.count_by_kind("link").unwrap_or(0),
        project_path: paths.project.display().to_string(),
        config_path,
        keyboard_config_path,
        data_directory_path: paths.data_directory.display().to_string(),
        uses_custom_data_directory: paths.uses_custom_data_directory(),
        storage_path: paths.storage.display().to_string(),
        icons_dir: paths.storage.join("icons").display().to_string(),
        database_path: paths.database.display().to_string(),
        database_size_bytes: file_or_dir_size(&paths.database),
        files_path: paths.files.display().to_string(),
        image_path: paths.images.display().to_string(),
        image_cleanup_enabled: paths.image_cleanup_enabled,
        file_cleanup_enabled: paths.file_cleanup_enabled,
        search_index_path: paths.search_index.display().to_string(),
        search_index_size_bytes: dir_size(&paths.search_index),
        search_index_version: SEARCH_INDEX_VERSION,
        search_index_rebuild_required: search_index.requires_full_rebuild(),
        disk_total_bytes: disk_space.map(|space| space.total_bytes),
        disk_available_bytes: disk_space.map(|space| space.available_bytes),
    })
}

#[tauri::command]
fn get_storage_kind_stats(
    database: tauri::State<'_, Database>,
    kind: ClipboardKind,
) -> Result<StorageKindStats, String> {
    database
        .kind_storage_stats(kind, STORAGE_KIND_DELETE_SCOPE)
        .map(StorageKindStats::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn configure_storage_directory(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    active_paths: tauri::State<'_, StoragePaths>,
    database: tauri::State<'_, Database>,
    capture: tauri::State<'_, CaptureState>,
    data_directory: Option<String>,
) -> Result<StorageDirectoryUpdate, String> {
    let requested_directory = data_directory.map(PathBuf::from);
    let (image_storage_path, file_storage_path) = {
        let config = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        (
            config.image_storage_path().map(PathBuf::from),
            config.file_storage_path().map(PathBuf::from),
        )
    };
    let target_paths = StoragePaths::initialize_with_resource_directories_for_configuration(
        active_paths.project.clone(),
        requested_directory,
        image_storage_path,
        file_storage_path,
    )
    .map_err(|error| error.to_string())?;

    if target_paths.data_directory != active_paths.data_directory {
        // Pause clipboard capture so no new items are written to the
        // old storage directory during migration.
        capture.set_paused(true);
        migrate_storage_data(&active_paths, &target_paths, &database)?;
    }

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

fn migrate_storage_data(
    old: &StoragePaths,
    new: &StoragePaths,
    database: &Database,
) -> Result<(), String> {
    let dirs_to_migrate: &[(PathBuf, PathBuf, &str)] = &[
        (old.images.clone(), new.images.clone(), "images"),
        (old.files.clone(), new.files.clone(), "files"),
    ];

    for (old_dir, new_dir, label) in dirs_to_migrate {
        if old_dir == new_dir {
            continue;
        }
        if old_dir.exists() {
            copy_dir_contents(old_dir, new_dir)
                .map_err(|e| format!("failed to migrate {}: {}", label, e))?;
        }
    }

    // Search index is not migrated — it will be rebuilt from the migrated
    // database on the next startup. We only ensure the directory exists.
    if old.search_index != new.search_index {
        std::fs::create_dir_all(&new.search_index)
            .map_err(|e| format!("failed to create search index directory: {}", e))?;
    }

    let icons_old = old.storage.join("icons");
    let icons_new = new.storage.join("icons");
    if icons_old != icons_new && icons_old.exists() {
        copy_dir_contents(&icons_old, &icons_new)
            .map_err(|e| format!("failed to migrate icons: {}", e))?;
    }

    if old.database != new.database && old.database.exists() {
        database
            .vacuum_into(&new.database)
            .map_err(|e| format!("failed to migrate database: {}", e))?;
        let migrated_database = Database::open(&new.database)
            .map_err(|e| format!("failed to open migrated database: {e}"))?;
        rewrite_database_storage_paths(&migrated_database, &storage_path_mappings(old, new))
            .map_err(|e| format!("failed to update migrated resource paths: {e}"))?;
    }

    Ok(())
}

fn storage_path_mappings(old: &StoragePaths, new: &StoragePaths) -> Vec<(PathBuf, PathBuf)> {
    let mut mappings = vec![
        (old.previews.clone(), new.previews.clone()),
        (old.images.clone(), new.images.clone()),
        (old.files.clone(), new.files.clone()),
        (old.storage.join("icons"), new.storage.join("icons")),
        (old.storage.clone(), new.storage.clone()),
    ];
    mappings.retain(|(from, to)| from != to);
    mappings.sort_by_key(|(from, _)| std::cmp::Reverse(from.components().count()));
    mappings
}

fn rewrite_database_storage_paths(
    database: &Database,
    mappings: &[(PathBuf, PathBuf)],
) -> Result<u64, storage::StorageError> {
    database.with_connection(|connection| {
        let transaction = connection.transaction()?;
        let records = {
            let mut statement = transaction.prepare(
                "SELECT id, kind, text_content, resource_path, preview_path, icon_path, metadata_json
                 FROM clipboard_items",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut updated = 0u64;
        for (id, kind, text_content, resource_path, preview_path, icon_path, metadata_json) in
            records
        {
            let rewritten_resource = rewrite_optional_storage_path(resource_path.as_deref(), mappings);
            let rewritten_preview = rewrite_optional_storage_path(preview_path.as_deref(), mappings);
            let rewritten_icon = rewrite_optional_storage_path(icon_path.as_deref(), mappings);
            let rewritten_text = if kind == "file" {
                rewrite_json_storage_paths(text_content.as_deref(), mappings)
            } else {
                text_content.clone()
            };
            let rewritten_metadata = rewrite_json_storage_paths(metadata_json.as_deref(), mappings);

            if rewritten_resource == resource_path
                && rewritten_preview == preview_path
                && rewritten_icon == icon_path
                && rewritten_text == text_content
                && rewritten_metadata == metadata_json
            {
                continue;
            }

            transaction.execute(
                "UPDATE clipboard_items
                 SET text_content = ?2,
                     resource_path = ?3,
                     preview_path = ?4,
                     icon_path = ?5,
                     metadata_json = ?6
                 WHERE id = ?1",
                params![
                    id,
                    rewritten_text,
                    rewritten_resource,
                    rewritten_preview,
                    rewritten_icon,
                    rewritten_metadata,
                ],
            )?;
            updated = updated.saturating_add(1);
        }

        transaction.commit()?;
        Ok(updated)
    })
}

fn rewrite_optional_storage_path(
    value: Option<&str>,
    mappings: &[(PathBuf, PathBuf)],
) -> Option<String> {
    value.map(|value| rewrite_storage_path(value, mappings))
}

fn rewrite_storage_path(value: &str, mappings: &[(PathBuf, PathBuf)]) -> String {
    let path = Path::new(value);
    for (from, to) in mappings {
        if let Ok(relative) = path.strip_prefix(from) {
            return to.join(relative).to_string_lossy().into_owned();
        }
    }
    value.to_owned()
}

fn rewrite_json_storage_paths(
    value: Option<&str>,
    mappings: &[(PathBuf, PathBuf)],
) -> Option<String> {
    let value = value?;
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(value) else {
        return Some(value.to_owned());
    };
    let changed = rewrite_json_value_paths(&mut json, mappings);
    if changed {
        serde_json::to_string(&json)
            .ok()
            .or_else(|| Some(value.to_owned()))
    } else {
        Some(value.to_owned())
    }
}

fn rewrite_json_value_paths(
    value: &mut serde_json::Value,
    mappings: &[(PathBuf, PathBuf)],
) -> bool {
    match value {
        serde_json::Value::String(path) => {
            let rewritten = rewrite_storage_path(path, mappings);
            if rewritten == *path {
                false
            } else {
                *path = rewritten;
                true
            }
        }
        serde_json::Value::Array(values) => {
            let mut changed = false;
            for value in values {
                changed |= rewrite_json_value_paths(value, mappings);
            }
            changed
        }
        serde_json::Value::Object(values) => {
            let mut changed = false;
            for value in values.values_mut() {
                changed |= rewrite_json_value_paths(value, mappings);
            }
            changed
        }
        _ => false,
    }
}

fn copy_dir_contents(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("create dir: {}", e))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("read dir: {}", e))? {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let dest = to.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|e| format!("read file type for {}: {e}", entry.path().display()))?
            .is_dir()
        {
            copy_dir_contents(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest).map_err(|e| {
                format!("copy {} to {}: {e}", entry.path().display(), dest.display())
            })?;
        }
    }
    Ok(())
}

fn file_or_dir_size(path: &PathBuf) -> u64 {
    if path.is_file() {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let wal = path.with_extension("sqlite3-wal");
        let shm = path.with_extension("sqlite3-shm");
        let wal_size = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        let shm_size = std::fs::metadata(&shm).map(|m| m.len()).unwrap_or(0);
        return size + wal_size + shm_size;
    }
    dir_size(path)
}

fn dir_size(path: &PathBuf) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                total += dir_size(&entry.path());
            } else {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

#[tauri::command]
fn get_application_filter_settings(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<ApplicationFilterSettings, String> {
    let discovered_applications = database
        .list_source_applications()
        .map_err(|error| error.to_string())?;
    let discovered_with_icons = database
        .list_source_applications_with_icons()
        .map_err(|error| error.to_string())?;
    let ignored_applications = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .ignored_applications()
        .to_vec();

    Ok(ApplicationFilterSettings {
        discovered_applications,
        discovered_applications_with_icons: discovered_with_icons
            .into_iter()
            .map(|(name, icon_path)| DiscoveredApplication { name, icon_path })
            .collect(),
        ignored_applications,
    })
}

#[tauri::command]
fn configure_ignored_applications(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
    applications: Vec<String>,
) -> Result<Vec<String>, String> {
    apply_ignored_applications(
        config.inner(),
        monitor.inner(),
        capture.inner(),
        applications,
    )
}

fn apply_ignored_applications(
    config: &Mutex<ConfigStore>,
    monitor: &Mutex<ClipboardMonitor>,
    capture: &CaptureState,
    applications: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut monitor = monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?;
    let normalized = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_ignored_applications(applications)
        .map_err(|error| error.to_string())?;
    let normalized = capture.set_ignored_apps(normalized);
    monitor.set_ignored_apps(normalized.clone());
    Ok(normalized)
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

/// Update the favorite flag for a selected group of records in one
/// transaction. The repository validates every id before changing anything,
/// so a stale selection cannot leave the UI and database out of sync.
#[tauri::command]
fn batch_set_favorite(
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
    is_favorite: bool,
) -> Result<bool, String> {
    database
        .set_favorite_batch(&ids, is_favorite)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_clipboard_item(database: tauri::State<'_, Database>, id: String) -> Result<bool, String> {
    database.delete_item(&id).map_err(|error| error.to_string())
}

/// Soft-delete a selected group of records. Favorite protection and error
/// wording intentionally match `soft_delete_clipboard_item`.
#[tauri::command]
fn batch_delete_clipboard_items(
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    database
        .soft_delete_batch(&ids)
        .map_err(|error| error.to_string())
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
fn regenerate_clipboard_item_ocr(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database
        .regenerate_ocr(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_source_applications(database: tauri::State<'_, Database>) -> Result<Vec<String>, String> {
    database
        .list_source_applications()
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
    hotkey_manager: tauri::State<'_, Mutex<HotkeyManager>>,
    action: String,
    shortcuts: Vec<String>,
) -> Result<Vec<String>, String> {
    let normalized = keyboard
        .lock()
        .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
        .set_action_shortcuts(action.clone(), shortcuts)
        .map_err(|error| error.to_string())?;

    if action == TOGGLE_WINDOW_ACTION {
        let config = keyboard
            .lock()
            .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
            .config();
        let (bindings, double_modifiers) = resolve_toggle_hotkeys(&config);
        let mut hm = hotkey_manager
            .lock()
            .map_err(|_| "hotkey manager lock is poisoned".to_owned())?;
        if bindings.is_empty() && double_modifiers.is_empty() {
            hm.stop();
        } else {
            hm.restart_with_hotkeys(bindings, double_modifiers);
        }
    }

    Ok(normalized)
}

#[tauri::command]
fn paste_to_previous_application(
    app: tauri::AppHandle,
    hotkey_manager: tauri::State<'_, Mutex<HotkeyManager>>,
) -> Result<bool, String> {
    let target = hotkey_manager
        .lock()
        .map_err(|_| "hotkey manager lock is poisoned".to_owned())?
        .take_quick_paste_target();
    let Some(target) = target else {
        return Ok(false);
    };

    let main_window = app.get_webview_window("main");
    if let Some(window) = &main_window {
        window.hide().map_err(|error| error.to_string())?;
    }
    thread::sleep(Duration::from_millis(40));

    if let Err(error) = platform::windows_hotkey::restore_window_and_paste(target) {
        if let Some(window) = &main_window {
            let _ = window.show();
            let _ = window.set_focus();
        }
        return Err(error);
    }

    Ok(true)
}

#[tauri::command]
fn search_clipboard_items(
    database: tauri::State<'_, Database>,
    search_index: tauri::State<'_, SearchIndex>,
    performance_tracker: tauri::State<'_, PerformanceTracker>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ClipboardItem>, String> {
    let started = Instant::now();
    SearchSynchronizer::default()
        .sync_until_idle(database.inner(), search_index.inner())
        .map_err(|error| error.to_string())?;
    let hits = search_index
        .search(&query, limit.unwrap_or(100).clamp(1, 500))
        .map_err(|error| error.to_string())?;
    let item_ids = hits.into_iter().map(|hit| hit.item_id).collect::<Vec<_>>();

    let items = database
        .get_items_by_ids(&item_ids)
        .map_err(|error| error.to_string())?;
    performance_tracker.record_search(
        &query,
        started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        items.len(),
    );
    Ok(items)
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
fn get_clipboard_formats(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<ClipboardFormatInfo, String> {
    let item = database
        .get_item(&id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("clipboard item not found: {id}"))?;

    Ok(content::clipboard_format_info_from_metadata(
        item.metadata_json.as_deref(),
    ))
}

#[tauri::command]
fn get_ocr_status(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<OcrStatusInfo, String> {
    let pending = database.count_pending_ocr().map_err(|e| e.to_string())?;
    let completed = database.count_completed_ocr().map_err(|e| e.to_string())?;
    let failed = database.count_failed_ocr().map_err(|e| e.to_string())?;
    let cfg = config
        .lock()
        .map_err(|_| "config lock poisoned".to_owned())?;
    let engine = cfg.ocr_engine().to_string();
    let models_dir = ocr::models::models_dir(&paths.storage);
    let model = configured_ppocr_model(&cfg);
    let installed_variants = ocr::models::installed_model_variants(&models_dir);
    let ppocr_available = ocr::models::model_is_installed(&models_dir, model);
    let tesseract_available = TesseractOcrEngine::is_available();
    let engine_available = match engine.as_str() {
        "ppocr" => ppocr_available,
        "tesseract" => tesseract_available,
        _ => false,
    };

    Ok(OcrStatusInfo {
        total_tasks: pending.saturating_add(completed).saturating_add(failed),
        pending_tasks: pending,
        completed_tasks: completed,
        failed_tasks: failed,
        tesseract_available,
        ppocr_available,
        has_engine: tesseract_available || !installed_variants.is_empty(),
        engine_available,
        engine,
        ppocr_model_variant: model.id.to_owned(),
        installed_variants,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrStatusInfo {
    total_tasks: u64,
    pending_tasks: u64,
    completed_tasks: u64,
    failed_tasks: u64,
    tesseract_available: bool,
    ppocr_available: bool,
    has_engine: bool,
    engine_available: bool,
    engine: String,
    ppocr_model_variant: String,
    installed_variants: Vec<&'static str>,
}

fn configured_ppocr_model(config: &ConfigStore) -> &'static ocr::models::PpOcrModelSpec {
    ocr::models::model_spec(config.ppocr_model_variant()).unwrap_or_else(|| {
        eprintln!(
            "[ocr] unsupported configured PP-OCR model variant '{}', using small",
            config.ppocr_model_variant()
        );
        ocr::models::default_model_spec()
    })
}

fn ocr_config_response(config: &ConfigStore) -> OcrConfigResponse {
    OcrConfigResponse {
        engine: config.ocr_engine().to_string(),
        tesseract_languages: config.tesseract_languages().to_string(),
        ppocr_model_variant: configured_ppocr_model(config).id.to_owned(),
        det_score_threshold: config.det_score_threshold(),
        det_box_threshold: config.det_box_threshold(),
        det_unclip_ratio: config.det_unclip_ratio(),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct OcrConfigUpdate {
    engine: Option<String>,
    ppocr_model_variant: Option<String>,
    det_score_threshold: Option<f32>,
    det_box_threshold: Option<f32>,
    det_unclip_ratio: Option<f32>,
}

fn apply_ocr_runtime_settings(
    config: &Mutex<ConfigStore>,
    paths: &StoragePaths,
    worker: &OcrWorkerManager,
    update: OcrConfigUpdate,
) -> Result<OcrConfigResponse, String> {
    let mut cfg = config
        .lock()
        .map_err(|_| "config lock poisoned".to_owned())?;
    let engine = update.engine.unwrap_or_else(|| cfg.ocr_engine().to_owned());
    let model = match update.ppocr_model_variant {
        Some(variant) => ocr::models::model_spec(&variant)
            .ok_or_else(|| format!("unsupported PP-OCR model variant: {variant}"))?,
        None => configured_ppocr_model(&cfg),
    };
    let score_threshold = update
        .det_score_threshold
        .unwrap_or_else(|| cfg.det_score_threshold());
    let box_threshold = update
        .det_box_threshold
        .unwrap_or_else(|| cfg.det_box_threshold());
    let unclip_ratio = update
        .det_unclip_ratio
        .unwrap_or_else(|| cfg.det_unclip_ratio());

    let runtime_engine: Arc<dyn OcrEngine> = match engine.as_str() {
        "ppocr" => {
            let ppocr = PpOcrEngine::new(
                ocr::models::models_dir(&paths.storage),
                model,
                score_threshold,
                box_threshold,
                unclip_ratio,
            );
            if !ppocr.is_available() {
                return Err(format!("PP-OCR {} model files are not installed", model.id));
            }
            Arc::new(ppocr)
        }
        "tesseract" if TesseractOcrEngine::is_available() => Arc::new(
            TesseractOcrEngine::with_languages(cfg.tesseract_languages().to_owned()),
        ),
        "tesseract" => return Err("Tesseract is not available".to_owned()),
        _ => return Err(format!("unsupported OCR engine: {engine}")),
    };
    let database = Database::open(&paths.database).map_err(|e| e.to_string())?;

    cfg.set_ocr_settings(
        engine,
        model.id.to_owned(),
        score_threshold,
        box_threshold,
        unclip_ratio,
    )
    .map_err(|e| e.to_string())?;
    worker.restart(runtime_engine, Arc::new(database));

    Ok(ocr_config_response(&cfg))
}

#[tauri::command]
fn get_ocr_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<OcrConfigResponse, String> {
    let config = config
        .lock()
        .map_err(|_| "config lock poisoned".to_owned())?;
    Ok(ocr_config_response(&config))
}

#[tauri::command]
fn set_ocr_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
    worker: tauri::State<'_, OcrWorkerManager>,
    settings: OcrConfigUpdate,
) -> Result<OcrConfigResponse, String> {
    if settings.engine.is_none() {
        return Err("OCR engine is required".to_owned());
    }
    apply_ocr_runtime_settings(&config, &paths, &worker, settings)
}

#[tauri::command]
fn restart_ocr_engine(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
    worker: tauri::State<'_, OcrWorkerManager>,
) -> Result<(), String> {
    apply_ocr_runtime_settings(&config, &paths, &worker, OcrConfigUpdate::default())?;
    Ok(())
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PpOcrDownloadProgress {
    filename: String,
    label: String,
    current: u64,
    total: u64,
    percentage: f64,
}

async fn download_ppocr_file(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    models_dir: &Path,
    model_file: ocr::models::PpOcrModelFile,
) -> Result<(), String> {
    if ocr::models::model_file_is_installed(models_dir, &model_file) {
        return Ok(());
    }

    let destination = models_dir.join(model_file.filename);
    let temporary = models_dir.join(format!("{}.part", model_file.filename));
    if temporary.exists() {
        std::fs::remove_file(&temporary)
            .map_err(|e| format!("remove stale {}: {e}", temporary.display()))?;
    }

    let _ = app.emit(
        "ppocr-download-progress",
        PpOcrDownloadProgress {
            filename: model_file.filename.to_owned(),
            label: model_file.label.to_owned(),
            current: 0,
            total: model_file.size_bytes,
            percentage: 0.0,
        },
    );

    let mut response = client
        .get(model_file.url)
        .send()
        .await
        .map_err(|e| format!("download {}: {e}", model_file.filename))?
        .error_for_status()
        .map_err(|e| format!("download {}: {e}", model_file.filename))?;
    let total = response.content_length().unwrap_or(model_file.size_bytes);
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    let mut downloaded = 0u64;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("download {}: {e}", model_file.filename))?
    {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        let percentage = if total > 0 {
            (downloaded as f64 / total as f64) * 100.0
        } else {
            -1.0
        };
        let _ = app.emit(
            "ppocr-download-progress",
            PpOcrDownloadProgress {
                filename: model_file.filename.to_owned(),
                label: model_file.label.to_owned(),
                current: downloaded,
                total,
                percentage,
            },
        );
    }

    file.flush().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);

    if downloaded != model_file.size_bytes {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "downloaded {} has unexpected size: expected {}, got {}",
            model_file.filename, model_file.size_bytes, downloaded
        ));
    }
    if destination.exists() {
        std::fs::remove_file(&destination)
            .map_err(|e| format!("replace {}: {e}", destination.display()))?;
    }
    std::fs::rename(&temporary, &destination)
        .map_err(|e| format!("install {}: {e}", model_file.filename))?;

    Ok(())
}

#[tauri::command]
async fn install_ppocr(
    app: tauri::AppHandle,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    worker: tauri::State<'_, OcrWorkerManager>,
    variant: String,
) -> Result<String, String> {
    let model = ocr::models::model_spec(&variant)
        .ok_or_else(|| format!("unsupported PP-OCR model variant: {variant}"))?;
    let models_dir = ocr::models::models_dir(&paths.storage);
    std::fs::create_dir_all(&models_dir).map_err(|e| e.to_string())?;

    let client = reqwest::Client::builder()
        .user_agent("clipboard-desktop")
        .build()
        .map_err(|e| format!("create client: {e}"))?;

    for model_file in model.files() {
        download_ppocr_file(&app, &client, &models_dir, model_file).await?;
    }

    apply_ocr_runtime_settings(
        &config,
        &paths,
        &worker,
        OcrConfigUpdate {
            engine: Some("ppocr".to_owned()),
            ppocr_model_variant: Some(model.id.to_owned()),
            ..Default::default()
        },
    )?;

    Ok(format!(
        "PP-OCRv6 {} model installed and activated",
        model.id
    ))
}

#[tauri::command]
fn check_ppocr_status(
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<PpOcrStatus, String> {
    let models_dir = ocr::models::models_dir(&paths.storage);
    let config = config
        .lock()
        .map_err(|_| "config lock poisoned".to_owned())?;
    let active_model = configured_ppocr_model(&config);
    Ok(PpOcrStatus {
        available: ocr::models::model_is_installed(&models_dir, active_model),
        tesseract_available: TesseractOcrEngine::is_available(),
        active_variant: active_model.id.to_owned(),
        installed_variants: ocr::models::installed_model_variants(&models_dir),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PpOcrStatus {
    available: bool,
    tesseract_available: bool,
    active_variant: String,
    installed_variants: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneralSettingsInfo {
    settings: GeneralConfig,
    legacy_migration_required: bool,
}

#[tauri::command]
fn get_general_settings(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<GeneralSettingsInfo, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    Ok(GeneralSettingsInfo {
        settings: config.general_settings().clone(),
        legacy_migration_required: !config.has_general_settings(),
    })
}

#[tauri::command]
fn set_general_settings(
    app: tauri::AppHandle,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    settings: GeneralConfig,
) -> Result<GeneralConfig, String> {
    let saved = {
        let mut config = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        config
            .set_general_settings(settings)
            .map_err(|error| error.to_string())?;
        config.general_settings().clone()
    };

    // The config write is authoritative; a failed notification should not turn
    // a successful persistence operation into a user-visible failure.
    let _ = app.emit("general-settings-changed", &saved);
    apply_window_transparency_to_main(&app, saved.window_transparency);
    Ok(saved)
}

/// Best-effort application of the configured transparency to the main
/// window; the persisted setting stays authoritative even when the native
/// call fails (e.g. on unsupported platforms).
fn apply_window_transparency_to_main(app: &tauri::AppHandle, percent: u8) {
    #[cfg(target_os = "windows")]
    {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        match window.hwnd() {
            Ok(hwnd) => {
                if let Err(error) = platform::apply_window_transparency(hwnd.0 as isize, percent) {
                    eprintln!("[window] failed to apply transparency: {error}");
                }
            }
            Err(error) => {
                eprintln!("[window] failed to resolve the main window handle: {error}");
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, percent);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryConfigInfo {
    max_items: u32,
    retention_days: u32,
    recycle_bin_days: u32,
}

#[tauri::command]
fn get_history_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<HistoryConfigInfo, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    Ok(HistoryConfigInfo {
        max_items: config.max_items(),
        retention_days: config.retention_days(),
        recycle_bin_days: config.recycle_bin_days(),
    })
}

#[tauri::command]
fn set_history_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    max_items: Option<u32>,
    retention_days: Option<u32>,
    recycle_bin_days: Option<u32>,
) -> Result<(), String> {
    let mut config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    if let Some(v) = max_items {
        config.set_max_items(v).map_err(|e| e.to_string())?;
    }
    if let Some(v) = retention_days {
        config.set_retention_days(v).map_err(|e| e.to_string())?;
    }
    if let Some(v) = recycle_bin_days {
        config.set_recycle_bin_days(v).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageConfigInfo {
    max_file_copy_size_bytes: u64,
    max_screenshot_size_bytes: u64,
    image_storage_path: Option<String>,
    file_storage_path: Option<String>,
}

#[tauri::command]
fn get_storage_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<StorageConfigInfo, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    Ok(StorageConfigInfo {
        max_file_copy_size_bytes: config.max_file_copy_size_bytes(),
        max_screenshot_size_bytes: config.max_screenshot_size_bytes(),
        image_storage_path: config
            .image_storage_path()
            .map(|path| path.display().to_string()),
        file_storage_path: config
            .file_storage_path()
            .map(|path| path.display().to_string()),
    })
}

#[tauri::command]
fn set_storage_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    capture: tauri::State<'_, CaptureState>,
    max_file_copy_size_bytes: Option<u64>,
) -> Result<(), String> {
    let mut config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    if let Some(v) = max_file_copy_size_bytes {
        config
            .set_max_file_copy_size_bytes(v)
            .map_err(|e| e.to_string())?;
        capture.set_max_file_copy_size_bytes(v);
    }
    Ok(())
}

#[tauri::command]
fn set_resource_storage_paths(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    active_paths: tauri::State<'_, StoragePaths>,
    image_storage_path: Option<String>,
    file_storage_path: Option<String>,
) -> Result<ResourceStorageUpdate, String> {
    let image_storage_path = image_storage_path.and_then(|path| {
        let path = path.trim().to_owned();
        (!path.is_empty()).then(|| PathBuf::from(path))
    });
    let file_storage_path = file_storage_path.and_then(|path| {
        let path = path.trim().to_owned();
        (!path.is_empty()).then(|| PathBuf::from(path))
    });

    let target_paths = StoragePaths::initialize_with_resource_directories_for_configuration(
        active_paths.project.clone(),
        Some(active_paths.data_directory.clone()),
        image_storage_path.clone(),
        file_storage_path.clone(),
    )
    .map_err(|error| error.to_string())?;

    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_resource_storage_paths(image_storage_path, file_storage_path)
        .map_err(|error| error.to_string())?;

    Ok(ResourceStorageUpdate {
        image_storage_path: target_paths.images.display().to_string(),
        file_storage_path: target_paths.files.display().to_string(),
        restart_required: target_paths.images != active_paths.images
            || target_paths.files != active_paths.files,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrConfigResponse {
    engine: String,
    tesseract_languages: String,
    det_score_threshold: f32,
    det_box_threshold: f32,
    det_unclip_ratio: f32,
    ppocr_model_variant: String,
}

#[tauri::command]
fn copy_file_to(src: String, dst: String) -> Result<(), String> {
    std::fs::copy(&src, &dst)
        .map(|_| ())
        .map_err(|e| format!("copy failed: {e}"))
}

fn generated_clipboard_title(text: &str) -> String {
    text.chars().take(200).collect()
}

fn metadata_custom_title(metadata_json: Option<&str>) -> Option<bool> {
    let value =
        metadata_json.and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())?;
    value
        .get("customTitle")
        .and_then(serde_json::Value::as_bool)
}

fn resolve_custom_title(title: &str, text_content: &str, metadata_json: Option<&str>) -> bool {
    metadata_custom_title(metadata_json)
        .unwrap_or_else(|| title != generated_clipboard_title(text_content))
}

fn set_custom_title_metadata(
    metadata_json: Option<&str>,
    custom_title: bool,
) -> Result<String, String> {
    let mut value = metadata_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if !value.is_object() {
        value = serde_json::json!({});
    }
    value
        .as_object_mut()
        .expect("custom-title metadata must be an object")
        .insert(
            "customTitle".to_owned(),
            serde_json::Value::Bool(custom_title),
        );
    serde_json::to_string(&value)
        .map_err(|error| format!("serialize custom title metadata: {error}"))
}

#[tauri::command]
fn rename_item(
    database: tauri::State<'_, Database>,
    id: String,
    new_name: String,
) -> Result<ClipboardItem, String> {
    let items = database
        .get_items_by_ids(std::slice::from_ref(&id))
        .map_err(|e| e.to_string())?;
    let item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    if new_name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let mut updated = item.clone();
    if item.kind == ClipboardKind::Image || item.kind == ClipboardKind::File {
        if let Some(ref old_path) = item.resource_path {
            let old = std::path::Path::new(old_path);
            if old.exists() {
                let ext = old.extension().unwrap_or_default().to_string_lossy();
                let parent = old.parent().unwrap_or(std::path::Path::new("."));
                let new_path = parent.join(format!("{}.{}", new_name.trim(), ext));
                if new_path != old {
                    if new_path.exists() {
                        return Err(format!("file already exists: {}", new_path.display()));
                    }
                    std::fs::rename(old, &new_path).map_err(|e| format!("rename failed: {e}"))?;
                }
                updated.resource_path = Some(new_path.to_string_lossy().to_string());
                updated.preview_path = Some(new_path.to_string_lossy().to_string());
            }
        }
    }
    updated.title = new_name.trim().to_string();
    if matches!(updated.kind, ClipboardKind::Text | ClipboardKind::Link) {
        updated.metadata_json = Some(set_custom_title_metadata(
            updated.metadata_json.as_deref(),
            true,
        )?);
    }
    database.save_item(&updated).map_err(|e| e.to_string())?;
    Ok(updated)
}

#[tauri::command]
fn update_clipboard_text(
    database: tauri::State<'_, Database>,
    id: String,
    new_title: String,
    new_text_content: String,
) -> Result<bool, String> {
    if new_text_content.trim().is_empty() {
        return Err("text content cannot be empty".to_owned());
    }

    let items = database
        .get_items_by_ids(std::slice::from_ref(&id))
        .map_err(|e| e.to_string())?;
    let item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    if !matches!(item.kind, ClipboardKind::Text | ClipboardKind::Link) {
        return Err("only text and link items can be edited".to_owned());
    }

    let kind_name = match item.kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image | ClipboardKind::File => unreachable!(),
    };
    let custom_title =
        resolve_custom_title(&new_title, &new_text_content, item.metadata_json.as_deref());
    let metadata_json = set_custom_title_metadata(item.metadata_json.as_deref(), custom_title)?;
    let content_hash = content::hash::compute_content_hash(kind_name, &new_text_content, None);
    let size_bytes = new_text_content.len() as u64;

    database
        .update_text_item(&TextItemUpdate {
            id: &id,
            kind: item.kind,
            title: &new_title,
            text_content: &new_text_content,
            content_hash: &content_hash,
            size_bytes,
            metadata_json: Some(&metadata_json),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| format!("failed to open URL: {e}"))
}

#[tauri::command]
fn reveal_in_explorer(path: String) -> Result<(), String> {
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
    database.soft_delete(&id).map_err(|error| error.to_string())
}

#[tauri::command]
fn clear_all_non_favorite_items(database: tauri::State<'_, Database>) -> Result<u64, String> {
    database
        .clear_all_non_favorite_items()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn restore_clipboard_item(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database
        .restore_deleted(&id)
        .map_err(|error| error.to_string())
}

/// List soft-deleted records for the recycle-bin view.  The repository keeps
/// the same bounded pagination contract as the active history endpoint.
#[tauri::command]
fn list_deleted_clipboard_items(
    database: tauri::State<'_, Database>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ClipboardItem>, String> {
    database
        .list_deleted(limit.unwrap_or(100), offset.unwrap_or(0))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn batch_restore_clipboard_items(
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    database
        .restore_deleted_batch(&ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn permanently_delete_clipboard_item(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database
        .permanently_delete(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn batch_permanently_delete_clipboard_items(
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    database
        .permanently_delete_batch(&ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn permanently_delete_storage_kind(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    search_index: tauri::State<'_, SearchIndex>,
    capture: tauri::State<'_, CaptureState>,
    app: tauri::AppHandle,
    kind: ClipboardKind,
    expected: StorageKindDeleteExpectation,
) -> Result<StorageKindDeleteResult, String> {
    let ingestion_guard = capture
        .ingestion_guard
        .lock()
        .map_err(|_| "clipboard ingestion lock is poisoned".to_owned())?;
    let expected = KindStorageStats {
        item_count: expected.item_count,
        size_bytes: expected.size_bytes,
    };
    let mut result = permanently_delete_storage_kind_for(
        database.inner(),
        paths.inner(),
        search_index.inner(),
        kind,
        Some(expected),
    )?;
    drop(ingestion_guard);
    if result.deleted_count > 0 {
        if let Err(error) = app.emit(
            "clipboard-history-invalidated",
            ClipboardHistoryInvalidated {
                deleted_ids: result.deleted_ids.clone(),
            },
        ) {
            result
                .warnings
                .push(format!("main window refresh is pending: {error}"));
        }
    }
    Ok(result)
}

fn permanently_delete_storage_kind_for(
    database: &Database,
    paths: &StoragePaths,
    search_index: &SearchIndex,
    kind: ClipboardKind,
    expected: Option<KindStorageStats>,
) -> Result<StorageKindDeleteResult, String> {
    let KindDeleteResult { stats, deleted_ids } = match expected {
        Some(expected) => database
            .permanently_delete_by_kind_if_stats_match(kind, STORAGE_KIND_DELETE_SCOPE, expected)
            .map_err(|error| error.to_string())?,
        None => database
            .permanently_delete_by_kind(kind, STORAGE_KIND_DELETE_SCOPE)
            .map_err(|error| error.to_string())?,
    };
    let mut warnings = Vec::new();
    let search_sync = match SearchSynchronizer::default().sync_until_idle(database, search_index) {
        Ok(summary) => Some(summary),
        Err(error) => {
            warnings.push(format!("search index cleanup is pending: {error}"));
            None
        }
    };
    let removed_files = match cleanup_orphan_storage_files(database, paths) {
        Ok(cleanup) => cleanup.removed_files,
        Err(error) => {
            warnings.push(format!("managed resource cleanup is pending: {error}"));
            0
        }
    };

    Ok(StorageKindDeleteResult {
        deleted_count: stats.item_count,
        deleted_size_bytes: stats.size_bytes,
        removed_files,
        search_sync,
        warnings,
        deleted_ids,
    })
}

#[tauri::command]
fn duplicate_clipboard_item(
    database: tauri::State<'_, Database>,
    app: tauri::AppHandle,
    id: String,
) -> Result<String, String> {
    let items = database
        .get_items_by_ids(std::slice::from_ref(&id))
        .map_err(|e| e.to_string())?;
    let mut item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    item.id = format!("{}-{}", item.content_hash, now_ms);
    item.content_hash = format!("{}-{}", item.content_hash, now_ms);
    item.created_at_ms = now_ms;
    item.last_used_at_ms = None;
    item.is_favorite = false;
    database.save_item(&item).map_err(|e| e.to_string())?;
    let _ = app.emit("clipboard-item-added", &item);
    Ok(item.id.clone())
}

#[tauri::command]
fn enforce_history_cleanup(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<u64, String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    enforce_history_cleanup_for(&database, &guard, &paths, Duration::ZERO)
}

fn enforce_history_cleanup_for(
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

fn cleanup_orphan_storage_files(
    database: &Database,
    paths: &StoragePaths,
) -> Result<StorageCleanupResult, String> {
    cleanup_orphan_storage_files_with_grace(database, paths, Duration::ZERO)
}

fn cleanup_orphan_storage_files_with_grace(
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
                    .map(|base| normalized_cleanup_path(&base.join(path))),
            );
        }
    }
}

fn normalized_cleanup_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[tauri::command]
fn cleanup_storage_files(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<StorageCleanupResult, String> {
    cleanup_orphan_storage_files(&database, &paths)
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
    capture: tauri::State<'_, CaptureState>,
) -> Result<bool, String> {
    let paused = {
        let mut privacy = privacy
            .lock()
            .map_err(|_| "privacy manager lock is poisoned".to_owned())?;
        privacy.toggle_pause();
        privacy.is_paused()
    };
    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_privacy_paused(paused)
        .map_err(|e| e.to_string())?;

    capture.set_paused(paused);

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

    export_database(database.inner(), &options)
}

#[tauri::command]
fn import_clipboard_items(
    database: tauri::State<'_, Database>,
    json: String,
    format: Option<String>,
) -> Result<ImportSummary, String> {
    let normalized = format
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| {
            if json.trim_start().starts_with('[') {
                "json".to_owned()
            } else {
                "plaintext".to_owned()
            }
        });
    match normalized.as_str() {
        "json" => import_from_json(&json, database.inner()),
        "text" | "txt" | "plain" | "plaintext" | "plain-text" => {
            import_from_plain_text(&json, database.inner())
        }
        other => Err(format!("unknown import format: {other}")),
    }
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
    capture: tauri::State<'_, CaptureState>,
    self_trigger: tauri::State<'_, SelfTriggerState>,
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
    let self_trigger_clone = self_trigger.0.clone();
    let capture_for_thread = capture.inner().clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_for_thread = Arc::clone(&stop_flag);
    let (stop_sender, stop_receiver) = mpsc::channel();

    let handle = thread::Builder::new()
        .name("clipboard-capture".to_owned())
        .spawn(move || {
            let database = match Database::open(&db_path) {
                Ok(db) => Arc::new(db),
                Err(e) => {
                    eprintln!("[clipboard-worker] failed to open database: {e}");
                    return;
                }
            };

            let self_trigger_guard = self_trigger_clone;
            let mut consecutive_errors = 0u32;

            loop {
                if stop_flag_for_thread.load(Ordering::SeqCst)
                    || stop_signal_requested(&stop_receiver)
                {
                    break;
                }
                match receiver.recv_timeout(Duration::from_millis(500)) {
                    Ok(change) => {
                        let _ingestion_guard = capture_for_thread
                            .ingestion_guard
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let clipboard_formats = change.formats;
                        if stop_flag_for_thread.load(Ordering::SeqCst)
                            || stop_signal_requested(&stop_receiver)
                        {
                            break;
                        }

                        let app_info = platform::windows_clipboard::get_foreground_app();
                        let source_app = foreground_app_name(&app_info);
                        if capture_for_thread.should_skip(source_app.as_deref(), None) {
                            continue;
                        }

                        let text = match platform::windows_clipboard::read_clipboard_text() {
                            Some(t) => t,
                            None => continue,
                        };

                        if text.is_empty() || text.len() > 500_000 {
                            continue;
                        }

                        if capture_for_thread.should_skip(source_app.as_deref(), Some(&text)) {
                            continue;
                        }

                        let markers = content::detect_markers(&text);
                        let kind = if markers.is_link || markers.has_url {
                            ClipboardKind::Link
                        } else {
                            ClipboardKind::Text
                        };

                        let content_hash = content::hash::compute_content_hash(
                            if kind == ClipboardKind::Link {
                                "link"
                            } else {
                                "text"
                            },
                            &text,
                            None,
                        );

                        if should_skip_self_triggered_text(
                            &mut self_trigger_guard.lock().unwrap(),
                            kind,
                            &text,
                        ) {
                            continue;
                        }

                        let title = text.chars().take(200).collect::<String>();
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
                            source_app: source_app.clone(),
                            icon_path: None,
                            size_bytes,
                            created_at_ms: now_ms,
                            last_used_at_ms: None,
                            is_favorite: false,
                            metadata_json: content::merge_clipboard_format_metadata(
                                None,
                                &clipboard_formats,
                            )
                            .unwrap_or(None),
                        };

                        if stop_flag_for_thread.load(Ordering::SeqCst)
                            || stop_signal_requested(&stop_receiver)
                        {
                            break;
                        }

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
                                    if wait_for_stop(
                                        &stop_receiver,
                                        &stop_flag_for_thread,
                                        Duration::from_secs(5),
                                    ) {
                                        break;
                                    }
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
        })
        .map_err(|error| format!("failed to start clipboard worker: {error}"))?;

    capture.install_worker(CaptureWorker {
        stop_flag,
        stop_sender: Some(stop_sender),
        handle: Some(handle),
    });

    Ok(true)
}

#[tauri::command]
fn stop_clipboard_monitoring(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
) -> Result<bool, String> {
    monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?
        .stop()?;
    capture.stop_worker();
    Ok(true)
}

#[tauri::command]
fn get_clipboard_monitor_status(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
) -> Result<ClipboardMonitorStatus, String> {
    let monitor = monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?;
    Ok(ClipboardMonitorStatus {
        running: monitor.running && capture.worker_running(),
        ignored_applications: capture.ignored_apps(),
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
    config: tauri::State<'_, Mutex<ConfigStore>>,
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
    apps: Vec<String>,
) -> Result<Vec<String>, String> {
    apply_ignored_applications(config.inner(), monitor.inner(), capture.inner(), apps)
}

#[tauri::command]
fn mark_self_triggered(
    self_trigger: tauri::State<'_, SelfTriggerState>,
    text: String,
) -> Result<(), String> {
    self_trigger
        .0
        .lock()
        .map_err(|_| "self-trigger lock poisoned".to_owned())?
        .mark_clipboard_write(&text);
    Ok(())
}

#[tauri::command]
fn mark_self_triggered_image(
    self_trigger: tauri::State<'_, SelfTriggerState>,
    resource_path: Option<String>,
    content_hash: Option<String>,
) -> Result<(), String> {
    let mut guard = self_trigger
        .0
        .lock()
        .map_err(|_| "self-trigger lock poisoned".to_owned())?;
    register_image_self_trigger(
        &mut guard,
        resource_path.as_deref(),
        content_hash.as_deref(),
    )
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
    WindowManager::save_position(&mut guard, x, y, width, height)
}

#[tauri::command]
fn restore_window_position(
    window: tauri::Window,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<Option<WindowPosition>, String> {
    let mut config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    let Some((x, y, width, height)) = WindowManager::restore_position(&config) else {
        return Ok(None);
    };
    let saved = WindowPosition {
        x,
        y,
        width,
        height,
    };
    let work_areas = match window.available_monitors() {
        Ok(monitors) => monitors
            .into_iter()
            .map(|monitor| {
                let area = monitor.work_area();
                WindowWorkArea {
                    x: area.position.x,
                    y: area.position.y,
                    width: area.size.width,
                    height: area.size.height,
                }
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            eprintln!("[window] failed to enumerate monitors while restoring bounds: {error}");
            Vec::new()
        }
    };
    let restored = clamp_window_position_to_work_areas(saved, &work_areas);
    if restored != saved {
        WindowManager::save_position(
            &mut config,
            restored.x,
            restored.y,
            restored.width,
            restored.height,
        )?;
    }
    Ok(Some(restored))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowPosition {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowWorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn window_intersection_area(bounds: WindowPosition, area: WindowWorkArea) -> u64 {
    let left = i64::from(bounds.x).max(i64::from(area.x));
    let top = i64::from(bounds.y).max(i64::from(area.y));
    let right = (i64::from(bounds.x) + i64::from(bounds.width))
        .min(i64::from(area.x) + i64::from(area.width));
    let bottom = (i64::from(bounds.y) + i64::from(bounds.height))
        .min(i64::from(area.y) + i64::from(area.height));
    if right <= left || bottom <= top {
        return 0;
    }
    ((right - left) as u64).saturating_mul((bottom - top) as u64)
}

fn window_center_distance_squared(bounds: WindowPosition, area: WindowWorkArea) -> i128 {
    let bounds_center_x = i128::from(bounds.x) * 2 + i128::from(bounds.width);
    let bounds_center_y = i128::from(bounds.y) * 2 + i128::from(bounds.height);
    let area_center_x = i128::from(area.x) * 2 + i128::from(area.width);
    let area_center_y = i128::from(area.y) * 2 + i128::from(area.height);
    let dx = bounds_center_x - area_center_x;
    let dy = bounds_center_y - area_center_y;
    dx * dx + dy * dy
}

fn clamp_window_axis(position: i32, area_start: i32, area_size: u32, window_size: u32) -> i32 {
    let minimum = i64::from(area_start);
    let maximum = minimum + i64::from(area_size) - i64::from(window_size);
    if maximum <= minimum {
        return area_start;
    }
    i64::from(position).clamp(minimum, maximum) as i32
}

fn clamp_window_position_to_work_areas(
    saved: WindowPosition,
    work_areas: &[WindowWorkArea],
) -> WindowPosition {
    let normalized = WindowPosition {
        width: saved.width.max(MAIN_WINDOW_MIN_WIDTH),
        height: saved.height.max(MAIN_WINDOW_MIN_HEIGHT),
        ..saved
    };
    let mut best_overlap: Option<(WindowWorkArea, u64)> = None;
    let mut nearest: Option<(WindowWorkArea, i128)> = None;

    for area in work_areas
        .iter()
        .copied()
        .filter(|area| area.width > 0 && area.height > 0)
    {
        let overlap = window_intersection_area(normalized, area);
        if best_overlap
            .as_ref()
            .is_none_or(|(_, current)| overlap > *current)
        {
            best_overlap = Some((area, overlap));
        }

        let distance = window_center_distance_squared(normalized, area);
        if nearest
            .as_ref()
            .is_none_or(|(_, current)| distance < *current)
        {
            nearest = Some((area, distance));
        }
    }

    let target = best_overlap
        .filter(|(_, overlap)| *overlap > 0)
        .map(|(area, _)| area)
        .or_else(|| nearest.map(|(area, _)| area));
    let Some(target) = target else {
        return normalized;
    };

    let width = normalized
        .width
        .min(target.width.max(MAIN_WINDOW_MIN_WIDTH));
    let height = normalized
        .height
        .min(target.height.max(MAIN_WINDOW_MIN_HEIGHT));
    WindowPosition {
        x: clamp_window_axis(normalized.x, target.x, target.width, width),
        y: clamp_window_axis(normalized.y, target.y, target.height, height),
        width,
        height,
    }
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
    app: tauri::AppHandle,
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

    drop(config);
    if let Some(desired) = launch_at_startup {
        sync_autostart(&app, desired)?;
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
fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

#[tauri::command]
fn run_cli_command(
    database: tauri::State<'_, Database>,
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

    cli::run_cli_command(&args, database.inner())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalApiStatus {
    running: bool,
    port: u16,
}

#[tauri::command]
fn start_local_api(
    api: tauri::State<'_, Mutex<LocalApiServer>>,
    paths: tauri::State<'_, StoragePaths>,
    port: Option<u16>,
) -> Result<LocalApiStatus, String> {
    let database = Arc::new(Database::open(&paths.database).map_err(|error| error.to_string())?);
    let mut api = api
        .lock()
        .map_err(|_| "local API server lock is poisoned".to_owned())?;
    if let Some(port) = port {
        api.set_port(port)?;
    }
    let bound_port = api.start_with_database(database)?;
    Ok(LocalApiStatus {
        running: true,
        port: bound_port,
    })
}

#[tauri::command]
fn stop_local_api(api: tauri::State<'_, Mutex<LocalApiServer>>) -> Result<LocalApiStatus, String> {
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
fn get_local_api_status(
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

#[tauri::command]
fn get_performance_metrics(
    performance_tracker: tauri::State<'_, PerformanceTracker>,
) -> Result<PerformanceSnapshot, String> {
    Ok(performance_tracker.snapshot())
}

#[tauri::command]
fn repair_database(
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
fn validate_search_index(search_index: tauri::State<'_, SearchIndex>) -> Result<bool, String> {
    Ok(search_index.validate())
}

fn stop_runtime_services(app: &tauri::AppHandle) {
    if let Some(cleanup) = app.try_state::<Mutex<CleanupWorker>>() {
        match cleanup.lock() {
            Ok(cleanup) => cleanup.stop(),
            Err(_) => eprintln!("[shutdown] history cleanup lock is poisoned"),
        }
    }

    if let Some(monitor) = app.try_state::<Mutex<ClipboardMonitor>>() {
        match monitor.lock() {
            Ok(mut monitor) => {
                if let Err(error) = monitor.stop() {
                    eprintln!("[shutdown] failed to stop clipboard monitor: {error}");
                }
            }
            Err(_) => eprintln!("[shutdown] clipboard monitor lock is poisoned"),
        }
    }

    if let Some(capture) = app.try_state::<CaptureState>() {
        capture.stop_worker();
    }

    if let Some(worker) = app.try_state::<OcrWorkerManager>() {
        worker.stop();
    }

    if let Some(thumbnails) = app.try_state::<Mutex<ThumbnailWorker>>() {
        match thumbnails.lock() {
            Ok(mut worker) => worker.stop(),
            Err(_) => eprintln!("[shutdown] thumbnail worker lock is poisoned"),
        }
    }

    if let Some(hotkey) = app.try_state::<Mutex<HotkeyManager>>() {
        match hotkey.lock() {
            Ok(mut hotkey) => hotkey.stop(),
            Err(_) => eprintln!("[shutdown] hotkey manager lock is poisoned"),
        }
    }

    if let Some(api) = app.try_state::<Mutex<LocalApiServer>>() {
        match api.lock() {
            Ok(mut api) => {
                if let Err(error) = api.stop() {
                    eprintln!("[shutdown] failed to stop local API: {error}");
                }
            }
            Err(_) => eprintln!("[shutdown] local API lock is poisoned"),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Clipboard")
                .build(),
        )
        .setup(|app| {
            let startup_timer = &mut StartupTimer::start();
            let project_directory = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| app.path().app_data_dir().unwrap_or_default());
            let config = ConfigStore::load(&project_directory)?;
            if config.single_instance() {
                let mut guard = match SingleInstanceGuard::acquire(&project_directory) {
                    Ok(guard) => guard,
                    Err(error) => {
                        if let SingleInstanceError::AlreadyRunning(owner_pid) = &error {
                            if !SingleInstanceGuard::notify_existing_instance(
                                &project_directory,
                                *owner_pid,
                            ) {
                                eprintln!(
                                    "[single-instance] failed to notify the existing process (PID: {owner_pid})"
                                );
                            }
                        }
                        return Err(error.into());
                    }
                };
                let app_handle = app.handle().clone();
                guard.start_wake_listener(move || show_main_window(&app_handle))?;
                app.manage(guard);
            }
            let keyboard = KeyboardManager::load(&project_directory)?;
            let paths = StoragePaths::initialize_with_resource_directories(
                project_directory.clone(),
                config.storage_directory().map(PathBuf::from),
                config.image_storage_path().map(PathBuf::from),
                config.file_storage_path().map(PathBuf::from),
            )?;
            let recovery_report = recover_database_if_needed(&paths.database)?;
            if let Some(report) = &recovery_report {
                eprintln!(
                    "[recovery] restored database from {}",
                    report.restored_from.display()
                );
                if let Some(quarantined_database) = &report.quarantined_database {
                    eprintln!(
                        "[recovery] quarantined damaged database at {}",
                        quarantined_database.display()
                    );
                }
                if let Some(quarantined_index) = quarantine_search_index(&paths.search_index)? {
                    eprintln!(
                        "[recovery] quarantined stale search index at {}",
                        quarantined_index.display()
                    );
                }
            }
            let database = Database::open(&paths.database)?;

            let repair_result = database.repair()?;
            if !repair_result.integrity_ok {
                return Err(storage::StorageError::DatabaseRecoveryUnavailable {
                    database: paths.database.clone(),
                    reason: repair_result.integrity_message,
                }
                .into());
            }
            if let Err(error) = refresh_database_backup(&database, &paths.database) {
                eprintln!("[recovery] failed to refresh database backup: {error}");
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

            let ocr_engine_name = config.ocr_engine().to_string();
            let models_dir = ocr::models::models_dir(&paths.storage);
            let ppocr_model = configured_ppocr_model(&config);
            let ppocr_ready = ocr::models::model_is_installed(&models_dir, ppocr_model);
            let score_threshold = config.det_score_threshold();
            let box_threshold = config.det_box_threshold();
            let unclip_ratio = config.det_unclip_ratio();

            let ocr_engine: Arc<dyn OcrEngine> = if ocr_engine_name == "ppocr" && ppocr_ready {
                Arc::new(PpOcrEngine::new(
                    models_dir,
                    ppocr_model,
                    score_threshold,
                    box_threshold,
                    unclip_ratio,
                ))
            } else if ocr_engine_name == "tesseract" && TesseractOcrEngine::is_available() {
                Arc::new(TesseractOcrEngine::with_languages(config.tesseract_languages().to_string()))
            } else if TesseractOcrEngine::is_available() {
                eprintln!("[ocr] falling back to Tesseract");
                Arc::new(TesseractOcrEngine::with_languages("chi_sim"))
            } else {
                eprintln!("[ocr] no OCR engine available");
                Arc::new(NoopOcrEngine)
            };
            let ocr_database = Database::open(&paths.database)?;
            let ocr_worker = OcrWorkerManager::start(ocr_engine, Arc::new(ocr_database));

            let thumbnail_database = Database::open(&paths.database)?;
            let thumbnail_worker =
                ThumbnailWorker::start(paths.previews.clone(), Arc::new(thumbnail_database));
            let thumbnail_queue = thumbnail_worker.queue();

            let mut privacy_manager = PrivacyManager::new();
            privacy_manager.sync_with_config(&config);
            let mut clipboard_monitor = ClipboardMonitor::new();
            let shortcut_manager = GlobalShortcutManager::new();

            // Auto-start clipboard monitoring in background
            let app_handle = app.handle().clone();
            let db_path = paths.database.clone();
            let storage_path = paths.storage.clone();
            let image_storage_path = paths.images.clone();
            let file_storage_path = paths.files.clone();
            let initial_ignored = config.ignored_applications().to_vec();
            let capture_state = CaptureState::new(
                &privacy_manager,
                initial_ignored.clone(),
                config.max_file_copy_size_bytes(),
            );
            app.manage(capture_state.clone());

            let self_trigger_guard = Arc::new(Mutex::new(content::hash::SelfTriggerGuard::new()));
            let self_trigger_guard_managed = SelfTriggerState(self_trigger_guard.clone());
            app.manage(self_trigger_guard_managed);

            // Route interrupt signals through the Tauri event loop so every
            // runtime service follows the same shutdown path.
            let app_handle_for_shutdown = app.handle().clone();
            ctrlc::set_handler(move || {
                eprintln!("[shutdown] received interrupt signal");
                app_handle_for_shutdown.exit(0);
            })
            .ok();

            clipboard_monitor.set_ignored_apps(initial_ignored);

            if clipboard_monitor.start().is_ok() {
                if let Some(receiver) = clipboard_monitor.take_receiver() {
                    let self_trigger_clone = self_trigger_guard.clone();
                    let capture_for_thread = capture_state.clone();
                    let stop_flag = Arc::new(AtomicBool::new(false));
                    let stop_flag_for_thread = Arc::clone(&stop_flag);
                    let (stop_sender, stop_receiver) = mpsc::channel();
                    let handle = thread::Builder::new()
                        .name("clipboard-capture".to_owned())
                        .spawn(move || {
                    let database = match Database::open(&db_path) {
                        Ok(db) => db,
                        Err(e) => {
                            eprintln!("[clipboard-worker] failed to open database: {e}");
                            return;
                        }
                    };

                    let self_trigger_guard = self_trigger_clone;
                    let mut consecutive_errors = 0u32;

                    loop {
                        if stop_flag_for_thread.load(Ordering::SeqCst)
                            || stop_signal_requested(&stop_receiver)
                        {
                            break;
                        }
                        match receiver.recv_timeout(Duration::from_millis(500)) {
                            Ok(change) => {
                                let _ingestion_guard = capture_for_thread
                                    .ingestion_guard
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                let clipboard_formats = change.formats;
                                if stop_flag_for_thread.load(Ordering::SeqCst)
                                    || stop_signal_requested(&stop_receiver)
                                {
                                    break;
                                }

                                let app_info = platform::windows_clipboard::get_foreground_app();
                                let source_app = foreground_app_name(&app_info);
                                if capture_for_thread.should_skip(source_app.as_deref(), None) {
                                    continue;
                                }

                                // Extract and cache app icon
                                let icon_dir = storage_path.join("icons");
                                let icon_path = if let Some(source_name) = source_app.as_deref() {
                                    platform::windows_clipboard::extract_app_icon(
                                        &icon_dir,
                                        source_name,
                                        &app_info.exe_path,
                                    )
                                } else {
                                    None
                                };

                                let text = platform::windows_clipboard::read_clipboard_text();
                                let image_data = platform::windows_clipboard::read_clipboard_image();
                                let file_paths = platform::windows_clipboard::read_clipboard_file_paths();

                                if capture_for_thread.should_skip(source_app.as_deref(), text.as_deref()) {
                                    continue;
                                }

                                if let Some((img, img_width, img_height)) = image_data {
                                    if stop_flag_for_thread.load(Ordering::SeqCst) {
                                        break;
                                    }
                                    if should_skip_self_triggered_media(
                                        &mut self_trigger_guard.lock().unwrap(),
                                        "image",
                                        &img,
                                    ) {
                                        continue;
                                    }
                                    let img_hash = content::hash::compute_media_hash("image", &img);
                                    let now_ms = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as i64;

                                    let image_dir = image_storage_path.clone();
                                    std::fs::create_dir_all(&image_dir).ok();
                                    let img_path = image_dir.join(format!("{}.png", img_hash));
                                    match std::fs::write(&img_path, &img) {
                                        Ok(_) => eprintln!("[clipboard-worker] saved image: {}", img_path.display()),
                                        Err(e) => eprintln!("[clipboard-worker] failed to write image {}: {}", img_path.display(), e),
                                    }

                                    let image_path = img_path.to_string_lossy().to_string();
                                    let metadata = serde_json::json!({
                                        "schemaVersion": RESOURCE_METADATA_SCHEMA_VERSION,
                                        "width": img_width,
                                        "height": img_height,
                                        "mimeType": "image/png",
                                        "extension": "png",
                                        "sizeBytes": img.len(),
                                        "resourcePath": image_path,
                                        "previewPath": image_path,
                                        "storagePath": image_path,
                                        "contentHash": img_hash,
                                    });

                                    let item = ClipboardItem {
                                        id: format!("img_{}", img_hash),
                                        kind: ClipboardKind::Image,
                                        title: img_path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
                                        text_content: None,
                                        resource_path: Some(image_path.clone()),
                                        preview_path: Some(image_path),
                                        content_hash: img_hash,
                                            source_app: source_app.clone(),
                                            icon_path: icon_path.clone(),
                                            size_bytes: img.len() as u64,
                                        created_at_ms: now_ms,
                                        last_used_at_ms: None,
                                        is_favorite: false,
                                        metadata_json: content::merge_clipboard_format_metadata(
                                            Some(&metadata.to_string()),
                                            &clipboard_formats,
                                        )
                                        .unwrap_or_else(|_| Some(metadata.to_string())),
                                    };

                                    if stop_flag_for_thread.load(Ordering::SeqCst) {
                                        break;
                                    }
                                    match database.save_item(&item) {
                                        Ok(saved_id) => {
                                            consecutive_errors = 0;
                                            let _ = database.enqueue_ocr(&saved_id);
                                            thumbnail_queue
                                                .enqueue(saved_id.clone(), img_path.clone());
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
                                    if stop_flag_for_thread.load(Ordering::SeqCst) {
                                        break;
                                    }
                                    let now_ms = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as i64;

                                    if file_paths.len() == 1 {
                                        let file_path = &file_paths[0];
                                        let file_hash = content::hash::compute_file_capture_hash(
                                            std::slice::from_ref(file_path),
                                        );
                                        if should_skip_self_triggered_hash(
                                            &mut self_trigger_guard.lock().unwrap(),
                                            &file_hash,
                                        ) {
                                            continue;
                                        }
                                        let stored_files = store_captured_file_references(
                                            std::slice::from_ref(file_path),
                                            &file_storage_path,
                                            capture_for_thread.max_file_copy_size_bytes(),
                                        );
                                        let stored_file = &stored_files[0];

                                        let item = ClipboardItem {
                                            id: format!("file_{}", file_hash),
                                            kind: ClipboardKind::File,
                                            title: stored_file.original_name.clone(),
                                            text_content: None,
                                            resource_path: Some(stored_file.storage_path.clone()),
                                            preview_path: None,
                                            content_hash: file_hash,
                                            source_app: source_app.clone(),
                                            icon_path: icon_path.clone(),
                                            size_bytes: stored_file.size_bytes,
                                            created_at_ms: now_ms,
                                            last_used_at_ms: None,
                                            is_favorite: false,
                                            metadata_json: content::merge_clipboard_format_metadata(
                                                Some(&captured_file_metadata(&stored_files)),
                                                &clipboard_formats,
                                            )
                                            .unwrap_or_else(|_| {
                                                Some(captured_file_metadata(&stored_files))
                                            }),
                                        };

                                        if stop_flag_for_thread.load(Ordering::SeqCst) {
                                            break;
                                        }
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
                                    } else {
                                        let group_hash =
                                            content::hash::compute_file_capture_hash(&file_paths);
                                        if should_skip_self_triggered_hash(
                                            &mut self_trigger_guard.lock().unwrap(),
                                            &group_hash,
                                        ) {
                                            continue;
                                        }

                                        let stored_files = store_captured_file_references(
                                            &file_paths,
                                            &file_storage_path,
                                            capture_for_thread.max_file_copy_size_bytes(),
                                        );
                                        let total_size = stored_files
                                            .iter()
                                            .map(|file| file.size_bytes)
                                            .sum();
                                        let stored_paths = stored_files
                                            .iter()
                                            .map(|file| file.storage_path.clone())
                                            .collect::<Vec<_>>();
                                        let paths_json = serde_json::to_string(&stored_paths)
                                            .unwrap_or_default();

                                        let item = ClipboardItem {
                                            id: format!("files_{}", group_hash),
                                            kind: ClipboardKind::File,
                                            title: stored_files[0].original_name.clone(),
                                            text_content: Some(paths_json),
                                            resource_path: Some(stored_files[0].storage_path.clone()),
                                            preview_path: None,
                                            content_hash: group_hash,
                                            source_app: source_app.clone(),
                                            icon_path: icon_path.clone(),
                                            size_bytes: total_size,
                                            created_at_ms: now_ms,
                                            last_used_at_ms: None,
                                            is_favorite: false,
                                            metadata_json: content::merge_clipboard_format_metadata(
                                                Some(&captured_file_metadata(&stored_files)),
                                                &clipboard_formats,
                                            )
                                            .unwrap_or_else(|_| {
                                                Some(captured_file_metadata(&stored_files))
                                            }),
                                        };

                                        if stop_flag_for_thread.load(Ordering::SeqCst) {
                                            break;
                                        }
                                        match database.save_item(&item) {
                                            Ok(saved_id) => {
                                                consecutive_errors = 0;
                                                let mut emit_item = item.clone();
                                                emit_item.id = saved_id;
                                                let _ = app_handle.emit("clipboard-item-added", &emit_item);
                                            }
                                            Err(e) => {
                                                eprintln!("[clipboard-worker] failed to save file batch: {e}");
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

                                if should_skip_self_triggered_text(
                                    &mut self_trigger_guard.lock().unwrap(),
                                    kind,
                                    &text,
                                ) {
                                    continue;
                                }

                                let title = text
                                    .chars()
                                    .take(200)
                                    .collect::<String>();
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
                                    metadata_json: content::merge_clipboard_format_metadata(
                                        None,
                                        &clipboard_formats,
                                    )
                                    .unwrap_or(None),
                                };

                                if stop_flag_for_thread.load(Ordering::SeqCst) {
                                    break;
                                }
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
                                            if wait_for_stop(
                                                &stop_receiver,
                                                &stop_flag_for_thread,
                                                Duration::from_secs(5),
                                            ) {
                                                break;
                                            }
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
                        })
                        .map_err(|error| format!("failed to start startup clipboard worker: {error}"))?;

                    capture_state.install_worker(CaptureWorker {
                        stop_flag,
                        stop_sender: Some(stop_sender),
                        handle: Some(handle),
                    });
            } else {
                eprintln!("[startup] clipboard monitor has no receiver");
            }
        } else {
            eprintln!("[startup] failed to start clipboard monitor");
        }

            let cleanup_database = Database::open(&paths.database)?;
            let cleanup_worker =
                CleanupWorker::start(project_directory.clone(), cleanup_database, paths.clone())?;

            let launch_at_startup = config.launch_at_startup();
            let startup_transparency = config.general_settings().window_transparency;
            app.manage(Mutex::new(config));
            app.manage(paths);
            app.manage(database);
            app.manage(search_index);
            app.manage(performance_tracker);
            app.manage(Mutex::new(privacy_manager));
            app.manage(Mutex::new(clipboard_monitor));
            app.manage(Mutex::new(shortcut_manager));
            app.manage(ocr_worker);
            app.manage(Mutex::new(thumbnail_worker));
            app.manage(Mutex::new(cleanup_worker));
            app.manage(Mutex::new(LocalApiServer::new(0)));

            if let Err(error) = sync_autostart(app.handle(), launch_at_startup) {
                eprintln!("[autostart] failed to synchronize startup registration: {error}");
            }

            SystemTray::create(app.handle())?;

            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                let window_to_hide = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let close_to_tray = match app_handle
                            .state::<Mutex<ConfigStore>>()
                            .lock()
                        {
                            Ok(config) => config.close_to_tray(),
                            Err(_) => {
                                eprintln!(
                                    "[tray] configuration lock is poisoned; allowing window close"
                                );
                                false
                            }
                        };

                        if close_to_tray {
                            api.prevent_close();
                            if let Err(error) = window_to_hide.hide() {
                                eprintln!("[tray] failed to hide the main window: {error}");
                            }
                        }
                    }
                });
            }

            // Register global hotkey from keyboard config
            let mut hotkey_manager = HotkeyManager::new();
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                let kb_config = keyboard.config();
                let (bindings, double_modifiers) = resolve_toggle_hotkeys(&kb_config);
                if !bindings.is_empty() || !double_modifiers.is_empty() {
                    hotkey_manager.start_with_hotkeys(bindings, double_modifiers, window.clone());
                } else {
                    eprintln!("[hotkey] no valid toggleWindow shortcut found in config, using default Alt+V");
                    use platform::windows_clipboard;
                    hotkey_manager.start_with_window(windows_clipboard::MOD_ALT, windows_clipboard::VK_V, window.clone());
                }
            }
            app.manage(Mutex::new(keyboard));
            app.manage(Mutex::new(hotkey_manager));

            apply_window_transparency_to_main(app.handle(), startup_transparency);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_info,
            get_storage_status,
            get_storage_kind_stats,
            configure_storage_directory,
            get_application_filter_settings,
            configure_ignored_applications,
            list_clipboard_items,
            set_clipboard_item_favorite,
            batch_set_favorite,
            delete_clipboard_item,
            batch_delete_clipboard_items,
            get_clipboard_item_ocr,
            regenerate_clipboard_item_ocr,
            list_source_applications,
            get_keyboard_config,
            configure_keyboard_shortcuts,
            paste_to_previous_application,
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
            get_general_settings,
            set_general_settings,
            get_window_config,
            set_window_config,
            get_export_config,
            set_export_config,
            run_cli_command,
            start_local_api,
            stop_local_api,
            get_local_api_status,
            get_clipboard_formats,
            get_ocr_status,
            get_ocr_config,
            set_ocr_config,
            restart_ocr_engine,
            install_ppocr,
            check_ppocr_status,
            mark_self_triggered,
            mark_self_triggered_image,
            open_external_url,
            reveal_in_explorer,
            copy_file_to,
            rename_item,
            update_clipboard_text,
            set_history_config,
            get_history_config,
            set_storage_config,
            set_resource_storage_paths,
            get_storage_config,
            detect_content_actions,
            soft_delete_clipboard_item,
            restore_clipboard_item,
            list_deleted_clipboard_items,
            batch_restore_clipboard_items,
            permanently_delete_clipboard_item,
            batch_permanently_delete_clipboard_items,
            permanently_delete_storage_kind,
            duplicate_clipboard_item,
            enforce_history_cleanup,
            clear_all_non_favorite_items,
            get_performance_metrics,
            memory::get_memory_diagnostics,
            repair_database,
            validate_search_index,
            cleanup_storage_files,
            restart_app
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            stop_runtime_services(app_handle);
        }
    });
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    fn capture_state() -> CaptureState {
        CaptureState::new(
            &PrivacyManager::new(),
            vec!["IgnoredApp".to_owned()],
            100 * 1024 * 1024,
        )
    }

    #[test]
    fn pause_state_is_shared_with_capture_policy() {
        let state = capture_state();
        assert!(!state.should_skip(Some("Notepad"), Some("ordinary text")));

        state.set_paused(true);
        assert!(state.should_skip(Some("Notepad"), Some("ordinary text")));

        state.set_paused(false);
        assert!(!state.should_skip(Some("Notepad"), Some("ordinary text")));
    }

    #[test]
    fn file_copy_limit_updates_are_immediately_visible_to_capture() {
        let state = capture_state();
        assert_eq!(state.max_file_copy_size_bytes(), 100 * 1024 * 1024);

        state.set_max_file_copy_size_bytes(8 * 1024 * 1024);

        assert_eq!(state.max_file_copy_size_bytes(), 8 * 1024 * 1024);
    }

    #[test]
    fn ignored_and_password_manager_sources_are_rejected_case_insensitively() {
        let state = capture_state();

        assert!(state.should_skip(Some("ignoredapp.exe"), None));
        assert!(state.should_skip(Some(r"C:\Program Files\KeePass\KeePass.exe"), None));
        assert!(state.should_skip(Some("1PASSWORD"), None));
        assert!(!state.should_skip(Some("notepad.exe"), None));
    }

    #[test]
    fn ignored_application_updates_are_deduplicated_and_immediately_visible() {
        let state = capture_state();
        let stored = state.set_ignored_apps(vec![
            " Browser.exe ".to_owned(),
            "browser".to_owned(),
            "Terminal".to_owned(),
        ]);

        assert_eq!(stored, vec!["Browser.exe", "Terminal"]);
        assert_eq!(state.ignored_apps(), stored);
        assert!(state.should_skip(Some("browser.exe"), None));
        assert!(state.should_skip(Some("TERMINAL"), None));
    }

    #[test]
    fn sensitive_text_is_rejected_before_persistence() {
        let state = capture_state();

        assert!(state.should_skip(Some("Notepad"), Some("password=supersecret123")));
        assert!(state.should_skip(Some("Notepad"), Some("4111 1111 1111 1111")));
        assert!(!state.should_skip(Some("Notepad"), Some("meeting notes")));
    }

    #[test]
    fn foreground_source_uses_name_then_executable_fallback() {
        let named = platform::windows_clipboard::ForegroundApp {
            name: "Editor".to_owned(),
            exe_path: r"C:\Apps\editor.exe".to_owned(),
        };
        let path_only = platform::windows_clipboard::ForegroundApp {
            name: String::new(),
            exe_path: r"C:\Apps\Browser.exe".to_owned(),
        };

        assert_eq!(foreground_app_name(&named).as_deref(), Some("editor"));
        assert_eq!(foreground_app_name(&path_only).as_deref(), Some("Browser"));
    }

    #[test]
    fn self_triggered_link_write_is_skipped_before_source_metadata_can_change() {
        let text = "https://example.com";
        let database = Database::open_in_memory().unwrap();
        let original = ClipboardItem {
            id: "original".to_owned(),
            kind: ClipboardKind::Link,
            title: text.to_owned(),
            text_content: Some(text.to_owned()),
            resource_path: None,
            preview_path: None,
            content_hash: content::hash::compute_content_hash("link", text, None),
            source_app: Some("Browser".to_owned()),
            icon_path: Some("browser.png".to_owned()),
            size_bytes: text.len() as u64,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        };
        database.save_item(&original).unwrap();

        let mut guard = content::hash::SelfTriggerGuard::new();
        guard.mark_clipboard_write(text);
        assert!(should_skip_self_triggered_text(
            &mut guard,
            ClipboardKind::Link,
            text,
        ));

        let stored = database.get_item("original").unwrap().unwrap();
        assert_eq!(stored.source_app, original.source_app);
        assert_eq!(stored.icon_path, original.icon_path);
        assert_eq!(stored.created_at_ms, original.created_at_ms);
    }

    #[test]
    fn self_triggered_file_writes_match_single_and_group_hashes() {
        let single_path = r"C:\Users\admin\Documents\report.txt";
        let mut single_guard = content::hash::SelfTriggerGuard::new();
        single_guard.mark_clipboard_write(single_path);
        let single_hash = content::hash::compute_content_hash("file", single_path, None);
        assert!(should_skip_self_triggered_hash(
            &mut single_guard,
            &single_hash
        ));

        let paths = [
            r"C:\Users\admin\Documents\zeta.txt",
            r"C:\Users\admin\Documents\alpha.txt",
        ];
        let mut group_guard = content::hash::SelfTriggerGuard::new();
        group_guard.mark_clipboard_write(&paths.join("\n"));
        let mut sorted_paths = paths.to_vec();
        sorted_paths.sort();
        let group_hash =
            content::hash::compute_content_hash("files", &sorted_paths.join("\n"), None);
        assert!(should_skip_self_triggered_hash(
            &mut group_guard,
            &group_hash
        ));
    }

    #[test]
    fn captured_files_are_copied_into_managed_storage() {
        let root = std::env::temp_dir().join(format!(
            "clipboard-captured-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source_dir = root.join("source");
        let storage_dir = root.join("storage/files");
        std::fs::create_dir_all(&source_dir).unwrap();
        let first = source_dir.join("first.txt");
        let second = source_dir.join("second.json");
        std::fs::write(&first, b"first file").unwrap();
        std::fs::write(&second, b"{\"second\":true}").unwrap();

        let stored = store_captured_file_references(
            &[
                first.to_string_lossy().to_string(),
                second.to_string_lossy().to_string(),
            ],
            &storage_dir,
            1024,
        );

        assert_eq!(stored.len(), 2);
        assert!(stored.iter().all(|file| file.copied));
        assert!(stored
            .iter()
            .all(|file| Path::new(&file.storage_path).starts_with(&storage_dir)));
        assert_eq!(
            std::fs::read(&stored[0].storage_path).unwrap(),
            b"first file"
        );
        assert_eq!(
            Path::new(&stored[1].storage_path).extension(),
            Some(std::ffi::OsStr::new("json"))
        );

        let metadata: serde_json::Value =
            serde_json::from_str(&captured_file_metadata(&stored)).unwrap();
        assert_eq!(metadata["schemaVersion"], RESOURCE_METADATA_SCHEMA_VERSION);
        assert_eq!(metadata["files"][0]["name"], "first.txt");
        assert_eq!(metadata["files"][0]["mimeType"], "text/plain");
        assert_eq!(metadata["files"][0]["extension"], "txt");
        assert_eq!(metadata["files"][0]["sizeBytes"], 10);
        assert!(metadata["files"][0]["contentHash"].is_string());
        assert_eq!(metadata["files"][1]["copied"], true);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_captured_file_keeps_the_original_link() {
        let root = std::env::temp_dir().join(format!(
            "clipboard-captured-file-limit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("source/large.bin");
        let storage_dir = root.join("storage/files");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, vec![7_u8; 32]).unwrap();

        let stored = store_captured_file_references(
            &[source.to_string_lossy().to_string()],
            &storage_dir,
            8,
        );

        assert_eq!(stored.len(), 1);
        assert!(!stored[0].copied);
        assert_eq!(stored[0].storage_path, source.to_string_lossy());
        assert_eq!(stored[0].size_bytes, 32);
        assert_eq!(stored[0].mime_type, "application/octet-stream");
        assert_eq!(stored[0].extension.as_deref(), Some("bin"));
        assert_eq!(std::fs::read_dir(&storage_dir).unwrap().count(), 0);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn self_triggered_image_write_matches_normalized_capture_hash() {
        use std::io::Cursor;

        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([20, 40, 80, 255]));
        let mut stored_png = Cursor::new(Vec::new());
        image
            .write_to(&mut stored_png, image::ImageFormat::Png)
            .expect("PNG encoding should succeed");
        let mut clipboard_bmp = Cursor::new(Vec::new());
        image
            .write_to(&mut clipboard_bmp, image::ImageFormat::Bmp)
            .expect("BMP encoding should succeed");

        let path = std::env::temp_dir().join(format!(
            "clipboard-self-trigger-image-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        std::fs::write(&path, stored_png.get_ref()).expect("test image should be writable");

        let mut guard = content::hash::SelfTriggerGuard::new();
        register_image_self_trigger(&mut guard, path.to_str(), None).unwrap();
        assert!(should_skip_self_triggered_media(
            &mut guard,
            "image",
            clipboard_bmp.get_ref()
        ));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn image_self_trigger_registration_falls_back_to_persisted_hash() {
        let bytes = b"legacy-image-bytes";
        let fallback_hash = content::hash::compute_media_hash("image", bytes);
        let mut guard = content::hash::SelfTriggerGuard::new();

        register_image_self_trigger(
            &mut guard,
            Some("C:\\missing\\clipboard-image.png"),
            Some(&fallback_hash),
        )
        .unwrap();

        assert!(should_skip_self_triggered_hash(&mut guard, &fallback_hash));
    }

    #[test]
    fn invalid_sensitive_patterns_are_excluded_during_initialization() {
        let mut privacy = PrivacyManager::new();
        privacy.sensitive_patterns = vec!["[invalid".to_owned(), "secret".to_owned()];
        let state = CaptureState::new(&privacy, Vec::new(), 100 * 1024 * 1024);

        assert_eq!(state.policy.sensitive_patterns.len(), 1);
        assert!(state.should_skip(Some("Notepad"), Some("a secret value")));
        assert!(!state.should_skip(Some("Notepad"), Some("ordinary text")));
    }

    #[test]
    fn capture_worker_stop_wakes_and_joins_thread() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let exited_for_thread = Arc::clone(&exited);
        let (stop_sender, stop_receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = stop_receiver.recv();
            exited_for_thread.store(true, Ordering::SeqCst);
        });
        let mut worker = CaptureWorker {
            stop_flag: Arc::clone(&stop_flag),
            stop_sender: Some(stop_sender),
            handle: Some(handle),
        };

        worker.stop();

        assert!(stop_flag.load(Ordering::SeqCst));
        assert!(exited.load(Ordering::SeqCst));
        assert!(worker.handle.is_none());
    }
}

#[cfg(test)]
mod title_metadata_tests {
    use super::*;

    #[test]
    fn legacy_records_infer_custom_titles_from_the_generated_title() {
        assert!(!resolve_custom_title(
            "first line\nsecond line",
            "first line\nsecond line",
            None,
        ));
        assert!(resolve_custom_title(
            "Pinned note",
            "first line\nsecond line",
            None,
        ));
    }

    #[test]
    fn explicit_custom_title_metadata_overrides_legacy_inference() {
        assert!(resolve_custom_title(
            "first line",
            "first line\nsecond line",
            Some(r#"{"customTitle":true}"#),
        ));
        assert!(!resolve_custom_title(
            "Pinned note",
            "first line\nsecond line",
            Some(r#"{"customTitle":false}"#),
        ));
    }

    #[test]
    fn setting_custom_title_metadata_preserves_existing_object_fields() {
        let metadata = set_custom_title_metadata(Some(r#"{"width":120}"#), true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(value["width"], 120);
        assert_eq!(value["customTitle"], true);
    }
}

#[cfg(test)]
mod window_position_tests {
    use super::*;

    fn bounds(x: i32, y: i32, width: u32, height: u32) -> WindowPosition {
        WindowPosition {
            x,
            y,
            width,
            height,
        }
    }

    fn work_area(x: i32, y: i32, width: u32, height: u32) -> WindowWorkArea {
        WindowWorkArea {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn keeps_bounds_inside_the_current_work_area() {
        let saved = bounds(240, 120, 900, 700);
        let areas = [work_area(0, 0, 1920, 1040)];

        assert_eq!(clamp_window_position_to_work_areas(saved, &areas), saved);
    }

    #[test]
    fn clamps_an_offscreen_window_to_the_visible_edge() {
        let saved = bounds(2500, 100, 900, 700);
        let areas = [work_area(0, 0, 1920, 1040)];

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &areas),
            bounds(1020, 100, 900, 700)
        );
    }

    #[test]
    fn moves_a_window_from_a_removed_monitor_to_the_nearest_remaining_area() {
        let saved = bounds(-1600, 80, 900, 700);
        let areas = [work_area(0, 0, 1920, 1040)];

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &areas),
            bounds(0, 80, 900, 700)
        );
    }

    #[test]
    fn preserves_negative_coordinates_for_a_connected_left_monitor() {
        let saved = bounds(-1800, 120, 800, 700);
        let areas = [work_area(-1920, 0, 1920, 1040), work_area(0, 0, 1920, 1040)];

        assert_eq!(clamp_window_position_to_work_areas(saved, &areas), saved);
    }

    #[test]
    fn chooses_the_monitor_with_the_largest_visible_overlap() {
        let saved = bounds(1700, 100, 800, 700);
        let areas = [work_area(0, 0, 1920, 1040), work_area(1920, 0, 1920, 1040)];

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &areas),
            bounds(1920, 100, 800, 700)
        );
    }

    #[test]
    fn limits_oversized_bounds_to_the_selected_work_area() {
        let saved = bounds(-400, -300, 4000, 2400);
        let areas = [work_area(0, 0, 1920, 1040)];

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &areas),
            bounds(0, 0, 1920, 1040)
        );
    }

    #[test]
    fn applies_native_minimums_without_monitor_information() {
        let saved = bounds(20, 30, 0, 0);

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &[]),
            bounds(20, 30, MAIN_WINDOW_MIN_WIDTH, MAIN_WINDOW_MIN_HEIGHT)
        );
    }
}

#[cfg(test)]
mod storage_cleanup_tests {
    use std::{fs, time::SystemTime};

    use super::*;

    fn temporary_project(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "clipboard-storage-cleanup-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn temporary_storage(label: &str) -> StoragePaths {
        StoragePaths::initialize(temporary_project(label)).unwrap()
    }

    fn stored_item(
        id: &str,
        kind: ClipboardKind,
        resource_path: Option<String>,
        icon_path: Option<String>,
    ) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind,
            title: format!("record-{id}"),
            text_content: (kind == ClipboardKind::Text).then(|| format!("content-{id}")),
            resource_path,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            icon_path,
            size_bytes: 12,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    #[test]
    fn cleanup_preserves_soft_deleted_resources_until_permanent_deletion() {
        let paths = temporary_storage("recycle-bin");
        let resource = paths.images.join("recoverable.png");
        let preview = paths.previews.join("recoverable-preview.png");
        fs::write(&resource, b"image-data").unwrap();
        fs::write(&preview, b"preview-data").unwrap();
        let database = Database::open_in_memory().unwrap();
        let mut item = stored_item(
            "recoverable",
            ClipboardKind::Image,
            Some(resource.to_string_lossy().into_owned()),
            None,
        );
        item.preview_path = Some(preview.to_string_lossy().into_owned());
        database.save_item(&item).unwrap();
        database.soft_delete("recoverable").unwrap();

        let first_cleanup = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(first_cleanup.removed_files, 0);
        assert!(resource.exists());
        assert!(preview.exists());
        assert!(database.restore_deleted("recoverable").unwrap());

        database.soft_delete("recoverable").unwrap();
        assert!(database.permanently_delete("recoverable").unwrap());
        let second_cleanup = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(second_cleanup.removed_files, 2);
        assert!(!resource.exists());
        assert!(!preview.exists());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn category_delete_cleans_ocr_search_index_and_managed_resources() {
        let paths = temporary_storage("category-delete");
        let resource = paths.images.join("category.png");
        let preview = paths.previews.join("category-preview.png");
        let favorite_resource = paths.images.join("favorite.png");
        let favorite_preview = paths.previews.join("favorite-preview.png");
        let icons = paths.storage.join("icons");
        let shared_icon = icons.join("shared.png");
        fs::create_dir_all(&icons).unwrap();
        fs::write(&resource, b"image-data").unwrap();
        fs::write(&preview, b"preview-data").unwrap();
        fs::write(&favorite_resource, b"favorite-image-data").unwrap();
        fs::write(&favorite_preview, b"favorite-preview-data").unwrap();
        fs::write(&shared_icon, b"shared-icon-data").unwrap();

        let database = Database::open_in_memory().unwrap();
        let mut image = stored_item(
            "category-image",
            ClipboardKind::Image,
            Some(resource.to_string_lossy().into_owned()),
            Some("shared.png".to_owned()),
        );
        image.preview_path = Some(preview.to_string_lossy().into_owned());
        database.save_item(&image).unwrap();
        let mut favorite_image = stored_item(
            "favorite-image",
            ClipboardKind::Image,
            Some(favorite_resource.to_string_lossy().into_owned()),
            Some("shared.png".to_owned()),
        );
        favorite_image.preview_path = Some(favorite_preview.to_string_lossy().into_owned());
        favorite_image.is_favorite = true;
        database.save_item(&favorite_image).unwrap();
        database
            .save_ocr_result(&OcrResult {
                item_id: image.id.clone(),
                status: domain::OcrStatus::Completed,
                engine: "test".to_owned(),
                model_version: "1".to_owned(),
                language: Some("en".to_owned()),
                full_text: "category recognized text".to_owned(),
                blocks: Vec::new(),
                image_hash: image.content_hash.clone(),
                created_at_ms: 1,
                completed_at_ms: Some(2),
                error_message: None,
            })
            .unwrap();
        database
            .save_item(&stored_item(
                "preserved-text",
                ClipboardKind::Text,
                None,
                None,
            ))
            .unwrap();

        let search_index = SearchIndex::in_memory().unwrap();
        SearchSynchronizer::default()
            .sync_until_idle(&database, &search_index)
            .unwrap();
        assert_eq!(search_index.search("recognized", 20).unwrap().len(), 1);

        let result = permanently_delete_storage_kind_for(
            &database,
            &paths,
            &search_index,
            ClipboardKind::Image,
            None,
        )
        .unwrap();

        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.deleted_size_bytes, image.size_bytes);
        assert_eq!(result.deleted_ids, vec![image.id.clone()]);
        assert_eq!(result.removed_files, 2);
        assert_eq!(result.search_sync.as_ref().unwrap().deleted_documents, 1);
        assert!(result.warnings.is_empty());
        assert!(database.get_item(&image.id).unwrap().is_none());
        assert!(database.get_ocr_result(&image.id).unwrap().is_none());
        assert!(database.get_item(&favorite_image.id).unwrap().is_some());
        assert!(database.get_item("preserved-text").unwrap().is_some());
        assert!(search_index.search("recognized", 20).unwrap().is_empty());
        assert!(!resource.exists());
        assert!(!preview.exists());
        assert!(favorite_resource.exists());
        assert!(favorite_preview.exists());
        assert!(shared_icon.exists());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn category_delete_rejects_stale_confirmation_statistics() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&stored_item("first-text", ClipboardKind::Text, None, None))
            .unwrap();
        let confirmed = database
            .kind_storage_stats(ClipboardKind::Text, STORAGE_KIND_DELETE_SCOPE)
            .unwrap();
        database
            .save_item(&stored_item("second-text", ClipboardKind::Text, None, None))
            .unwrap();

        let error = database
            .permanently_delete_by_kind_if_stats_match(
                ClipboardKind::Text,
                STORAGE_KIND_DELETE_SCOPE,
                confirmed,
            )
            .unwrap_err();

        assert!(error.to_string().contains("data changed"));
        assert_eq!(database.item_count().unwrap(), 2);
    }

    #[test]
    fn cleanup_resolves_icon_keys_and_removes_only_unreferenced_icons() {
        let paths = temporary_storage("icon-key");
        let icons = paths.storage.join("icons");
        fs::create_dir_all(&icons).unwrap();
        let referenced_icon = icons.join("notepad.png");
        let orphan_icon = icons.join("orphan.png");
        fs::write(&referenced_icon, b"referenced").unwrap();
        fs::write(&orphan_icon, b"orphan").unwrap();
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&stored_item(
                "text",
                ClipboardKind::Text,
                None,
                Some("notepad.png".to_owned()),
            ))
            .unwrap();

        let result = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(result.removed_files, 1);
        assert!(referenced_icon.exists());
        assert!(!orphan_icon.exists());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn scheduled_cleanup_worker_stops_and_joins_cleanly() {
        let paths = temporary_storage("worker-lifecycle");
        let database = Database::open(&paths.database).unwrap();
        let worker = CleanupWorker::start_with_interval(
            paths.project.clone(),
            database,
            paths.clone(),
            Duration::from_millis(5),
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(20));
        worker.stop();

        assert!(worker.stop_flag.load(Ordering::SeqCst));
        assert!(worker
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_none());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn scheduled_cleanup_grace_preserves_recent_orphan_files() {
        let paths = temporary_storage("orphan-grace");
        let orphan = paths.images.join("recent.png");
        fs::write(&orphan, b"recent-data").unwrap();
        let database = Database::open_in_memory().unwrap();

        let scheduled = cleanup_orphan_storage_files_with_grace(
            &database,
            &paths,
            Duration::from_secs(60 * 60),
        )
        .unwrap();
        assert_eq!(scheduled.removed_files, 0);
        assert!(orphan.exists());

        let manual = cleanup_orphan_storage_files(&database, &paths).unwrap();
        assert_eq!(manual.removed_files, 1);
        assert!(!orphan.exists());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn cleanup_preserves_unowned_custom_resource_files() {
        let root = temporary_project("unowned-custom-resource");
        let project = root.join("project");
        let images = root.join("user-images");
        let files = root.join("user-files");
        fs::create_dir_all(&images).unwrap();
        fs::create_dir_all(&files).unwrap();
        let unrelated_image = images.join("family-photo.png");
        let unrelated_file = files.join("report.docx");
        fs::write(&unrelated_image, b"user image").unwrap();
        fs::write(&unrelated_file, b"user document").unwrap();

        let paths = StoragePaths::initialize_with_resource_directories(
            project,
            None,
            Some(images),
            Some(files),
        )
        .unwrap();
        assert!(!paths.image_cleanup_enabled);
        assert!(!paths.file_cleanup_enabled);

        let database = Database::open_in_memory().unwrap();
        let result = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(result.removed_files, 0);
        assert!(unrelated_image.exists());
        assert!(unrelated_file.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_removes_orphans_only_from_marked_custom_resource_roots() {
        let root = temporary_project("marked-custom-resource");
        let project = root.join("project");
        let images = root.join("managed-images");
        let files = root.join("managed-files");
        let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
            project,
            None,
            Some(images.clone()),
            Some(files.clone()),
        )
        .unwrap();
        assert!(paths.image_cleanup_enabled);
        assert!(paths.file_cleanup_enabled);

        let orphan_image = images.join("orphan.png");
        let orphan_file = files.join("orphan.txt");
        fs::write(&orphan_image, b"orphan image").unwrap();
        fs::write(&orphan_file, b"orphan file").unwrap();

        let database = Database::open_in_memory().unwrap();
        let result = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(result.removed_files, 2);
        assert!(!orphan_image.exists());
        assert!(!orphan_file.exists());
        assert!(images.join(storage::RESOURCE_ROOT_MARKER).exists());
        assert!(files.join(storage::RESOURCE_ROOT_MARKER).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn storage_migration_rewrites_every_managed_resource_reference() {
        let project = temporary_project("path-migration");
        let old_paths = StoragePaths::initialize(project.clone()).unwrap();
        let new_paths = StoragePaths::initialize_with_data_directory(
            project.clone(),
            Some(project.join("custom-data")),
        )
        .unwrap();
        let old_icons = old_paths.storage.join("icons");
        fs::create_dir_all(&old_icons).unwrap();

        let image = old_paths.images.join("image.png");
        let preview = old_paths.previews.join("image-preview.png");
        let managed_file = old_paths.files.join("document.txt");
        let icon = old_icons.join("notepad.png");
        let external_file = project.join("outside.txt");
        let search_marker = old_paths.search_index.join("migration-marker");
        for (path, contents) in [
            (&image, b"image".as_slice()),
            (&preview, b"preview".as_slice()),
            (&managed_file, b"managed".as_slice()),
            (&icon, b"icon".as_slice()),
            (&external_file, b"external".as_slice()),
            (&search_marker, b"search-index".as_slice()),
        ] {
            fs::write(path, contents).unwrap();
        }

        let database = Database::open(&old_paths.database).unwrap();
        let mut image_item = stored_item(
            "image",
            ClipboardKind::Image,
            Some(image.to_string_lossy().into_owned()),
            Some(icon.to_string_lossy().into_owned()),
        );
        image_item.preview_path = Some(preview.to_string_lossy().into_owned());
        image_item.metadata_json = Some(r#"{"width":100,"height":80}"#.to_owned());
        database.save_item(&image_item).unwrap();

        let mut file_item = stored_item(
            "file",
            ClipboardKind::File,
            Some(managed_file.to_string_lossy().into_owned()),
            None,
        );
        file_item.text_content = Some(
            serde_json::to_string(&[
                managed_file.to_string_lossy().into_owned(),
                external_file.to_string_lossy().into_owned(),
            ])
            .unwrap(),
        );
        file_item.metadata_json = Some(
            serde_json::json!({
                "files": [{
                    "path": managed_file.to_string_lossy(),
                    "originalPath": external_file.to_string_lossy(),
                    "copied": true,
                }],
            })
            .to_string(),
        );
        database.save_item(&file_item).unwrap();

        migrate_storage_data(&old_paths, &new_paths, &database).unwrap();

        let migrated = Database::open(&new_paths.database).unwrap();
        let migrated_image = migrated.get_item("image").unwrap().unwrap();
        assert_eq!(
            migrated_image.resource_path.as_deref(),
            Some(
                new_paths
                    .images
                    .join("image.png")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            migrated_image.preview_path.as_deref(),
            Some(
                new_paths
                    .previews
                    .join("image-preview.png")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            migrated_image.icon_path.as_deref(),
            Some(
                new_paths
                    .storage
                    .join("icons")
                    .join("notepad.png")
                    .to_string_lossy()
                    .as_ref()
            )
        );

        let migrated_file = migrated.get_item("file").unwrap().unwrap();
        let migrated_paths: Vec<String> =
            serde_json::from_str(migrated_file.text_content.as_deref().unwrap()).unwrap();
        assert_eq!(
            migrated_paths[0],
            new_paths.files.join("document.txt").to_string_lossy()
        );
        assert_eq!(migrated_paths[1], external_file.to_string_lossy());
        let migrated_metadata: serde_json::Value =
            serde_json::from_str(migrated_file.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(
            migrated_metadata["files"][0]["path"],
            new_paths
                .files
                .join("document.txt")
                .to_string_lossy()
                .as_ref()
        );
        assert_eq!(
            migrated_metadata["files"][0]["originalPath"],
            external_file.to_string_lossy().as_ref()
        );
        assert!(new_paths.images.join("image.png").exists());
        assert!(new_paths.files.join("document.txt").exists());
        assert!(new_paths.storage.join("icons").join("notepad.png").exists());
        assert!(
            new_paths.search_index.exists(),
            "search index directory must exist at the target"
        );
        assert!(
            !new_paths.search_index.join("migration-marker").exists(),
            "search index is not copied — it will be rebuilt on restart"
        );

        let original = database.get_item("image").unwrap().unwrap();
        assert_eq!(original.resource_path, image_item.resource_path);
        drop(migrated);
        drop(database);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn storage_migration_skips_paths_that_already_use_the_target_layout() {
        let project = temporary_project("same-layout-migration");
        let old_paths = StoragePaths::initialize(project.clone()).unwrap();
        let database = Database::open(&old_paths.database).unwrap();
        let new_paths = StoragePaths::initialize_with_data_directory(
            project.clone(),
            Some(old_paths.storage.clone()),
        )
        .unwrap();

        assert_ne!(old_paths.data_directory, new_paths.data_directory);
        assert_eq!(old_paths.storage, new_paths.storage);
        migrate_storage_data(&old_paths, &new_paths, &database).unwrap();

        drop(database);
        fs::remove_dir_all(project).unwrap();
    }
}
