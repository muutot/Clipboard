use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::config::ConfigStore;
use crate::keyboard::{is_global_action, KeyboardConfig, KeyboardManager};
use crate::platform::windows::hotkey::HotkeyManager;
use crate::platform::{self, ClipboardMonitor, RuntimeInfo};
use crate::privacy::PrivacyManager;
use crate::storage::{ClipboardRepository, Database};
use crate::CaptureState;

use super::{ApplicationFilterSettings, DiscoveredApplication, PrivacySettings, PrivacyStatus};
use crate::commands::lock::lock_state;

#[tauri::command]
pub fn get_runtime_info() -> RuntimeInfo {
    platform::runtime_info()
}

#[tauri::command]
pub fn toggle_privacy_pause(
    app: tauri::AppHandle,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    capture: tauri::State<'_, CaptureState>,
) -> Result<bool, String> {
    let paused = {
        let mut privacy = lock_state(&privacy, "privacy manager lock is poisoned")?;
        privacy.toggle_pause();
        privacy.is_paused()
    };

    // Persist before touching the capture worker: if the config write fails,
    // roll the in-memory toggle back so `PrivacyManager` and `CaptureState`
    // cannot fork (paused in memory while still recording).
    if let Err(error) = lock_state(&config, "configuration lock is poisoned")?
        .set_privacy_paused(paused)
        .map_err(|e| e.to_string())
    {
        lock_state(&privacy, "privacy manager lock is poisoned")?.toggle_pause();
        return Err(error);
    }

    capture.set_paused(paused);

    if let Err(error) = app.emit("privacy-pause-changed", paused) {
        crate::log_error!("[privacy] failed to emit pause-changed: {error}");
    }

    Ok(paused)
}

#[tauri::command]
pub fn check_sensitive_content(
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    text: String,
) -> Result<bool, String> {
    Ok(lock_state(&privacy, "privacy manager lock is poisoned")?.is_sensitive_content(&text))
}

#[tauri::command]
pub fn get_privacy_status(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
) -> Result<PrivacyStatus, String> {
    let privacy = lock_state(&privacy, "privacy manager lock is poisoned")?;
    let config = lock_state(&config, "configuration lock is poisoned")?;

    Ok(PrivacyStatus {
        paused: privacy.is_paused(),
        master_password_hash_set: config.privacy_master_password_hash().is_some(),
    })
}

#[tauri::command]
pub fn get_privacy_settings(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
) -> Result<PrivacySettings, String> {
    let privacy = lock_state(&privacy, "privacy manager lock is poisoned")?;
    let config = lock_state(&config, "configuration lock is poisoned")?;

    Ok(PrivacySettings {
        paused: privacy.is_paused(),
        local_only: config.privacy_local_only(),
        sensitive_patterns: config.sensitive_patterns().to_vec(),
    })
}

#[tauri::command]
pub fn set_privacy_settings(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    sync_cancellation: tauri::State<'_, crate::commands::sync::SyncCancellation>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    capture: tauri::State<'_, CaptureState>,
    local_only: Option<bool>,
    sensitive_patterns: Option<Vec<String>>,
) -> Result<PrivacySettings, String> {
    // Validate the pattern list up front so nothing is persisted when one of
    // the regexes fails to compile.
    if let Some(patterns) = sensitive_patterns.as_ref() {
        for pattern in patterns {
            if let Err(error) = regex_lite::Regex::new(pattern.trim()) {
                return Err(format!("invalid sensitive pattern {pattern:?}: {error}"));
            }
        }
    }

    let persisted_patterns: Option<Vec<String>> = {
        let mut config = lock_state(&config, "configuration lock is poisoned")?;
        if let Some(value) = local_only {
            config
                .set_privacy_local_only(value)
                .map_err(|error| error.to_string())?;
            if value {
                sync_cancellation.cancel_active();
            }
        }
        sensitive_patterns
            .map(|patterns| {
                config
                    .set_sensitive_patterns(patterns)
                    .map_err(|error| error.to_string())
            })
            .transpose()?
    };

    // Mirror the persisted values into the runtime managers so the running
    // capture worker picks them up without a restart.
    if let Some(patterns) = persisted_patterns {
        let compiled: Vec<regex_lite::Regex> = patterns
            .iter()
            .filter_map(|pattern| regex_lite::Regex::new(pattern).ok())
            .collect();

        lock_state(&privacy, "privacy manager lock is poisoned")?.sensitive_patterns =
            compiled.clone();
        capture.set_sensitive_patterns(compiled);
    }

    // No event is emitted here: the command returns the fresh
    // `PrivacySettings` and the settings panel renders from that value.

    let privacy = lock_state(&privacy, "privacy manager lock is poisoned")?;
    let config = lock_state(&config, "configuration lock is poisoned")?;

    Ok(PrivacySettings {
        paused: privacy.is_paused(),
        local_only: config.privacy_local_only(),
        sensitive_patterns: config.sensitive_patterns().to_vec(),
    })
}

