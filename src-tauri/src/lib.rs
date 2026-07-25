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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

use cli::{CliArgs, CliCommand, LocalApiServer};
use config::{ConfigStore, GeneralConfig};
use content::{
    ClipboardFormatInfo, ContentMarkers, QuickAction, TextTransform, TransformOperation,
};
use domain::{ClipboardItem, ClipboardKind, OcrResult};
use export::{
    export_database, import_from_json, import_from_plain_text, ExportFormat, ExportOptions,
    ImportSummary,
};
use keyboard::{KeyboardConfig, KeyboardManager};
use ocr::{NoopOcrEngine, OcrEngine, OcrWorker, PpOcrEngine, TesseractOcrEngine};
use performance::{PerformanceSnapshot, PerformanceTracker, StartupMetrics, StartupTimer};
use platform::windows_hotkey::{shortcut_to_windows_hotkey, HotkeyManager};
use platform::{
    ClipboardMonitor, GlobalShortcutManager, RuntimeInfo, SingleInstanceGuard, SystemTray,
    WindowManager,
};
use privacy::PrivacyManager;
use search::{SearchIndex, SearchSyncSummary, SearchSynchronizer, SEARCH_INDEX_VERSION};
use serde::Serialize;
use std::io::Write;
use std::str::FromStr;
use std::time::{Duration, Instant};
use storage::{ClipboardRepository, Database, OcrRepository, RepairResult, StoragePaths};
use tauri::{Emitter, Manager};

const TOGGLE_WINDOW_ACTION: &str = "toggleWindow";

fn resolve_toggle_hotkey(config: &KeyboardConfig) -> Option<(u32, u32)> {
    let shortcuts = config.shortcuts.get(TOGGLE_WINDOW_ACTION)?;
    for shortcut in shortcuts {
        if let Ok(binding) = keyboard::ShortcutBinding::from_str(shortcut) {
            if let Some(hotkey) = shortcut_to_windows_hotkey(&binding) {
                return Some(hotkey);
            }
        }
    }
    None
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
    search_index_path: String,
    search_index_size_bytes: u64,
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

/// Shared state for ignored applications list, synced between capture thread and Tauri commands.
/// Capture policy shared by every clipboard ingestion path.
///
/// The command handlers and background workers do not share a Tauri `State`
/// reference directly.  Instead they hold this small, thread-safe snapshot so
/// pause/ignore changes take effect without restarting a monitor thread.
#[derive(Clone)]
struct CaptureState {
    paused: Arc<AtomicBool>,
    ignored_apps: Arc<Mutex<Vec<String>>>,
    policy: Arc<CapturePolicy>,
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

impl CaptureState {
    fn new(privacy: &PrivacyManager, ignored_apps: Vec<String>) -> Self {
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
            ignored_apps: Arc::new(Mutex::new(normalize_app_list(&ignored_apps))),
            policy: Arc::new(CapturePolicy {
                sensitive_patterns: Arc::new(sensitive_patterns),
                password_manager_apps: Arc::new(privacy.password_manager_apps.clone()),
            }),
            worker: Arc::new(Mutex::new(None)),
        }
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
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
        search_index_path: paths.search_index.display().to_string(),
        search_index_size_bytes: dir_size(&paths.search_index),
        search_index_version: SEARCH_INDEX_VERSION,
        search_index_rebuild_required: search_index.requires_full_rebuild(),
    })
}

