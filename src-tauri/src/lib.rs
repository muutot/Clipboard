pub mod cli;
pub mod commands;
pub mod item_operations;

/// Legacy quick-paste debug trace. Deliberately compiled out of release
/// builds: an unconditionally appended, unbounded file in the user's temp
/// directory is both a growth hazard and an unwanted diagnostic surface.
#[allow(unused_variables)]
pub fn dbg_log(msg: &str) {
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        let path = std::env::temp_dir().join("paste_debug.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{}", msg);
        }
    }
}
use commands::capture::*;
use commands::clipboard::*;
use commands::config::*;
use commands::export::*;
use commands::float::*;
use commands::ocr::*;
use commands::sync::*;
use commands::system::*;
use commands::update::*;
pub mod config;
pub mod content;
pub mod domain;
pub mod export;
pub mod geometry;
pub mod keyboard;
pub mod logging;
pub mod memory;
pub mod ocr;
pub mod performance;
pub mod platform;
pub mod privacy;
pub mod search;
pub mod shutdown;
pub mod state;
pub mod storage;
pub mod sync;
pub mod tags;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use cli::LocalApiServer;
use commands::clipboard::SearchResultCache;
use config::{ConfigStore, SearchIndexSyncMode};
use content::{self_trigger, ThumbnailWorker};
use keyboard::{KeyboardConfig, KeyboardManager, GLOBAL_HOTKEY_ACTIONS};
use ocr::{NoopOcrEngine, OcrEngine, OcrWorkerManager, PpOcrEngine, TesseractOcrEngine};
use performance::{PerformanceTracker, StartupMetrics, StartupTimer};
use platform::hotkey_common::{
    shortcut_bindings_to_double_modifiers, shortcut_bindings_to_windows_hotkeys,
};
use platform::windows::hotkey::HotkeyManager;
use platform::{
    show_main_window, sync_autostart, ClipboardMonitor, SingleInstanceError, SingleInstanceGuard,
    SystemTray,
};
use privacy::PrivacyManager;
use search::{SearchIndex, SearchSyncWorker, SearchSynchronizer};
use shutdown::stop_runtime_services;
use state::{CaptureState, CaptureWorker, SelfTriggerState};
use storage::{
    discard_database_backups, discard_database_quarantine, quarantine_search_index,
    recover_database_if_needed, refresh_database_backup, Database, KindDeleteScope, OcrRepository,
    StoragePaths,
};
use tauri::{Emitter, Manager};
mod background_operations;

pub(crate) const STORAGE_KIND_DELETE_SCOPE: KindDeleteScope = KindDeleteScope {
    include_favorites: false,
    include_deleted: true,
};
#[cfg_attr(not(test), allow(dead_code))]
const MAIN_WINDOW_MIN_WIDTH: u32 = 730;
#[cfg_attr(not(test), allow(dead_code))]
const MAIN_WINDOW_MIN_HEIGHT: u32 = 500;

const HISTORY_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Directory holding config, logs, the database and managed storage.
/// Portable installs keep data next to the executable while that directory is
/// writable; otherwise (AppImages, which mount read-only, system install
/// locations, read-only portable folders) fall back to the platform data
/// directory so the app can still start.
pub fn resolve_project_directory() -> PathBuf {
    let executable_directory = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()));
    if let Some(directory) = &executable_directory {
        if directory_is_writable(directory) {
            return directory.clone();
        }
    }
    platform_data_directory()
        .or(executable_directory)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn directory_is_writable(directory: &Path) -> bool {
    if std::fs::create_dir_all(directory).is_err() {
        return false;
    }
    let probe = directory.join(format!(".clipboard-write-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn platform_data_directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(|base| PathBuf::from(base).join("com.clipboard.desktop"))
            .or_else(|| {
                std::env::var_os("LOCALAPPDATA")
                    .map(|base| PathBuf::from(base).join("com.clipboard.desktop"))
            })
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("com.clipboard.desktop")
        })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("com.clipboard.desktop"));
            }
        }
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".local/share/com.clipboard.desktop"))
    }
}

