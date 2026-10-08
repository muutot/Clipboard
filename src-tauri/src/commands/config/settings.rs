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
    settings: Option<GeneralConfig>,
    patch: Option<serde_json::Value>,
) -> Result<GeneralConfig, String> {
    // Serialize mode transitions without holding the config lock while opening
    // a database or joining a worker. Searches inspect this same runtime state.
    let mut worker = lock_state(&search_worker, "search-sync lock is poisoned")?;
    let (settings, previous_opacity_affects_text) = {
        let config = lock_state(&config, "configuration lock is poisoned")?;
        (
            resolve_general_settings(config.general_settings(), settings, patch)?,
            config.general_settings().window_opacity_affects_text,
        )
    };
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
    capture.set_max_text_capture_bytes(max_text_capture_bytes);
    crate::logging::set_level(crate::logging::LogLevel::from_str_lossy(&saved.log_level));
    if let Err(error) = app.emit("general-settings-changed", &saved) {
        crate::log_error!("[settings] failed to emit general-settings-changed: {error}");
    }
    if let Some(percent) = native_opacity_after_change(previous_opacity_affects_text, &saved) {
        apply_window_transparency_to_main(&app, percent);
    }
    apply_window_effect_to_main(&app, &saved.window_effect);
    // Keep side effects and broadcasts in the same order as persisted writes.
    drop(worker);
    Ok(saved)
}

fn native_opacity_after_change(previous_affects_text: bool, saved: &GeneralConfig) -> Option<u8> {
    if saved.window_opacity_affects_text {
        Some(saved.window_transparency)
    } else if previous_affects_text {
        // CSS now controls background opacity; undo the previous whole-window alpha.
        Some(100)
    } else {
        // Avoid enabling Windows' layered alpha on windows that never needed it.
        None
    }
}

#[cfg(test)]
mod opacity_tests {
    use super::native_opacity_after_change;
    use crate::config::GeneralConfig;

    #[test]
    fn disabling_text_opacity_restores_native_alpha_without_affecting_css_preference() {
        let mut saved = GeneralConfig::default();
        saved.window_transparency = 70;
        assert_eq!(native_opacity_after_change(true, &saved), Some(100));
        assert_eq!(saved.window_transparency, 70);
        assert_eq!(native_opacity_after_change(false, &saved), None);
        saved.window_opacity_affects_text = true;
        assert_eq!(native_opacity_after_change(false, &saved), Some(70));
        assert_eq!(native_opacity_after_change(true, &saved), Some(70));
        saved.window_transparency = 90;
        assert_eq!(native_opacity_after_change(true, &saved), Some(90));
    }
}