#[tauri::command]
fn configure_storage_directory(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    active_paths: tauri::State<'_, StoragePaths>,
    database: tauri::State<'_, Database>,
    data_directory: Option<String>,
) -> Result<StorageDirectoryUpdate, String> {
    let requested_directory = data_directory.map(PathBuf::from);
    let target_paths = StoragePaths::initialize_with_data_directory(
        active_paths.project.clone(),
        requested_directory,
    )
    .map_err(|error| error.to_string())?;

    if target_paths.data_directory != active_paths.data_directory {
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
        (
            old.search_index.clone(),
            new.search_index.clone(),
            "search-index",
        ),
    ];

    for (old_dir, new_dir, label) in dirs_to_migrate {
        if old_dir.exists() {
            copy_dir_contents(old_dir, new_dir)
                .map_err(|e| format!("failed to migrate {}: {}", label, e))?;
        }
    }

    let icons_old = old.storage.join("icons");
    let icons_new = new.storage.join("icons");
    if icons_old.exists() {
        copy_dir_contents(&icons_old, &icons_new)
            .map_err(|e| format!("failed to migrate icons: {}", e))?;
    }

    if old.database.exists() {
        database
            .vacuum_into(&new.database)
            .map_err(|e| format!("failed to migrate database: {}", e))?;
    }

    Ok(())
}

fn copy_dir_contents(from: &PathBuf, to: &PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("create dir: {}", e))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("read dir: {}", e))? {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let dest = to.join(entry.file_name());
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            copy_dir_contents(&entry.path(), &dest)?;
        } else {
            // Skip locked files, continue with others
            if let Err(e) = std::fs::copy(entry.path(), &dest) {
                eprintln!(
                    "[migrate] skip locked file {}: {}",
                    entry.path().display(),
                    e
                );
            }
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
        if let Some((mod_flags, vk)) = resolve_toggle_hotkey(&config) {
            let mut hm = hotkey_manager
                .lock()
                .map_err(|_| "hotkey manager lock is poisoned".to_owned())?;
            hm.restart(mod_flags, vk);
        }
    }

    Ok(normalized)
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
fn get_clipboard_formats(window: tauri::Window) -> Result<ClipboardFormatInfo, String> {
    let _ = window;
    Ok(ClipboardFormatInfo::empty())
}