pub(crate) struct CleanupWorker {
    stop_flag: Arc<AtomicBool>,
    stop_sender: Mutex<Option<mpsc::Sender<()>>>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl CleanupWorker {
    fn start(
        project_directory: PathBuf,
        database: Database,
        paths: StoragePaths,
        storage_maintenance_lock: Arc<Mutex<()>>,
    ) -> Result<Self, String> {
        Self::start_with_interval(
            project_directory,
            database,
            paths,
            storage_maintenance_lock,
            HISTORY_CLEANUP_INTERVAL,
        )
    }

    fn start_with_interval(
        project_directory: PathBuf,
        database: Database,
        paths: StoragePaths,
        storage_maintenance_lock: Arc<Mutex<()>>,
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
                    Ok(config) => {
                        // Hold the storage maintenance lock for the whole run:
                        // the orphan scan inside must not race a concurrent
                        // rename's DB-first-then-move sequence, or it can
                        // delete the not-yet-moved old file and leave the
                        // rolled-back record pointing at a missing file.
                        let _maintenance = storage_maintenance_lock
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        match commands::cleanup::enforce_history_cleanup_for(
                            &database,
                            &config,
                            &paths,
                            commands::cleanup::ORPHAN_FILE_GRACE,
                        ) {
                            Ok(total_deleted) if total_deleted > 0 => {
                                crate::log_event!(
                                    "[cleanup] removed {total_deleted} expired history entries"
                                );
                            }
                            Ok(_) => {}
                            Err(error) => {
                                crate::log_error!("[cleanup] scheduled cleanup failed: {error}")
                            }
                        }
                    }
                    Err(error) => {
                        crate::log_error!("[cleanup] failed to load configuration: {error}")
                    }
                }

                if commands::signal::wait_for_stop(&stop_receiver, &worker_flag, interval) {
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
            if handle.thread().id() == thread::current().id() {
                return;
            }
            // A cleanup sweep on a large library can take minutes; do not
            // hang shutdown on it. Give the thread a short grace window and
            // detach when it exceeds it.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !handle.is_finished() && std::time::Instant::now() < deadline {
                thread::sleep(Duration::from_millis(50));
            }
            if handle.is_finished() {
                if handle.join().is_err() {
                    crate::log_error!("[cleanup] history cleanup thread terminated with a panic");
                }
            } else {
                crate::log_error!(
                    "[cleanup] history cleanup thread did not finish within 5s of stop; detaching"
                );
            }
        }
    }
}

impl Drop for CleanupWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Builds the OS registration plan for every global registry action, in
/// `global_action_ids()` order: one chord list per action plus the
/// toggle-only double-modifier list. Invalid chords are skipped per action
/// (`keyboard::action_bindings`), so one bad binding can never break the
/// whole plan. A new global shortcut needs no changes here — only one row in
/// `keyboard::GLOBAL_HOTKEY_ACTIONS` (+ its `keyboard-defaults.json` default).
fn resolve_global_hotkey_plan(
    config: &KeyboardConfig,
) -> (Vec<Vec<(u32, u32)>>, Vec<keyboard::Modifier>) {
    use keyboard::{action_bindings, ActionScope};

    let mut chords = Vec::with_capacity(GLOBAL_HOTKEY_ACTIONS.len());
    let mut double_modifiers = Vec::new();
    for def in GLOBAL_HOTKEY_ACTIONS
        .iter()
        .filter(|def| def.scope == ActionScope::Global)
    {
        let bindings = action_bindings(config, def.id);
        chords.push(shortcut_bindings_to_windows_hotkeys(&bindings));
        if def.allow_double_tap {
            double_modifiers.extend(shortcut_bindings_to_double_modifiers(&bindings));
        }
    }
    (chords, double_modifiers)
}

