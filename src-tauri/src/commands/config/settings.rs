use std::sync::Mutex;

use tauri::Emitter;

use crate::config::{ConfigStore, GeneralConfig};
use crate::geometry::{clamp_window_position_to_work_areas, WindowPosition, WindowWorkArea};
use crate::platform::{sync_autostart, WindowManager};

use super::{ExportConfigInfo, GeneralSettingsInfo, HistoryConfigInfo, WindowConfigInfo};

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

    let _ = app.emit("general-settings-changed", &saved);
    apply_window_transparency_to_main(&app, saved.window_transparency);
    Ok(saved)
}

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
