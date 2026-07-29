use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use rusqlite::params;
use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::config::{ConfigStore, GeneralConfig};
use crate::domain::ClipboardKind;
use crate::keyboard::{KeyboardConfig, KeyboardManager};
use crate::platform::{self, sync_autostart, ClipboardMonitor, RuntimeInfo, WindowManager};
use crate::platform::windows_hotkey::HotkeyManager;
use crate::privacy::PrivacyManager;
use crate::search::{SearchIndex, SEARCH_INDEX_VERSION};
use crate::storage::{ClipboardRepository, Database, KindStorageStats, StorageError, StoragePaths};
use crate::{
    CaptureState, TOGGLE_WINDOW_ACTION, resolve_toggle_hotkeys,
    STORAGE_KIND_DELETE_SCOPE, WindowPosition, WindowWorkArea,
    clamp_window_position_to_work_areas,
};

#[tauri::command]
pub fn get_runtime_info() -> RuntimeInfo {
    platform::runtime_info()
}

#[tauri::command]
pub fn get_storage_status(
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
pub fn get_storage_kind_stats(
    database: tauri::State<'_, Database>,
    kind: ClipboardKind,
) -> Result<StorageKindStats, String> {
    database
        .kind_storage_stats(kind, STORAGE_KIND_DELETE_SCOPE)
        .map(StorageKindStats::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn configure_storage_directory(
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

#[tauri::command]
pub fn get_application_filter_settings(
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
pub fn configure_ignored_applications(
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

pub fn apply_ignored_applications(
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
pub fn get_keyboard_config(
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
) -> Result<KeyboardConfig, String> {
    Ok(keyboard
        .lock()
        .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
        .config())
}

#[tauri::command]
pub fn configure_keyboard_shortcuts(
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
pub fn delete_keyboard_action(
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    action: String,
) -> Result<(), String> {
    keyboard
        .lock()
        .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
        .delete_action(action)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn reset_keyboard_config(
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    hotkey_manager: tauri::State<'_, Mutex<HotkeyManager>>,
) -> Result<KeyboardConfig, String> {
    {
        let mut km = keyboard
            .lock()
            .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?;
        km.reset_to_defaults().map_err(|error| error.to_string())?;
    }
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
    Ok(config)
}

#[tauri::command]
pub fn paste_to_previous_application(
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
pub fn get_general_settings(
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
pub fn set_general_settings(
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
pub fn apply_window_transparency_to_main(app: &tauri::AppHandle, percent: u8) {
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

#[tauri::command]
pub fn get_history_config(
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
pub fn set_history_config(
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

#[tauri::command]
pub fn get_storage_config(
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
pub fn set_storage_config(
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
pub fn set_resource_storage_paths(
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

pub fn copy_dir_contents(from: &Path, to: &Path) -> Result<(), String> {
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

pub fn file_or_dir_size(path: &PathBuf) -> u64 {
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

pub fn dir_size(path: &PathBuf) -> u64 {
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
pub fn toggle_privacy_pause(
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
pub fn check_sensitive_content(
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    text: String,
) -> Result<bool, String> {
    Ok(privacy
        .lock()
        .map_err(|_| "privacy manager lock is poisoned".to_owned())?
        .is_sensitive_content(&text))
}

#[tauri::command]
pub fn check_password_manager(
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    app_name: String,
) -> Result<bool, String> {
    Ok(privacy
        .lock()
        .map_err(|_| "privacy manager lock is poisoned".to_owned())?
        .is_password_manager(&app_name))
}

#[tauri::command]
pub fn get_privacy_status(
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

#[tauri::command]
pub fn set_clipboard_ignored_apps(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
    apps: Vec<String>,
) -> Result<Vec<String>, String> {
    apply_ignored_applications(config.inner(), monitor.inner(), capture.inner(), apps)
}

#[tauri::command]
pub fn save_window_position(
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
pub fn restore_window_position(
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

#[tauri::command]
pub fn get_window_config(
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

#[tauri::command]
pub fn set_window_config(
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
pub fn get_export_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<ExportConfigInfo, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    Ok(ExportConfigInfo {
        schedule_auto_export: config.schedule_auto_export().map(|s| s.to_owned()),
    })
}

#[tauri::command]
pub fn set_export_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    schedule_auto_export: Option<String>,
) -> Result<(), String> {
    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_schedule_auto_export(schedule_auto_export)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
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
pub struct StorageKindStats {
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
pub struct StorageDirectoryUpdate {
    data_directory_path: String,
    storage_path: String,
    restart_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStorageUpdate {
    image_storage_path: String,
    file_storage_path: String,
    restart_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveredApplication {
    name: String,
    icon_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationFilterSettings {
    discovered_applications: Vec<String>,
    discovered_applications_with_icons: Vec<DiscoveredApplication>,
    ignored_applications: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowConfigInfo {
    launch_at_startup: bool,
    close_to_tray: bool,
    single_instance: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfigInfo {
    schedule_auto_export: Option<String>,
}

pub fn rewrite_database_storage_paths(
    database: &Database,
    mappings: &[(PathBuf, PathBuf)],
) -> Result<u64, StorageError> {
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

pub fn rewrite_optional_storage_path(
    value: Option<&str>,
    mappings: &[(PathBuf, PathBuf)],
) -> Option<String> {
    value.map(|value| rewrite_storage_path(value, mappings))
}

pub fn rewrite_storage_path(value: &str, mappings: &[(PathBuf, PathBuf)]) -> String {
    let path = Path::new(value);
    for (from, to) in mappings {
        if let Ok(relative) = path.strip_prefix(from) {
            return to.join(relative).to_string_lossy().into_owned();
        }
    }
    value.to_owned()
}

pub fn rewrite_json_storage_paths(
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

pub fn rewrite_json_value_paths(
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

pub fn migrate_storage_data(
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

    // Search index is not migrated 鈥?it will be rebuilt from the migrated
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

pub fn storage_path_mappings(old: &StoragePaths, new: &StoragePaths) -> Vec<(PathBuf, PathBuf)> {
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsInfo {
    settings: GeneralConfig,
    legacy_migration_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryConfigInfo {
    max_items: u32,
    retention_days: u32,
    recycle_bin_days: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfigInfo {
    max_file_copy_size_bytes: u64,
    max_screenshot_size_bytes: u64,
    image_storage_path: Option<String>,
    file_storage_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyStatus {
    paused: bool,
    password_manager_apps: Vec<String>,
    master_password_hash_set: bool,
}