#[tauri::command]
pub fn get_auto_tag_rules(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<Vec<crate::config::AutoTagRule>, String> {
    Ok(lock_state(&config, "configuration lock is poisoned")?
        .auto_tag_rules()
        .to_vec())
}

#[tauri::command]
pub fn set_auto_tag_rules(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    capture: tauri::State<'_, CaptureState>,
    rules: Vec<crate::config::AutoTagRule>,
) -> Result<Vec<crate::config::AutoTagRule>, String> {
    crate::tags::validate_auto_tag_rules(&rules)?;

    let persisted = {
        lock_state(&config, "configuration lock is poisoned")?
            .set_auto_tag_rules(rules)
            .map_err(|error| error.to_string())?
    };

    // Mirror into the running capture worker without a restart.
    capture.set_auto_tag_rules(crate::tags::compile_auto_tag_rules(&persisted));
    Ok(persisted)
}

fn compiled_preview_rules(
    rules: &[crate::config::AutoTagRule],
) -> Result<Vec<crate::tags::CompiledAutoTagRule>, String> {
    crate::tags::validate_auto_tag_rules(rules)?;
    Ok(crate::tags::compile_auto_tag_rules(
        &rules
            .iter()
            .map(|rule| rule.normalized())
            .collect::<Vec<_>>(),
    ))
}

#[tauri::command]
pub fn preview_auto_tag_rules(
    rules: Vec<crate::config::AutoTagRule>,
    text: String,
    source_app: String,
    kind: crate::domain::ClipboardKind,
) -> Result<Vec<String>, String> {
    if text.len() > 1_000_000 || source_app.len() > 4096 {
        return Err("sample is too large".into());
    }
    Ok(crate::tags::match_auto_tags(
        &compiled_preview_rules(&rules)?,
        &text,
        &source_app,
        kind,
    ))
}

#[tauri::command]
pub async fn preview_auto_tag_history(
    app: tauri::AppHandle,
    rules: Vec<crate::config::AutoTagRule>,
) -> Result<crate::storage::AutoTagHistoryPreview, String> {
    run_auto_tag_history(app, rules, false).await
}

#[tauri::command]
pub async fn apply_auto_tag_history(
    app: tauri::AppHandle,
    rules: Vec<crate::config::AutoTagRule>,
) -> Result<crate::storage::AutoTagHistoryPreview, String> {
    run_auto_tag_history(app, rules, true).await
}

async fn run_auto_tag_history(
    app: tauri::AppHandle,
    rules: Vec<crate::config::AutoTagRule>,
    apply: bool,
) -> Result<crate::storage::AutoTagHistoryPreview, String> {
    let compiled = compiled_preview_rules(&rules)?;
    let operation = app
        .state::<crate::background_operations::BackgroundOperations>()
        .start(
            crate::background_operations::OperationKind::Tags,
            &clipboard_sync::cancellation::CancellationToken::default(),
        )?;
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = (|| {
            let result = app
                .state::<Database>()
                .auto_tag_history_with_progress(&compiled, apply, |progress| {
                    use crate::{background_operations::OperationPhase, storage::AutoTagPhase};
                    let phase = match progress.phase {
                        AutoTagPhase::Scanning => OperationPhase::Scanning,
                        AutoTagPhase::Applying => OperationPhase::Applying,
                    };
                    operation
                        .progress(phase, progress.completed, Some(progress.total))
                        .map_err(|error| {
                            crate::storage::StorageError::Io(std::io::Error::other(error))
                        })
                })
                .map_err(|error| error.to_string())?;
            if apply && result.changed_count > 0 {
                app.state::<crate::commands::clipboard::SearchResultCache>()
                    .clear();
                for event in ["clipboard-history-invalidated", "tags-changed"] {
                    if let Err(error) = app.emit(event, serde_json::json!({ "deletedIds": [] })) {
                        crate::log_warn!("[autotag] unable to broadcast {event}: {error}");
                    }
                }
            }
            Ok(result)
        })();
        operation.finish(outcome)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn get_keyboard_config(
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
) -> Result<KeyboardConfig, String> {
    Ok(lock_state(&keyboard, "keyboard configuration lock is poisoned")?.config())
}

#[tauri::command]
pub fn configure_keyboard_shortcuts(
    app: tauri::AppHandle,
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    hotkey_manager: tauri::State<'_, Mutex<HotkeyManager>>,
    action: String,
    shortcuts: Vec<String>,
) -> Result<Vec<String>, String> {
    let normalized = lock_state(&keyboard, "keyboard configuration lock is poisoned")?
        .set_action_shortcuts(action.clone(), shortcuts)
        .map_err(|error| error.to_string())?;

    if is_global_action(&action) {
        crate::refresh_hotkey_registrations(&keyboard, &hotkey_manager, &app)?;
    }

    Ok(normalized)
}

#[tauri::command]
pub fn delete_keyboard_action(
    app: tauri::AppHandle,
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    hotkey_manager: tauri::State<'_, Mutex<HotkeyManager>>,
    action: String,
) -> Result<(), String> {
    lock_state(&keyboard, "keyboard configuration lock is poisoned")?
        .delete_action(action.clone())
        .map_err(|error| error.to_string())?;
    // Deleting a global binding must unregister it immediately instead of
    // leaving a stale OS hotkey until restart. The registry check keeps this
    // working for future global actions without edits here.
    if is_global_action(&action) {
        crate::refresh_hotkey_registrations(&keyboard, &hotkey_manager, &app)?;
    }
    Ok(())
}

#[tauri::command]
pub fn reset_keyboard_config(
    app: tauri::AppHandle,
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    hotkey_manager: tauri::State<'_, Mutex<HotkeyManager>>,
) -> Result<KeyboardConfig, String> {
    {
        let mut km = lock_state(&keyboard, "keyboard configuration lock is poisoned")?;
        km.reset_to_defaults().map_err(|error| error.to_string())?;
    }
    let config = lock_state(&keyboard, "keyboard configuration lock is poisoned")?.config();
    crate::refresh_hotkey_registrations(&keyboard, &hotkey_manager, &app)?;
    Ok(config)
}

#[tauri::command]
pub fn paste_to_previous_application(
    app: tauri::AppHandle,
    hotkey_manager: tauri::State<'_, Mutex<HotkeyManager>>,
) -> Result<bool, String> {
    let target =
        lock_state(&hotkey_manager, "hotkey manager lock is poisoned")?.take_quick_paste_target();
    let Some(target) = target else {
        crate::dbg_log("paste_to_previous_application: no target");
        return Ok(false);
    };
    crate::dbg_log(&format!(
        "paste_to_previous_application: target=0x{target:X}"
    ));

    let main_window = app.get_webview_window("main");
    if let Some(window) = &main_window {
        window.hide().map_err(|error| error.to_string())?;
    }
    thread::sleep(Duration::from_millis(40));

    if let Err(error) = platform::windows::hotkey::restore_window_and_paste(target) {
        crate::dbg_log(&format!("paste_to_previous_application: error={error}"));
        if let Some(window) = &main_window {
            let _ = window.show();
            let _ = window.set_focus();
        }
        return Err(error);
    }

    crate::dbg_log("paste_to_previous_application: OK");
    Ok(true)
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
    let ignored_applications = lock_state(&config, "configuration lock is poisoned")?
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
    let mut monitor = lock_state(&monitor, "clipboard monitor lock is poisoned")?;
    let normalized = lock_state(&config, "configuration lock is poisoned")?
        .set_ignored_applications(applications)
        .map_err(|error| error.to_string())?;
    let normalized = capture.set_ignored_apps(normalized);
    monitor.set_ignored_apps(normalized.clone());
    Ok(normalized)
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