#[tauri::command]
fn get_ocr_status(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<OcrStatusInfo, String> {
    let pending = database.count_pending_ocr().map_err(|e| e.to_string())?;
    let completed = database.count_completed_ocr().map_err(|e| e.to_string())?;
    let cfg = config
        .lock()
        .map_err(|_| "config lock poisoned".to_owned())?;
    let engine = cfg.ocr_engine().to_string();
    let models_dir = ocr::models::models_dir(&paths.storage);

    Ok(OcrStatusInfo {
        pending_tasks: pending,
        completed_tasks: completed,
        tesseract_available: TesseractOcrEngine::is_available(),
        ppocr_available: ocr::models::all_models_present(&models_dir),
        has_engine: TesseractOcrEngine::is_available()
            || ocr::models::all_models_present(&models_dir),
        engine,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrStatusInfo {
    pending_tasks: u64,
    completed_tasks: u64,
    tesseract_available: bool,
    ppocr_available: bool,
    has_engine: bool,
    engine: String,
}

#[tauri::command]
fn get_ocr_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<OcrConfigResponse, String> {
    let config = config
        .lock()
        .map_err(|_| "config lock poisoned".to_owned())?;
    Ok(OcrConfigResponse {
        engine: config.ocr_engine().to_string(),
        tesseract_languages: config.tesseract_languages().to_string(),
        ppocr_model_variant: config.ppocr_model_variant().to_string(),
        det_score_threshold: config.det_score_threshold(),
        det_box_threshold: config.det_box_threshold(),
        det_unclip_ratio: config.det_unclip_ratio(),
    })
}

#[tauri::command]
fn set_ocr_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    engine: String,
    det_score_threshold: Option<f32>,
    det_box_threshold: Option<f32>,
    det_unclip_ratio: Option<f32>,
) -> Result<(), String> {
    let mut cfg = config
        .lock()
        .map_err(|_| "config lock poisoned".to_owned())?;
    cfg.set_ocr_engine(engine).map_err(|e| e.to_string())?;
    let score = det_score_threshold.unwrap_or_else(|| cfg.det_score_threshold());
    let box_t = det_box_threshold.unwrap_or_else(|| cfg.det_box_threshold());
    let unclip = det_unclip_ratio.unwrap_or_else(|| cfg.det_unclip_ratio());
    cfg.set_det_thresholds(score, box_t, unclip)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn restart_ocr_engine(
    app: tauri::AppHandle,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
    _old_worker: tauri::State<'_, OcrWorker>,
) -> Result<(), String> {
    // Stop old worker
    _old_worker.stop();

    let cfg = config.lock().map_err(|_| "config lock".to_owned())?;
    let ocr_engine_name = cfg.ocr_engine().to_string();
    let langs = cfg.tesseract_languages().to_string();
    let score_threshold = cfg.det_score_threshold();
    let box_threshold = cfg.det_box_threshold();
    let unclip_ratio = cfg.det_unclip_ratio();
    drop(cfg);

    let engine: Arc<dyn OcrEngine> = if ocr_engine_name == "ppocr" {
        let ppocr = PpOcrEngine::new(
            ocr::models::models_dir(&paths.storage),
            score_threshold,
            box_threshold,
            unclip_ratio,
        );
        if ppocr.is_available() {
            Arc::new(ppocr)
        } else {
            return Err("PP-OCR models not downloaded".to_string());
        }
    } else if ocr_engine_name == "tesseract" && TesseractOcrEngine::is_available() {
        Arc::new(TesseractOcrEngine::with_languages(langs))
    } else if TesseractOcrEngine::is_available() {
        Arc::new(TesseractOcrEngine::with_languages("chi_sim"))
    } else {
        return Err("no OCR engine available".to_string());
    };

    let database = Database::open(&paths.database).map_err(|e| e.to_string())?;
    let new_worker = OcrWorker::start(engine, Arc::new(database));
    // Store new worker by replacing app state - use manage to add after removing
    // Tauri 2 doesn't support replacing state directly, so we'll use the old worker's clone mechanism
    // The old worker was stopped above; the new one will take over
    let _ = app.manage(new_worker);
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

#[tauri::command]
async fn install_ppocr(
    app: tauri::AppHandle,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    variant: String,
) -> Result<String, String> {
    let models_dir = ocr::models::models_dir(&paths.storage);
    std::fs::create_dir_all(&models_dir).map_err(|e| e.to_string())?;
    if ocr::models::all_models_present(&models_dir) {
        let mut cfg = config
            .lock()
            .map_err(|_| "config lock poisoned".to_owned())?;
        let _ = cfg.set_ppocr_model_variant(variant.to_string());
        return Ok("PP-OCR models already installed".to_string());
    }
    let det_url = match variant.as_str() {
        "tiny" => "https://github.com/hiroi-sora/pp-ocrv6-onnx/releases/download/v1.0/pp-ocrv6_tiny_det.onnx",
        "medium" => "https://github.com/hiroi-sora/pp-ocrv6-onnx/releases/download/v1.0/pp-ocrv6_medium_det.onnx",
        _ => "https://github.com/hiroi-sora/pp-ocrv6-onnx/releases/download/v1.0/pp-ocrv6_small_det.onnx",
    };
    let rec_url = match variant.as_str() {
        "tiny" => "https://github.com/hiroi-sora/pp-ocrv6-onnx/releases/download/v1.0/pp-ocrv6_tiny_rec.onnx",
        "medium" => "https://github.com/hiroi-sora/pp-ocrv6-onnx/releases/download/v1.0/pp-ocrv6_medium_rec.onnx",
        _ => "https://github.com/hiroi-sora/pp-ocrv6-onnx/releases/download/v1.0/pp-ocrv6_small_rec.onnx",
    };
    let dict_url =
        "https://raw.githubusercontent.com/hiroi-sora/pp-ocrv6-onnx/main/ppocrv6_dict.txt";

    let files = [
        (det_url, "pp-ocrv6_small_det.onnx", "检测模型"),
        (rec_url, "pp-ocrv6_small_rec.onnx", "识别模型"),
        (dict_url, "ppocrv6_dict.txt", "字典文件"),
    ];

    let client = reqwest::Client::builder()
        .user_agent("clipboard-desktop")
        .build()
        .map_err(|e| format!("create client: {e}"))?;

    for (url, filename, label) in &files {
        let dest = models_dir.join(filename);
        if dest.exists() {
            continue;
        }

        let _ = app.emit(
            "ppocr-download-progress",
            PpOcrDownloadProgress {
                filename: filename.to_string(),
                label: label.to_string(),
                current: 0,
                total: 0,
                percentage: 0.0,
            },
        );

        let mut response = client
            .get(*url)
            .send()
            .await
            .map_err(|e| format!("download {filename}: {e}"))?;
        let total = response.content_length().unwrap_or(0);
        let mut file = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        let mut downloaded: u64 = 0;

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("download {filename}: {e}"))?
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
                    filename: filename.to_string(),
                    label: label.to_string(),
                    current: downloaded,
                    total,
                    percentage,
                },
            );
        }
    }

    let mut cfg = config
        .lock()
        .map_err(|_| "config lock poisoned".to_owned())?;
    cfg.set_ppocr_model_variant(variant)
        .map_err(|e| e.to_string())?;
    drop(cfg);

    Ok("PP-OCRv6 models downloaded successfully".to_string())
}

#[tauri::command]
fn check_ppocr_status(paths: tauri::State<'_, StoragePaths>) -> Result<PpOcrStatus, String> {
    let models_dir = ocr::models::models_dir(&paths.storage);
    Ok(PpOcrStatus {
        available: ocr::models::all_models_present(&models_dir),
        tesseract_available: TesseractOcrEngine::is_available(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PpOcrStatus {
    available: bool,
    tesseract_available: bool,
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
    Ok(saved)
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
    })
}

#[tauri::command]
fn set_storage_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    max_file_copy_size_bytes: Option<u64>,
) -> Result<(), String> {
    let mut config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    if let Some(v) = max_file_copy_size_bytes {
        config
            .set_max_file_copy_size_bytes(v)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
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
    let content_hash = content::hash::compute_content_hash(kind_name, &new_text_content, None);
    let size_bytes = new_text_content.len() as u64;

    database
        .update_text_item(
            &id,
            item.kind,
            &new_title,
            &new_text_content,
            &content_hash,
            size_bytes,
        )
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
    let (retention_days, max_items, recycle_bin_days) = {
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        (
            guard.retention_days(),
            guard.max_items(),
            guard.recycle_bin_days(),
        )
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

    let _ = cleanup_orphan_storage_files(&database, &paths);

    Ok(total_deleted)
}

fn cleanup_orphan_storage_files(
    database: &Database,
    paths: &StoragePaths,
) -> Result<StorageCleanupResult, String> {
    let active_paths: std::collections::HashSet<String> = database
        .list_active_file_paths()
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect();

    let mut removed_files = 0u64;
    let mut freed_bytes = 0u64;

    let scan_dirs: &[&std::path::Path] = &[
        &paths.images,
        &paths.previews,
        &paths.files,
        &paths.storage.join("icons"),
    ];

    for dir in scan_dirs {
        if !dir.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                continue;
            }
            let path_str = entry_path.to_string_lossy().to_string();
            if active_paths.contains(&path_str) {
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                freed_bytes += metadata.len();
            }
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
                    Ok(_change) => {
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

                        if self_trigger_guard
                            .lock()
                            .unwrap()
                            .is_self_triggered(&content_hash)
                        {
                            self_trigger_guard
                                .lock()
                                .unwrap()
                                .mark_as_self_triggered(&content_hash);
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
                            metadata_json: None,
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
    let hash = content::hash::compute_content_hash("text", &text, None);
    self_trigger
        .0
        .lock()
        .map_err(|_| "self-trigger lock poisoned".to_owned())?
        .mark_as_self_triggered(&hash);
    Ok(())
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
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<Option<WindowPosition>, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    Ok(
        WindowManager::restore_position(&config).map(|(x, y, w, h)| WindowPosition {
            x,
            y,
            width: w,
            height: h,
        }),
    )
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
fn repair_database(database: tauri::State<'_, Database>) -> Result<RepairResult, String> {
    database.repair().map_err(|e| e.to_string())
}

#[tauri::command]
fn validate_search_index(search_index: tauri::State<'_, SearchIndex>) -> Result<bool, String> {
    Ok(search_index.validate())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let startup_timer = &mut StartupTimer::start();
            let project_directory = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| app.path().app_data_dir().unwrap_or_default());
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

            let ocr_engine_name = config.ocr_engine().to_string();
            let models_dir = ocr::models::models_dir(&paths.storage);
            let ppocr_ready = ocr::models::all_models_present(&models_dir);
            let score_threshold = config.det_score_threshold();
            let box_threshold = config.det_box_threshold();
            let unclip_ratio = config.det_unclip_ratio();

            let ocr_engine: Arc<dyn OcrEngine> = if ocr_engine_name == "ppocr" && ppocr_ready {
                Arc::new(PpOcrEngine::new(models_dir, score_threshold, box_threshold, unclip_ratio))
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

            // Auto-start clipboard monitoring in background
            let app_handle = app.handle().clone();
            let db_path = paths.database.clone();
            let storage_path = paths.storage.clone();
            let initial_ignored = config.ignored_applications().to_vec();
            let capture_state = CaptureState::new(&privacy_manager, initial_ignored.clone());
            app.manage(capture_state.clone());

            let self_trigger_guard = Arc::new(Mutex::new(content::hash::SelfTriggerGuard::new()));
            let self_trigger_guard_managed = SelfTriggerState(self_trigger_guard.clone());
            app.manage(self_trigger_guard_managed);

            // Graceful shutdown handler
            let ocr_worker_for_shutdown = ocr_worker.clone();
            let capture_for_shutdown = capture_state.clone();
            let paths_for_shutdown = paths.clone();
            ctrlc::set_handler(move || {
                eprintln!("[shutdown] received interrupt signal, cleaning up...");
                capture_for_shutdown.stop_worker();
                ocr_worker_for_shutdown.stop();
                // Config is auto-saved on drop
                let _ = paths_for_shutdown;
                std::process::exit(0);
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
                            Ok(_change) => {
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
                                    let img_hash = content::hash::compute_media_hash("image", &img);
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

                                    let metadata = serde_json::json!({
                                        "width": img_width,
                                        "height": img_height,
                                    });

                                    let item = ClipboardItem {
                                        id: format!("img_{}", img_hash),
                                        kind: ClipboardKind::Image,
                                        title: img_path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
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
                                        metadata_json: Some(metadata.to_string()),
                                    };

                                    if stop_flag_for_thread.load(Ordering::SeqCst) {
                                        break;
                                    }
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
                                    if stop_flag_for_thread.load(Ordering::SeqCst) {
                                        break;
                                    }
                                    let now_ms = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as i64;

                                    if file_paths.len() == 1 {
                                        let file_path = &file_paths[0];
                                        let file_hash = content::hash::compute_content_hash("file", file_path, None);
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
                                            title: file_name,
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
                                            metadata_json: None,
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
                                        let mut sorted_paths = file_paths.clone();
                                        sorted_paths.sort();
                                        let joined = sorted_paths.join("\n");
                                        let group_hash = content::hash::compute_content_hash("files", &joined, None);

                                        let total_size: u64 = file_paths
                                            .iter()
                                            .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
                                            .sum();

                                        let first_name = std::path::Path::new(&file_paths[0])
                                            .file_name()
                                            .map(|n| n.to_string_lossy().to_string())
                                            .unwrap_or_default();

                                        let file_sizes: Vec<(String, u64)> = file_paths
                                            .iter()
                                            .map(|p| {
                                                let name = std::path::Path::new(p)
                                                    .file_name()
                                                    .map(|n| n.to_string_lossy().to_string())
                                                    .unwrap_or_default();
                                                let size = std::fs::metadata(p)
                                                    .map(|m| m.len())
                                                    .unwrap_or(0);
                                                (name, size)
                                            })
                                            .collect();

                                        let paths_json = serde_json::to_string(&file_paths).unwrap_or_default();

                                        let metadata = serde_json::json!({
                                            "files": file_sizes.iter().map(|(name, size)| {
                                                serde_json::json!({ "name": name, "size": size })
                                            }).collect::<Vec<_>>(),
                                        });

                                        let item = ClipboardItem {
                                            id: format!("files_{}", group_hash),
                                            kind: ClipboardKind::File,
                                            title: first_name,
                                            text_content: Some(paths_json),
                                            resource_path: Some(file_paths[0].clone()),
                                            preview_path: None,
                                            content_hash: group_hash,
                                            source_app: source_app.clone(),
                                            icon_path: icon_path.clone(),
                                            size_bytes: total_size,
                                            created_at_ms: now_ms,
                                            last_used_at_ms: None,
                                            is_favorite: false,
                                            metadata_json: Some(metadata.to_string()),
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

                                if self_trigger_guard.lock().unwrap().is_self_triggered(&content_hash) {
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
                                    metadata_json: None,
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


            app.manage(Mutex::new(config));
            app.manage(paths);
            app.manage(database);
            app.manage(search_index);
            app.manage(performance_tracker);
            app.manage(Mutex::new(privacy_manager));
            app.manage(Mutex::new(clipboard_monitor));
            app.manage(Mutex::new(shortcut_manager));
            app.manage(ocr_worker);
            app.manage(Mutex::new(LocalApiServer::new(0)));

            let _tray = SystemTray::create().ok();

            // Register global hotkey from keyboard config
            let mut hotkey_manager = HotkeyManager::new();
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                let kb_config = keyboard.config();
                if let Some((mod_flags, vk)) = resolve_toggle_hotkey(&kb_config) {
                    hotkey_manager.start_with_window(mod_flags, vk, window.clone());
                } else {
                    eprintln!("[hotkey] no valid toggleWindow shortcut found in config, using default Alt+V");
                    use platform::windows_clipboard;
                    hotkey_manager.start_with_window(windows_clipboard::MOD_ALT, windows_clipboard::VK_V, window.clone());
                }
            }
            app.manage(Mutex::new(keyboard));
            app.manage(Mutex::new(hotkey_manager));

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
            batch_set_favorite,
            delete_clipboard_item,
            batch_delete_clipboard_items,
            get_clipboard_item_ocr,
            list_source_applications,
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
            open_external_url,
            reveal_in_explorer,
            copy_file_to,
            rename_item,
            update_clipboard_text,
            set_history_config,
            get_history_config,
            set_storage_config,
            get_storage_config,
            detect_content_actions,
            soft_delete_clipboard_item,
            restore_clipboard_item,
            list_deleted_clipboard_items,
            batch_restore_clipboard_items,
            permanently_delete_clipboard_item,
            batch_permanently_delete_clipboard_items,
            duplicate_clipboard_item,
            enforce_history_cleanup,
            clear_all_non_favorite_items,
            get_performance_metrics,
            repair_database,
            validate_search_index,
            cleanup_storage_files,
            restart_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    fn capture_state() -> CaptureState {
        CaptureState::new(&PrivacyManager::new(), vec!["IgnoredApp".to_owned()])
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
    fn invalid_sensitive_patterns_are_excluded_during_initialization() {
        let mut privacy = PrivacyManager::new();
        privacy.sensitive_patterns = vec!["[invalid".to_owned(), "secret".to_owned()];
        let state = CaptureState::new(&privacy, Vec::new());

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
