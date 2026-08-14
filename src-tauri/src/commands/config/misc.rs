use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::config::ConfigStore;
use crate::keyboard::{KeyboardConfig, KeyboardManager};
use crate::platform::windows_hotkey::HotkeyManager;
use crate::platform::{self, ClipboardMonitor, RuntimeInfo};
use crate::privacy::PrivacyManager;
use crate::storage::{ClipboardRepository, Database};
use crate::{resolve_toggle_hotkeys, CaptureState, TOGGLE_WINDOW_ACTION};

use super::{ApplicationFilterSettings, DiscoveredApplication, PrivacySettings, PrivacyStatus};

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

    let _ = app.emit("privacy-pause-changed", paused);

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
pub fn get_privacy_settings(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
) -> Result<PrivacySettings, String> {
    let privacy = privacy
        .lock()
        .map_err(|_| "privacy manager lock is poisoned".to_owned())?;
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;

    Ok(PrivacySettings {
        paused: privacy.is_paused(),
        local_only: config.privacy_local_only(),
        capture_sensitive_sources: config.privacy_capture_sensitive_sources(),
        sensitive_patterns: config.sensitive_patterns().to_vec(),
        password_manager_apps: privacy.password_manager_apps.clone(),
    })
}

#[tauri::command]
pub fn set_privacy_settings(
    app: tauri::AppHandle,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    capture: tauri::State<'_, CaptureState>,
    local_only: Option<bool>,
    capture_sensitive_sources: Option<bool>,
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
        let mut config = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        if let Some(value) = local_only {
            config
                .set_privacy_local_only(value)
                .map_err(|error| error.to_string())?;
        }
        if let Some(value) = capture_sensitive_sources {
            config
                .set_privacy_capture_sensitive_sources(value)
                .map_err(|error| error.to_string())?;
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
        privacy
            .lock()
            .map_err(|_| "privacy manager lock is poisoned".to_owned())?
            .sensitive_patterns = compiled.clone();
        capture.set_sensitive_patterns(compiled);
    }
    if let Some(value) = capture_sensitive_sources {
        capture.set_capture_sensitive_sources(value);
    }

    let _ = app.emit("privacy-settings-changed", ());

    let privacy = privacy
        .lock()
        .map_err(|_| "privacy manager lock is poisoned".to_owned())?;
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;

    Ok(PrivacySettings {
        paused: privacy.is_paused(),
        local_only: config.privacy_local_only(),
        capture_sensitive_sources: config.privacy_capture_sensitive_sources(),
        sensitive_patterns: config.sensitive_patterns().to_vec(),
        password_manager_apps: privacy.password_manager_apps.clone(),
    })
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

    if let Err(error) = platform::windows_hotkey::restore_window_and_paste(target) {
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
pub fn set_clipboard_ignored_apps(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
    apps: Vec<String>,
) -> Result<Vec<String>, String> {
    apply_ignored_applications(config.inner(), monitor.inner(), capture.inner(), apps)
}
