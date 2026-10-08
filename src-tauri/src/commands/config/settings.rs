use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::commands::clipboard::SearchResultCache;
use crate::config::{ConfigStore, GeneralConfig, SearchIndexSyncMode};
use crate::geometry::{clamp_window_position_to_work_areas, WindowPosition, WindowWorkArea};
use crate::platform::{sync_autostart, WindowManager};
use crate::search::{SearchIndex, SearchSyncWorker};
use crate::state::CaptureState;
use crate::storage::{Database, StoragePaths};

use super::{ExportConfigInfo, GeneralSettingsInfo, HistoryConfigInfo, WindowConfigInfo};
use crate::commands::lock::lock_state;

#[tauri::command]
pub fn get_general_settings(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<GeneralSettingsInfo, String> {
    let config = lock_state(&config, "configuration lock is poisoned")?;
    Ok(GeneralSettingsInfo {
        settings: config.general_settings().clone(),
        legacy_migration_required: !config.has_general_settings(),
    })
}

#[tauri::command]
pub fn set_general_settings(
    app: tauri::AppHandle,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    capture: tauri::State<'_, CaptureState>,
    search_worker: tauri::State<'_, Mutex<Option<SearchSyncWorker>>>,
    settings: GeneralConfig,
) -> Result<GeneralConfig, String> {
    // Serialize mode transitions without holding the config lock while opening
    // a database or joining a worker. Searches inspect this same runtime state.
    let mut worker = lock_state(&search_worker, "search-sync lock is poisoned")?;
    if worker.as_ref().is_some_and(|worker| !worker.is_running()) {
        *worker = None;
    }
    let background = settings
        .search_index_sync_mode
        .parse::<SearchIndexSyncMode>()
        == Ok(SearchIndexSyncMode::Background);
    let (saved, max_text_capture_bytes) = transition_search_worker(
        &mut worker,
        background,
        || {
            let database = Database::open(&app.state::<StoragePaths>().database)
                .map_err(|error| error.to_string())?;
            let index = app.state::<Arc<SearchIndex>>().inner().clone();
            let app_for_sync = app.clone();
            SearchSyncWorker::start(
                database,
                index,
                Duration::from_millis(500),
                Arc::new(move || {
                    app_for_sync.state::<SearchResultCache>().clear();
                    if let Err(error) = app_for_sync.emit("search-index-changed", ()) {
                        crate::log_warn!("[search] unable to announce index changes: {error}");
                    }
                }),
            )
            .map_err(|error| error.to_string())
        },
        || {
            let mut config = lock_state(&config, "configuration lock is poisoned")?;
            config
                .set_general_settings(settings)
                .map_err(|error| error.to_string())?;
            Ok((
                config.general_settings().clone(),
                // Use the clamped getter, not the raw stored field: the command
                // accepts any u64, so an unclamped value would silently change the
                // live capture cap while the config read-back reports the clamp.
                config.max_text_capture_bytes(),
            ))
        },
    )?;
    drop(worker);

    capture.set_max_text_capture_bytes(max_text_capture_bytes);
    crate::logging::set_level(crate::logging::LogLevel::from_str_lossy(&saved.log_level));
    if let Err(error) = app.emit("general-settings-changed", &saved) {
        crate::log_error!("[settings] failed to emit general-settings-changed: {error}");
    }
    if saved.window_opacity_affects_text {
        apply_window_transparency_to_main(&app, saved.window_transparency);
    }
    apply_window_effect_to_main(&app, &saved.window_effect);
    Ok(saved)
}

/// Prepare a replacement before persistence, then publish it only on success.
/// Dropping a worker stops and joins it, including a prepared worker on error.
fn transition_search_worker<W, T>(
    current: &mut Option<W>,
    background: bool,
    start: impl FnOnce() -> Result<W, String>,
    persist: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let prepared = if background && current.is_none() {
        Some(start()?)
    } else {
        None
    };
    let saved = persist()?;
    if !background {
        *current = None;
    } else if prepared.is_some() {
        *current = prepared;
    }
    Ok(saved)
}

#[cfg(test)]
mod search_mode_tests {
    use super::transition_search_worker;

    #[test]
    fn live_mode_switches_reuse_and_stop_the_worker() {
        let mut worker = None;
        transition_search_worker(&mut worker, true, || Ok(1), || Ok(())).unwrap();
        assert_eq!(worker, Some(1));
        transition_search_worker(&mut worker, true, || panic!("duplicate worker"), || Ok(()))
            .unwrap();
        transition_search_worker(
            &mut worker,
            false,
            || panic!("lazy must not start"),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(worker, None);
        transition_search_worker(&mut worker, true, || Ok(2), || Ok(())).unwrap();
        assert_eq!(worker, Some(2));
    }

    #[test]
    fn failed_transitions_preserve_the_previous_mode() {
        let mut worker = None::<u8>;
        assert!(transition_search_worker(
            &mut worker,
            true,
            || Err("spawn".into()),
            || -> Result<(), String> { panic!("must not save") }
        )
        .is_err());
        assert_eq!(worker, None);
        assert!(transition_search_worker(
            &mut worker,
            true,
            || Ok(1),
            || Err::<(), _>("save".into())
        )
        .is_err());
        assert_eq!(worker, None);
        worker = Some(2);
        assert!(transition_search_worker(
            &mut worker,
            false,
            || panic!(),
            || Err::<(), _>("save".into())
        )
        .is_err());
        assert_eq!(worker, Some(2));
    }
}

pub fn apply_window_transparency_to_main(app: &tauri::AppHandle, percent: u8) {
    #[cfg(target_os = "windows")]
    {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        match window.hwnd() {
            Ok(hwnd) => {
                if let Err(error) =
                    crate::platform::apply_window_transparency(hwnd.0 as isize, percent)
                {
                    crate::log_error!("[window] failed to apply transparency: {error}");
                }
            }
            Err(error) => {
                crate::log_error!("[window] failed to resolve the main window handle: {error}");
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, percent);
    }
}

pub fn apply_window_effect_to_main(app: &tauri::AppHandle, effect: &str) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = crate::platform::apply_window_effect(&window, effect) {
        crate::log_error!("[window] failed to apply window effect: {error}");
    }
}

#[tauri::command]
pub fn get_history_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<HistoryConfigInfo, String> {
    let config = lock_state(&config, "configuration lock is poisoned")?;
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
    let mut config = lock_state(&config, "configuration lock is poisoned")?;
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
    let config = lock_state(&config, "configuration lock is poisoned")?;
    Ok(ExportConfigInfo {
        schedule_auto_export: config.schedule_auto_export().map(|s| s.to_owned()),
    })
}

#[tauri::command]
pub fn set_export_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    schedule_auto_export: Option<String>,
) -> Result<(), String> {
    lock_state(&config, "configuration lock is poisoned")?
        .set_schedule_auto_export(schedule_auto_export)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_window_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<WindowConfigInfo, String> {
    let config = lock_state(&config, "configuration lock is poisoned")?;
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
    let mut config = lock_state(&config, "configuration lock is poisoned")?;
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
    let mut guard = lock_state(&config, "configuration lock is poisoned")?;
    WindowManager::save_position(&mut guard, x, y, width, height)
}

#[tauri::command]
pub fn restore_window_position(
    window: tauri::Window,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<Option<WindowPosition>, String> {
    let mut config = lock_state(&config, "configuration lock is poisoned")?;
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
            crate::log_error!(
                "[window] failed to enumerate monitors while restoring bounds: {error}"
            );
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
