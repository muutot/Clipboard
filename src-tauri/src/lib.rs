pub mod cli;
pub mod commands;
use commands::capture::*;
use commands::clipboard::*;
use commands::config::*;
use commands::export::*;
use commands::ocr::*;
use commands::system::*;
use commands::update::*;
pub mod config;
pub mod content;
pub mod domain;
pub mod export;
pub mod geometry;
pub mod keyboard;
pub mod memory;
pub mod ocr;
pub mod performance;
pub mod platform;
pub mod privacy;
pub mod search;
pub mod shutdown;
pub mod state;
pub mod storage;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use cli::LocalApiServer;
use commands::clipboard::SearchResultCache;
use config::ConfigStore;
use content::{resource_metadata::RESOURCE_METADATA_SCHEMA_VERSION, self_trigger, ThumbnailWorker};
use domain::{ClipboardItem, ClipboardKind};
#[allow(unused_imports)]
use geometry::{clamp_window_position_to_work_areas, WindowPosition, WindowWorkArea};
use keyboard::{KeyboardConfig, KeyboardManager};
use ocr::{NoopOcrEngine, OcrEngine, OcrWorkerManager, PpOcrEngine, TesseractOcrEngine};
use performance::{PerformanceTracker, StartupMetrics, StartupTimer};
use platform::windows_hotkey::{
    shortcut_bindings_to_double_modifiers, shortcut_bindings_to_windows_hotkeys, HotkeyManager,
};
use platform::{
    show_main_window, sync_autostart, ClipboardMonitor, GlobalShortcutManager, SingleInstanceError,
    SingleInstanceGuard, SystemTray,
};
use privacy::PrivacyManager;
use search::{SearchIndex, SearchSynchronizer};
use shutdown::stop_runtime_services;
use state::{CaptureState, CaptureWorker, SelfTriggerState};
use std::str::FromStr;
use storage::{
    quarantine_search_index, recover_database_if_needed, refresh_database_backup,
    ClipboardRepository, Database, KindDeleteScope, OcrRepository, StoragePaths,
};
use tauri::{Emitter, Manager};

const TOGGLE_WINDOW_ACTION: &str = "toggleWindow";
pub(crate) const STORAGE_KIND_DELETE_SCOPE: KindDeleteScope = KindDeleteScope {
    include_favorites: false,
    include_deleted: true,
};
#[cfg_attr(not(test), allow(dead_code))]
const MAIN_WINDOW_MIN_WIDTH: u32 = 730;
#[cfg_attr(not(test), allow(dead_code))]
const MAIN_WINDOW_MIN_HEIGHT: u32 = 500;

const HISTORY_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const SCHEDULED_ORPHAN_FILE_GRACE: Duration = Duration::from_secs(10 * 60);

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
                    Ok(config) => match commands::cleanup::enforce_history_cleanup_for(
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

            let self_trigger_guard = Arc::new(Mutex::new(self_trigger::SelfTriggerGuard::new()));
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
                            Ok(_change) => {
                                let _ingestion_guard = capture_for_thread
                                    .ingestion_guard
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                if stop_flag_for_thread.load(Ordering::SeqCst)
                                    || stop_signal_requested(&stop_receiver)
                                {
                                    break;
                                }

                                let app_info = platform::get_foreground_app();
                                let source_app = foreground_app_name(&app_info);
                                if capture_for_thread.should_skip(source_app.as_deref(), None) {
                                    continue;
                                }

                                // Extract and cache app icon
                                let icon_dir = storage_path.join("icons");
                                let icon_path = if let Some(source_name) = source_app.as_deref() {
                                    platform::extract_app_icon(
                                        &icon_dir,
                                        source_name,
                                        &app_info.exe_path,
                                    )
                                } else {
                                    None
                                };

                                let text = platform::read_clipboard_text();
                                let image_data = platform::read_clipboard_image();
                                let file_paths = platform::read_clipboard_file_paths();

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
                                        metadata_json: Some(metadata.to_string()),
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
                                            metadata_json: Some(captured_file_metadata(&stored_files)),
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
                                            metadata_json: Some(captured_file_metadata(&stored_files)),
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

            let cleanup_database = Database::open(&paths.database)?;
            let cleanup_worker =
                CleanupWorker::start(project_directory.clone(), cleanup_database, paths.clone())?;

            let launch_at_startup = config.launch_at_startup();
            let startup_transparency = config.general_settings().window_transparency;
            let startup_window_effect = config.general_settings().window_effect.clone();
            app.manage(Mutex::new(config));
            app.manage(paths);
            app.manage(database);
            app.manage(search_index);
            app.manage(performance_tracker);
            app.manage(SearchResultCache::new());
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
            #[allow(unused_mut)]
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
            apply_window_effect_to_main(app.handle(), &startup_window_effect);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_info,
            get_storage_status,
            get_storage_kind_stats,
            list_icon_files,
            delete_icon_files,
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
            delete_keyboard_action,
            reset_keyboard_config,
            paste_to_previous_application,
            search_clipboard_items,
            rebuild_search_index,
            detect_content_markers,
            transform_text,
            toggle_privacy_pause,
            check_sensitive_content,
            check_password_manager,
            get_privacy_status,
            check_for_update,
            export_clipboard_items,
            export_to_file,
            import_clipboard_items,
            import_from_file,
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
#[path = "lib_tests.rs"]
mod lib_tests;