fn resolve_general_settings(
    current: &GeneralConfig,
    settings: Option<GeneralConfig>,
    patch: Option<serde_json::Value>,
) -> Result<GeneralConfig, String> {
    let mut value = match (settings, patch) {
        (Some(settings), None) => {
            serde_json::to_value(settings).map_err(|error| error.to_string())?
        }
        (None, Some(patch)) if patch.is_object() => {
            let mut value = serde_json::to_value(current).map_err(|error| error.to_string())?;
            merge_settings_patch(&mut value, patch);
            value
        }
        _ => return Err("provide either settings or an object patch".into()),
    };
    // Two windows may independently change the page size and candidate cap.
    let cap = value["searchPageSizeLimit"]
        .as_u64()
        .unwrap_or(500)
        .clamp(50, 1000);
    if let Some(page_size) = value["display"]["searchPageSize"].as_u64() {
        value["display"]["searchPageSize"] = page_size.clamp(10, 500).min(cap).into();
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn merge_settings_patch(target: &mut serde_json::Value, patch: serde_json::Value) {
    if let serde_json::Value::Object(fields) = patch {
        if !target.is_object() {
            *target = serde_json::json!({});
        }
        let object = target.as_object_mut().expect("object initialized above");
        for (key, value) in fields {
            if value.is_null() {
                object.remove(&key);
            } else {
                merge_settings_patch(object.entry(key).or_insert(serde_json::Value::Null), value);
            }
        }
    } else {
        *target = patch;
    }
}

#[cfg(test)]
mod settings_patch_tests {
    use super::resolve_general_settings;
    use crate::config::GeneralConfig;
    use serde_json::json;

    #[test]
    fn sequential_patches_preserve_unrelated_and_nested_fields() {
        let initial = GeneralConfig::default();
        let first = resolve_general_settings(
            &initial,
            None,
            Some(json!({"pageSizeLimit": 800, "display": {"pageSize": 200}})),
        )
        .unwrap();
        let second = resolve_general_settings(
            &first,
            None,
            Some(json!({"searchCacheSize": 1000, "display": {"maxTextLines": 6}})),
        )
        .unwrap();
        assert_eq!(second.page_size_limit, 800);
        assert_eq!(second.display.max_text_lines, 6);
        let saved = serde_json::to_value(second).unwrap();
        assert_eq!(saved["display"]["pageSize"], 200);
        assert_eq!(saved["searchCacheSize"], 1000);
    }

    #[test]
    fn patches_clear_optional_fields_and_replace_arrays() {
        let initial = serde_json::from_value(json!({
            "activePresetId": "custom", "iconColors": {"text": "#ffffff", "file": "#000000"},
            "searchSortRules": [{"field": "title", "direction": "asc"}]
        }))
        .unwrap();
        let saved = resolve_general_settings(
            &initial,
            None,
            Some(json!({
                "activePresetId": null, "iconColors": {"text": null},
                "searchSortRules": [{"field": "lastUsedAt", "direction": "desc"}]
            })),
        )
        .unwrap();
        let saved = serde_json::to_value(saved).unwrap();
        assert!(saved.get("activePresetId").is_none());
        assert_eq!(saved["iconColors"], json!({"file": "#000000"}));
        assert_eq!(
            saved["searchSortRules"],
            json!([{"field": "lastUsedAt", "direction": "desc"}])
        );
    }

    #[test]
    fn rejects_ambiguous_invalid_requests_and_clamps_cross_field_limits() {
        let initial = GeneralConfig::default();
        assert!(resolve_general_settings(&initial, None, None).is_err());
        assert!(resolve_general_settings(&initial, None, Some(json!([]))).is_err());
        assert!(
            resolve_general_settings(&initial, Some(initial.clone()), Some(json!({}))).is_err()
        );
        assert!(resolve_general_settings(&initial, None, Some(json!({"display": false}))).is_err());
        let saved =
            resolve_general_settings(&initial, None, Some(json!({"searchPageSizeLimit": 50})))
                .unwrap();
        let saved = resolve_general_settings(
            &saved,
            None,
            Some(json!({"display": {"searchPageSize": 300}})),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(saved).unwrap()["display"]["searchPageSize"],
            50
        );
    }
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
    let app_for_ui = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let Some(window) = app_for_ui.get_webview_window("main") else {
            return;
        };
        if let Err(error) = crate::platform::ui::apply_webview_transparency(&window, percent) {
            crate::log_error!("[window] failed to apply transparency: {error}");
        }
    }) {
        crate::log_error!("[window] failed to schedule transparency: {error}");
    }
}

pub fn apply_window_effect_to_main(app: &tauri::AppHandle, effect: &str) {
    let app_for_ui = app.clone();
    let effect = effect.to_owned();
    if let Err(error) = app.run_on_main_thread(move || {
        let Some(window) = app_for_ui.get_webview_window("main") else {
            return;
        };
        if let Err(error) = crate::platform::apply_window_effect(&window, &effect) {
            crate::log_error!("[window] failed to apply window effect: {error}");
        }
    }) {
        crate::log_error!("[window] failed to schedule window effect: {error}");
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
