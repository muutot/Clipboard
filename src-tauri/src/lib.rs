pub mod cli;
pub mod commands;

pub fn dbg_log(msg: &str) {
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
use commands::capture::*;
use commands::clipboard::*;
use commands::config::*;
use commands::export::*;
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

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use cli::LocalApiServer;
use commands::clipboard::SearchResultCache;
use config::{ConfigStore, SearchIndexSyncMode};
use content::{self_trigger, ThumbnailWorker};
use keyboard::{KeyboardConfig, KeyboardManager};
use ocr::{NoopOcrEngine, OcrEngine, OcrWorkerManager, PpOcrEngine, TesseractOcrEngine};
use performance::{PerformanceTracker, StartupMetrics, StartupTimer};
use platform::windows_hotkey::{
    shortcut_bindings_to_double_modifiers, shortcut_bindings_to_windows_hotkeys, HotkeyManager,
};
use platform::{
    show_main_window, sync_autostart, ClipboardMonitor, SingleInstanceError, SingleInstanceGuard,
    SystemTray,
};
use privacy::PrivacyManager;
use search::{SearchIndex, SearchSyncWorker, SearchSynchronizer};
use shutdown::stop_runtime_services;
use state::{CaptureState, CaptureWorker, SelfTriggerState};
use std::str::FromStr;
use storage::{
    quarantine_search_index, recover_database_if_needed, refresh_database_backup, Database,
    KindDeleteScope, OcrRepository, StoragePaths,
};
use tauri::Manager;

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

            // Initialize sync device identifier for multi-device oplog sync.
            let device_id = get_sync_device_id();
            if let Err(e) = database.set_sync_device_id(&device_id) {
                eprintln!("[sync] failed to set device_id: {e}");
            } else {
                eprintln!("[sync] device_id = {device_id}");
            }

            let db_open_duration = startup_timer.finish_segment();

            let search_index = Arc::new(SearchIndex::open(&paths.search_index)?);
            // `SearchSynchronizer::initialize` rebuilds the index when
            // `SearchIndexLayout` flags it as `rebuild_required`; the previous
            // `validate()` call here was a no-op (always returned `true`) and
            // has been removed. Use the `validate_search_index` Tauri command
            // for an on-demand health probe instead.
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
                eprintln!("[startup] clipboard monitor has no receiver");
            }
        } else {
            eprintln!("[startup] failed to start clipboard monitor");
        }

            let cleanup_database = Database::open(&paths.database)?;
            let cleanup_worker =
                CleanupWorker::start(project_directory.clone(), cleanup_database, paths.clone())?;

            // Optional background search-index synchronizer. In `Lazy` mode the
            // search command drains the outbox itself; in `Background` mode this
            // worker keeps the Tantivy index fresh off the hot path so queries
            // never block on indexing. The mode only takes effect at startup.
            let search_sync_worker: Option<SearchSyncWorker> =
                if config.search_index_sync_mode() == SearchIndexSyncMode::Background {
                    let sync_database = Database::open(&paths.database)?;
                    let sync_index = search_index.clone();
                    let app_for_sync = app.handle().clone();
                    let on_changes_applied = Arc::new(move || {
                        if let Some(cache) = app_for_sync.try_state::<SearchResultCache>() {
                            cache.clear();
                        }
                    });
                    match SearchSyncWorker::start(
                        sync_database,
                        sync_index,
                        Duration::from_millis(500),
                        on_changes_applied,
                    ) {
                        Ok(worker) => {
                            eprintln!("[search-sync] background synchronizer started");
                            Some(worker)
                        }
                        Err(error) => {
                            eprintln!("[search-sync] failed to start background synchronizer: {error}");
                            None
                        }
                    }
                } else {
                    None
                };

            let launch_at_startup = config.launch_at_startup();
            let startup_transparency = if config.general_settings().window_opacity_affects_text {
                config.general_settings().window_transparency
            } else {
                100
            };
            let startup_window_effect = config.general_settings().window_effect.clone();
            app.manage(Mutex::new(config));
            app.manage(paths);
            app.manage(database);
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
            list_icon_cache,
            delete_icon_files,
            replace_icon_file,
            configure_storage_directory,
            get_application_filter_settings,
            configure_ignored_applications,
            list_clipboard_items,
            set_clipboard_item_favorite,
            set_clipboard_item_tags,
            list_all_tags,
            rename_tag,
            delete_tag,
            set_tag_color,
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
            get_release,
            export_clipboard_items,
            export_to_file,
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
            restart_app,
            get_sync_config,
            set_sync_config,
            test_sync_connection,
            sync_upload_backup,
            sync_list_remote_backups,
            sync_download_backup,
            verify_backup_file
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            stop_runtime_services(app_handle);
        }
    });
}

fn get_sync_device_id() -> String {
    if let Ok(hostname) = std::env::var("COMPUTERNAME") {
        return hostname.to_lowercase();
    }
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        return hostname.to_lowercase();
    }
    "unknown".to_string()
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;