/// Re-registers OS hotkeys for every global action from the current keyboard
/// config in a single rebuild. Also fixes a latent gap where deleting a
/// binding left a stale registration until restart.
pub(crate) fn refresh_hotkey_registrations(
    keyboard: &Mutex<KeyboardManager>,
    hotkey_manager: &Mutex<HotkeyManager>,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let config = keyboard
        .lock()
        .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
        .config();
    let (chords, double_modifiers) = resolve_global_hotkey_plan(&config);
    let mut manager = hotkey_manager
        .lock()
        .map_err(|_| "hotkey manager lock is poisoned".to_owned())?;
    if chords.iter().all(Vec::is_empty) && double_modifiers.is_empty() {
        manager.stop();
    } else {
        manager.apply_global_plan(chords, double_modifiers, app.clone());
    }
    Ok(())
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
            let project_directory = resolve_project_directory();
            // Redirect stderr into a rotating log file as early as possible so
            // every eprintln! diagnostic survives in GUI-subsystem builds.
            if let Some(log_path) = logging::init(&project_directory, crate::logging::LogLevel::Info)
            {
                crate::log_event!("[logging] writing diagnostics to {}", log_path.display());
            }
            let mut config = ConfigStore::load(&project_directory)?;
            logging::set_level(config.log_level());
            if config.single_instance() {
                let mut guard = match SingleInstanceGuard::acquire(&project_directory) {
                    Ok(guard) => guard,
                    Err(error) => {
                        if let SingleInstanceError::AlreadyRunning(owner_pid) = &error {
                            if !SingleInstanceGuard::notify_existing_instance(
                                &project_directory,
                                *owner_pid,
                            ) {
                                crate::log_error!(
                                    "[single-instance] failed to notify the existing process (PID: {owner_pid})"
                                );
                            }
                        }
                        return Err(error.into());
                    }
                };
                let app_handle = app.handle().clone();
                guard.start_wake_listener(move || {
                    let app = app_handle.clone();
                    // Teardown may join the IPC thread from the UI thread.
                    // Post UI work rather than waiting for it here.
                    let _ = app_handle.run_on_main_thread(move || show_main_window(&app));
                })?;
                app.manage(guard);
            }
            let keyboard = KeyboardManager::load(&project_directory)?;
            let paths = StoragePaths::initialize_with_resource_directories(
                project_directory.clone(),
                config.storage_directory().map(PathBuf::from),
                config.image_storage_path().map(PathBuf::from),
                config.file_storage_path().map(PathBuf::from),
            )?;
            // The static asset scope in tauri.conf.json is empty on purpose:
            // `$EXE` and `$RESOURCE` both resolve to the install directory on
            // Windows, which also holds `conf/api.token`, `conf/conf.json`,
            // and the clipboard database. Grant the webview read access only
            // to the managed storage root at runtime — it follows the user's
            // configured data directory, including after it changes — so image
            // previews, file icons, and resource files render while arbitrary
            // disk paths (and the app's own secrets) stay unreachable.
            let asset_scope = app.asset_protocol_scope();
            asset_scope
                .allow_directory(&paths.storage, true)
                .map_err(|error| -> String { format!("failed to grant asset access: {error}") })?;
            // Custom image/file roots may live outside `storage`; grant them
            // too or every preview/thumbnail/icon request for those records is
            // rejected by the webview asset protocol.
            for resource_root in [&paths.images, &paths.files] {
                asset_scope
                    .allow_directory(resource_root, true)
                    .map_err(|error| -> String { format!("failed to grant asset access: {error}") })?;
            }
            let recovery_report = recover_database_if_needed(&paths.database)?;
            if let Some(report) = &recovery_report {
                crate::log_warn!(
                    "[recovery] restored database from {}",
                    report.restored_from.display()
                );
                if let Some(quarantined_database) = &report.quarantined_database {
                    crate::log_warn!(
                        "[recovery] quarantined damaged database at {}",
                        quarantined_database.display()
                    );
                }
            }
            let database = Database::open(&paths.database)?;

            if database.schema_was_reset() {
                crate::log_event!("[storage] initialized schema v1; reset derived and recovery state");
                discard_database_backups(&paths.database)?;
                if let Some(quarantined_database) = recovery_report
                    .as_ref()
                    .and_then(|report| report.quarantined_database.as_deref())
                {
                    discard_database_quarantine(quarantined_database)?;
                }
                storage::reset_search_index(&paths.search_index)?;
            } else if recovery_report.is_some() {
                if let Some(quarantined_index) = quarantine_search_index(&paths.search_index)? {
                    crate::log_warn!(
                        "[recovery] quarantined stale search index at {}",
                        quarantined_index.display()
                    );
                }
            }

            let repair_result = database.repair()?;
            if !repair_result.integrity_ok {
                return Err(storage::StorageError::DatabaseRecoveryUnavailable {
                    database: paths.database.clone(),
                    reason: repair_result.integrity_message,
                }
                .into());
            }
            if let Err(error) = refresh_database_backup(&database, &paths.database) {
                crate::log_error!("[recovery] failed to refresh database backup: {error}");
            }

            database.requeue_interrupted_ocr()?;

            // Database::open creates the persistent sync UUID before any
            // capture worker can produce v1 outbox entries.
            match database.get_sync_device_id() {
                Ok(device_id) => crate::log_event!("[sync] device_id = {device_id}"),
                Err(error) => crate::log_error!("[sync] failed to read device_id: {error}"),
            }

            let db_open_duration = startup_timer.finish_segment();

            let search_index = Arc::new(SearchIndex::open(&paths.search_index)?);
            // `SearchSynchronizer::initialize` rebuilds the index when
            // `SearchIndexLayout` flags it as `rebuild_required`; the previous
            // `validate()` call here was a no-op (always returned `true`) and
            // has been removed. Use the `validate_search_index` Tauri command
            // for an on-demand health probe instead.
            if let Err(error) = SearchSynchronizer::default().initialize(&database, &search_index) {
                // A failed initial sync leaves this session's search results
                // empty (the manifest retries on the next start), so at least
                // leave a diagnostic trail instead of failing silently.
                crate::log_error!("[search] initial index sync failed: {error}");
            }
            let search_init_duration = startup_timer.finish_segment();

            let startup_metrics = StartupMetrics {
                total_startup_ms: db_open_duration.as_millis() as u64
                    + search_init_duration.as_millis() as u64,
                db_open_ms: db_open_duration.as_millis() as u64,
                search_init_ms: search_init_duration.as_millis() as u64,
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
                let fallback_languages = config.tesseract_languages().to_string();
                crate::log_warn!("[ocr] falling back to Tesseract ({fallback_languages})");
                Arc::new(TesseractOcrEngine::with_languages(fallback_languages))
            } else {
                crate::log_warn!("[ocr] no OCR engine available");
                Arc::new(NoopOcrEngine)
            };
            let ocr_database = Database::open(&paths.database)?;
            let ocr_worker = OcrWorkerManager::start(ocr_engine, Arc::new(ocr_database))?;

            let thumbnail_database = Database::open(&paths.database)?;
            let thumbnail_worker =
                ThumbnailWorker::start(paths.previews.clone(), Arc::new(thumbnail_database))?;
            let thumbnail_queue = thumbnail_worker.queue();

            let mut privacy_manager = PrivacyManager::new();
            privacy_manager.sync_with_config(&config);
            let mut clipboard_monitor = ClipboardMonitor::new();

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
                config.max_text_capture_bytes(),
            );
            capture_state.set_auto_tag_rules(crate::tags::compile_auto_tag_rules(
                config.auto_tag_rules(),
            ));
            app.manage(capture_state.clone());

            let self_trigger_guard = Arc::new(Mutex::new(self_trigger::SelfTriggerGuard::new()));
            let self_trigger_guard_managed = SelfTriggerState(self_trigger_guard.clone());
            app.manage(self_trigger_guard_managed);

            // Route interrupt signals through the Tauri event loop so every
            // runtime service follows the same shutdown path.
            let app_handle_for_shutdown = app.handle().clone();
            if let Err(error) = ctrlc::set_handler(move || {
                crate::log_event!("[shutdown] received interrupt signal");
                app_handle_for_shutdown.exit(0);
            }) {
                // A foreign handler that installed first would otherwise send
                // Ctrl+C straight to process exit, bypassing the unified stop.
                crate::log_warn!(
                    "[shutdown] failed to install the interrupt handler; Ctrl+C will not run the unified shutdown: {error}"
                );
            }

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
                                    crate::log_error!("[clipboard-worker] failed to open database: {e}");
                                    return;
                                }
                            };

                            // The full ingestion loop (text/html, image, files,
                            // OCR/thumbnail enqueue, `clipboard-item-added`
                            // emission) lives in the shared helper so the
                            // startup path and `start_clipboard_monitoring`
                            // command stay in lock-step.
                            commands::capture::run_capture_loop(
                                receiver,
                                database,
                                capture_for_thread,
                                self_trigger_clone,
                                stop_flag_for_thread,
                                stop_receiver,
                                storage_path,
                                image_storage_path,
                                file_storage_path,
                                thumbnail_queue,
                                app_handle,
                            );
                        })
                        .map_err(|error| format!("failed to start startup clipboard worker: {error}"))?;

                    capture_state.install_worker(CaptureWorker {
                        stop_flag,
                        stop_sender: Some(stop_sender),
                        handle: Some(handle),
                    });
            } else {
                crate::log_warn!("[startup] clipboard monitor has no receiver");
            }
        } else {
            crate::log_error!("[startup] failed to start clipboard monitor");
        }

            let cleanup_database = Database::open(&paths.database)?;
            let cleanup_worker = CleanupWorker::start(
                project_directory.clone(),
                cleanup_database,
                paths.clone(),
                Arc::clone(&capture_state.storage_maintenance_lock),
            )?;

            // Optional background search-index synchronizer. In `Lazy` mode the
            // search command drains the outbox itself; in `Background` mode this
            // worker keeps the Tantivy index fresh off the hot path so queries
            // never block on indexing. Settings can replace this worker live.
            let search_sync_worker: Option<SearchSyncWorker> =
                if config.search_index_sync_mode() == SearchIndexSyncMode::Background {
                    let sync_database = Database::open(&paths.database)?;
                    let sync_index = search_index.clone();
                    let app_for_sync = app.handle().clone();
                    let on_changes_applied = Arc::new(move || {
                        if let Some(cache) = app_for_sync.try_state::<SearchResultCache>() {
                            cache.clear();
                            if let Err(error) = app_for_sync.emit("search-index-changed", ()) {
                                crate::log_warn!("[search] unable to announce index changes: {error}");
                            }
                        }
                    });
                    match SearchSyncWorker::start(
                        sync_database,
                        sync_index,
                        Duration::from_millis(500),
                        on_changes_applied,
                    ) {
                        Ok(worker) => {
                            crate::log_event!("[search-sync] background synchronizer started");
                            Some(worker)
                        }
                        Err(error) => {
                            crate::log_error!("[search-sync] failed to start background synchronizer: {error}");
                            // Degrade to Lazy draining so the search command
                            // keeps the index fresh instead of serving stale
                            // results for the whole session.
                            config.config.general.search_index_sync_mode =
                                "lazy".to_owned();
                            None
                        }
                    }
                } else {
                    None
                };

            let launch_at_startup = config.launch_at_startup();
            // The native layered alpha fades the whole window including text, so
            // it must only be applied when text is meant to follow the window
            // transparency. Otherwise the window stays in its per-pixel
            // transparent state and the CSS backdrop translucency composites
            // over the desktop.
            let startup_transparency = config
                .general_settings()
                .window_opacity_affects_text
                .then(|| config.general_settings().window_transparency);
            let startup_window_effect = config.general_settings().window_effect.clone();
            app.manage(Mutex::new(config));
            app.manage(paths);
            app.manage(database);
            app.manage(Mutex::new(item_operations::ExternalChangeWorker::start(app.handle().clone())?));
            app.manage(search_index);
            app.manage(performance_tracker);
            app.manage(SearchResultCache::new());
            app.manage(Mutex::new(search_sync_worker));
            app.manage(Mutex::new(privacy_manager));
            app.manage(Mutex::new(clipboard_monitor));
            app.manage(ocr_worker);
            app.manage(Mutex::new(thumbnail_worker));
            app.manage(Mutex::new(cleanup_worker));
            app.manage(Mutex::new(LocalApiServer::new(0)));

            // Background auto-sync worker. It re-reads `auto_sync` and
            // `auto_sync_interval_secs` from the managed config each tick, so
            // toggling the setting in the UI takes effect without a restart.
            app.manage(commands::sync::SyncCancellation::default());
            app.manage(background_operations::BackgroundOperations::default());
            let auto_sync_worker = match commands::sync::AutoSyncWorker::start(app.handle().clone())
            {
                Ok(worker) => {
                    crate::log_event!("[auto-sync] background worker started");
                    Some(worker)
                }
                Err(error) => {
                    crate::log_error!("[auto-sync] failed to start background worker: {error}");
                    None
                }
            };
            app.manage(Mutex::new(auto_sync_worker));

            if let Err(error) = sync_autostart(app.handle(), launch_at_startup) {
                crate::log_error!("[autostart] failed to synchronize startup registration: {error}");
            }

            SystemTray::create(app.handle())?;
            // Populate the tray "recent" submenu once the database is managed.
            crate::platform::refresh_tray_recent_menu(app.handle());

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
                                crate::log_error!(
                                    "[tray] configuration lock is poisoned; allowing window close"
                                );
                                false
                            }
                        };

                        if close_to_tray {
                            api.prevent_close();
                            if let Err(error) = window_to_hide.hide() {
                                crate::log_error!("[tray] failed to hide the main window: {error}");
                            }
                        }
                    }
                });
            }

            // Register global hotkeys from the keyboard config in one rebuild:
            // every registry action shares the same message loop, and an empty
            // chord list simply registers nothing for that action.
            #[allow(unused_mut)]
            let mut hotkey_manager = HotkeyManager::new();
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                let kb_config = keyboard.config();
                let (mut chords, double_modifiers) = resolve_global_hotkey_plan(&kb_config);
                if chords.iter().all(Vec::is_empty) && double_modifiers.is_empty() {
                    // No usable binding in config: fall back to the canonical
                    // toggle default from the shared bundle instead of a
                    // hardcoded chord.
                    let bundled = KeyboardConfig::default();
                    let (fallback_chords, _) = resolve_global_hotkey_plan(&bundled);
                    let fallback_toggle = fallback_chords.into_iter().next().unwrap_or_default();
                    if fallback_toggle.is_empty() {
                        crate::log_warn!("[hotkey] no valid toggleWindow shortcut and no bundled default; global toggle disabled");
                    } else {
                        crate::log_warn!("[hotkey] no valid toggleWindow shortcut found in config, using bundled default");
                        if let Some(slot) = chords.first_mut() {
                            *slot = fallback_toggle;
                        }
                    }
                }
                if !cfg!(target_os = "windows") || chords.iter().any(|list| !list.is_empty()) || !double_modifiers.is_empty() {
                    hotkey_manager.start_with_plan(
                        chords,
                        double_modifiers,
                        window.clone(),
                        app.handle().clone(),
                    );
                }
            }
            app.manage(Mutex::new(keyboard));
            app.manage(Mutex::new(hotkey_manager));

            if let Some(transparency) = startup_transparency {
                apply_window_transparency_to_main(app.handle(), transparency);
            }
            apply_window_effect_to_main(app.handle(), &startup_window_effect);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_info,
            get_storage_status,
            get_storage_kind_stats,
            list_icon_cache,
            delete_icon_files,
            replace_icon_file,
            configure_storage_directory,
            get_application_filter_settings,
            configure_ignored_applications,
            list_clipboard_items,
            read_clipboard_text,
            set_clipboard_item_favorite,
            set_clipboard_item_tags,
            set_clipboard_item_last_used,
            list_all_tags,
            rename_tag,
            delete_tag,
            set_tag_color,
            batch_set_favorite,
            delete_clipboard_item,
            batch_delete_clipboard_items,
            get_clipboard_item_ocr,
            get_clipboard_item,
            regenerate_clipboard_item_ocr,
            list_source_applications,
            get_keyboard_config,
            configure_keyboard_shortcuts,
            delete_keyboard_action,
            reset_keyboard_config,
            paste_to_previous_application,
            search_clipboard_items,
            rebuild_search_index,
            detect_content_markers,
    log_frontend_message,
            transform_text,
            toggle_privacy_pause,
            check_sensitive_content,
            get_privacy_status,
            get_privacy_settings,
            set_privacy_settings,
            get_auto_tag_rules,
            preview_auto_tag_rules,
            preview_auto_tag_history,
            apply_auto_tag_history,
            set_auto_tag_rules,
            check_for_update,
            get_release,
            export_clipboard_items,
            export_to_file,
            create_resource_backup,
            preview_resource_backup,
            restore_resource_backup,
            import_clipboard_items,
            import_from_file,
            get_export_formats,
            get_import_formats,
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
            open_float_panel,
            toggle_float_panel,
            set_export_config,
            run_cli_command,
            start_local_api,
            stop_local_api,
            get_local_api_status,
            get_ocr_status,
            get_ocr_config,
            set_ocr_config,
            restart_ocr_engine,
            install_ppocr,
            check_ppocr_status,
            mark_self_triggered,
            mark_self_triggered_image,
            unmark_self_triggered,
            unmark_self_triggered_image,
            open_external_url,
            reveal_in_explorer,
            save_clipboard_item_file,
            rename_item,
            update_clipboard_text,
            save_clipboard_item_as_new,
            set_history_config,
            get_history_config,
            set_storage_config,
            set_resource_storage_paths,
            set_resource_ownership,
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
            record_search_interaction_latency,
            memory::get_memory_diagnostics,
            repair_database,
            validate_search_index,
            cleanup_storage_files,
            restart_app,
            get_sync_config,
            set_sync_config,
            test_sync_connection,
            commands::background_operations::get_background_operation,
            commands::background_operations::cancel_background_operation,
            sync_now,
            materialize_clipboard_item,
            copy_clipboard_item_files,
            copy_clipboard_item
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
#[path = "lib_tests.rs"]
mod lib_tests;
