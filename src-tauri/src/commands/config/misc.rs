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

/// OS UI locale name for the `language: "system"` setting. The webview's
/// `navigator.language` follows the Chromium accepted-language list, which on
/// Windows can disagree with the OS display language, so the frontend prefers
/// this answer and only falls back to `navigator.language` when it is `null`.
#[tauri::command]
pub fn get_os_locale() -> Option<String> {
    platform::os_locale_name()
}

#[tauri::command]
pub fn toggle_privacy_pause(
    app: tauri::AppHandle,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    privacy: tauri::State<'_, Mutex<PrivacyManager>>,
    capture: tauri::State<'_, CaptureState>,
) -> Result<bool, String> {
    // Hold the privacy lock through persistence to serialize with tray
    // toggles. On failure the settings command retains the previous state.
    let mut privacy = lock_state(&privacy, "privacy manager lock is poisoned")?;
    let paused = !privacy.is_paused();
    lock_state(&config, "configuration lock is poisoned")?
        .set_privacy_paused(paused)
        .map_err(|error| error.to_string())?;
    privacy.paused = paused;
    capture.set_paused(paused);
    drop(privacy);

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
    apply_privacy_settings(
        config.inner(),
        sync_cancellation.inner(),
        privacy.inner(),
        capture.inner(),
        local_only,
        sensitive_patterns,
    )
}

fn apply_privacy_settings(
    config: &Mutex<ConfigStore>,
    sync_cancellation: &crate::commands::sync::SyncCancellation,
    privacy: &Mutex<PrivacyManager>,
    capture: &CaptureState,
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

    // Use the same privacy -> config order as pause/getters, and retain the
    // privacy guard through both runtime mirrors. A later save must not apply
    // before this one, and a poisoned runtime must fail before persistence.
    let mut privacy = lock_state(&privacy, "privacy manager lock is poisoned")?;
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

        privacy.sensitive_patterns = compiled.clone();
        capture.set_sensitive_patterns(compiled);
    }

    // No event is emitted here: the command returns the fresh
    // `PrivacySettings` and the settings panel renders from that value.

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

#[tauri::command]
pub fn get_source_mapping_rules(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<Vec<crate::config::SourceMappingRule>, String> {
    Ok(lock_state(&config, "configuration lock is poisoned")?
        .source_mapping_rules()
        .to_vec())
}

#[tauri::command]
pub fn set_source_mapping_rules(
    app: tauri::AppHandle,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    rules: Vec<crate::config::SourceMappingRule>,
) -> Result<Vec<crate::config::SourceMappingRule>, String> {
    save_source_mapping_rules(config.inner(), rules, || {
        if let Err(error) = app.emit("source-mapping-rules-changed", ()) {
            crate::log_error!("[config] failed to emit source mapping rules change: {error}");
        }
    })
}

fn save_source_mapping_rules(
    config: &Mutex<ConfigStore>,
    rules: Vec<crate::config::SourceMappingRule>,
    notify: impl FnOnce(),
) -> Result<Vec<crate::config::SourceMappingRule>, String> {
    let saved = lock_state(&config, "configuration lock is poisoned")?
        .set_source_mapping_rules(rules)
        .map_err(|error| error.to_string())?;
    // A notification is an invalidation signal, not a possibly stale config
    // snapshot. Release the config lock before listeners re-read the latest rules.
    notify();
    Ok(saved)
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
pub async fn paste_to_previous_application(
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
        platform::hide_main_window(window).map_err(|error| error.to_string())?;
    }

    // The paste sequence sleeps and injects input; run it on the blocking
    // pool so the synchronous command does not freeze the main thread (and
    // with it every other window operation) for the whole duration.
    let paste_window = main_window.clone();
    tauri::async_runtime::spawn_blocking(move || {
        thread::sleep(Duration::from_millis(40));
        if let Err(error) = platform::windows::hotkey::restore_window_and_paste(target) {
            crate::dbg_log(&format!("paste_to_previous_application: error={error}"));
            if let Some(window) = &paste_window {
                let _ = platform::restore_main_window(window);
            }
            return Err(error);
        }
        crate::dbg_log("paste_to_previous_application: OK");
        Ok(true)
    })
    .await
    .map_err(|error| format!("paste_to_previous_application task failed: {error}"))?
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

#[cfg(test)]
mod tests {
    use super::{
        apply_ignored_applications, apply_privacy_settings, preview_auto_tag_rules,
        save_source_mapping_rules,
    };
    use crate::config::{AutoTagRule, ConfigStore, SourceMappingRule};
    use crate::domain::ClipboardKind;
    use crate::platform::ClipboardMonitor;
    use crate::privacy::PrivacyManager;
    use crate::state::CaptureState;
    use std::fs;

    #[test]
    fn unavailable_privacy_runtime_cannot_commit_a_new_capture_policy() {
        let root =
            std::env::temp_dir().join(format!("clipboard-privacy-policy-{}", uuid::Uuid::new_v4()));
        let mut store = ConfigStore::load(&root).unwrap();
        store
            .set_sensitive_patterns(vec!["old-secret".into()])
            .unwrap();
        let mut manager = PrivacyManager::new();
        manager.sync_with_config(&store);
        let capture = CaptureState::new(&manager, vec![], 0, 100_000);
        let config = Mutex::new(store);
        let privacy = std::sync::Arc::new(Mutex::new(manager));
        let poisoned = privacy.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("simulate a failed privacy operation");
        })
        .join();
        let result = apply_privacy_settings(
            &config,
            &Default::default(),
            &privacy,
            &capture,
            None,
            Some(vec!["new-secret".into()]),
        );
        assert!(result.is_err());
        assert_eq!(config.lock().unwrap().sensitive_patterns(), &["old-secret"]);
        assert_eq!(
            ConfigStore::load(&root).unwrap().sensitive_patterns(),
            &["old-secret"]
        );
        assert!(capture.should_skip(None, Some("old-secret")));
        assert!(!capture.should_skip(None, Some("new-secret")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn privacy_policy_updates_apply_only_after_a_successful_save() {
        let root =
            std::env::temp_dir().join(format!("clipboard-privacy-policy-{}", uuid::Uuid::new_v4()));
        let store = ConfigStore::load(&root).unwrap();
        let privacy = Mutex::new(PrivacyManager::new());
        let capture = CaptureState::new(&privacy.lock().unwrap(), vec![], 0, 100_000);
        let config = Mutex::new(store);
        let cancellation = Default::default();
        apply_privacy_settings(
            &config,
            &cancellation,
            &privacy,
            &capture,
            None,
            Some(vec!["old-secret".into()]),
        )
        .unwrap();
        fs::rename(root.join("conf"), root.join("saved-conf")).unwrap();
        assert!(apply_privacy_settings(
            &config,
            &cancellation,
            &privacy,
            &capture,
            None,
            Some(vec!["new-secret".into()])
        )
        .is_err());
        assert!(privacy.lock().unwrap().is_sensitive_content("old-secret"));
        assert!(capture.should_skip(None, Some("old-secret")));
        assert!(!capture.should_skip(None, Some("new-secret")));
        fs::rename(root.join("saved-conf"), root.join("conf")).unwrap();
        assert!(apply_privacy_settings(
            &config,
            &cancellation,
            &privacy,
            &capture,
            None,
            Some(vec!["[".into()])
        )
        .is_err());
        let saved = apply_privacy_settings(
            &config,
            &cancellation,
            &privacy,
            &capture,
            Some(true),
            Some(vec!["new-secret".into()]),
        )
        .unwrap();
        assert!(saved.local_only);
        assert_eq!(saved.sensitive_patterns, vec!["new-secret"]);
        assert!(privacy.lock().unwrap().is_sensitive_content("new-secret"));
        assert!(!capture.should_skip(None, Some("old-secret")));
        assert!(capture.should_skip(None, Some("new-secret")));
        assert_eq!(
            ConfigStore::load(&root).unwrap().sensitive_patterns(),
            &["new-secret"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_privacy_saves_wait_for_the_previous_runtime_application() {
        use std::sync::{mpsc, Arc};
        use std::time::{Duration, Instant};
        let root =
            std::env::temp_dir().join(format!("clipboard-privacy-policy-{}", uuid::Uuid::new_v4()));
        let config = Arc::new(Mutex::new(ConfigStore::load(&root).unwrap()));
        let privacy = Arc::new(Mutex::new(PrivacyManager::new()));
        let capture = CaptureState::new(&privacy.lock().unwrap(), vec![], 0, 100_000);
        let cancellation = Arc::new(crate::commands::sync::SyncCancellation::default());
        // Block the real capture-policy application, after the first save.
        let blocked_capture = capture.policy.sensitive_patterns.write().unwrap();
        let spawn_update = |pattern: &str, started: Option<mpsc::Sender<()>>| {
            let config = config.clone();
            let privacy = privacy.clone();
            let capture = capture.clone();
            let cancellation = cancellation.clone();
            let pattern = pattern.to_owned();
            std::thread::spawn(move || {
                if let Some(started) = started {
                    started.send(()).unwrap();
                }
                apply_privacy_settings(
                    &config,
                    &cancellation,
                    &privacy,
                    &capture,
                    None,
                    Some(vec![pattern]),
                )
            })
        };
        let first = spawn_update("first-secret", None);
        let deadline = Instant::now() + Duration::from_secs(3);
        let first_saved = loop {
            if config.lock().unwrap().sensitive_patterns() == ["first-secret"] {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let (started, receiver) = mpsc::channel();
        let second = spawn_update("second-secret", Some(started));
        receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        let deadline = Instant::now() + Duration::from_millis(200);
        let saved_ahead = loop {
            if config.lock().unwrap().sensitive_patterns() == ["second-secret"] {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        drop(blocked_capture);
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        assert!(first_saved, "first request must reach persistence");
        assert!(
            !saved_ahead,
            "second request must not save ahead of the blocked runtime update"
        );
        assert!(capture.should_skip(None, Some("second-secret")));
        assert!(!capture.should_skip(None, Some("first-secret")));
        assert!(privacy
            .lock()
            .unwrap()
            .is_sensitive_content("second-secret"));
        assert_eq!(
            ConfigStore::load(&root).unwrap().sensitive_patterns(),
            &["second-secret"]
        );
        fs::remove_dir_all(root).unwrap();
    }
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn config_fixture(name: &str) -> (PathBuf, Mutex<ConfigStore>) {
        let root = std::env::temp_dir().join(format!(
            "clipboard-config-misc-{name}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        let store = Mutex::new(ConfigStore::load(&root).unwrap());
        (root, store)
    }

    fn rule(
        pattern: &str,
        tag: &str,
        source_app: &str,
        kind: Option<ClipboardKind>,
    ) -> AutoTagRule {
        AutoTagRule {
            pattern: pattern.to_owned(),
            tag: tag.to_owned(),
            source_app: source_app.to_owned(),
            kind,
        }
    }

    #[test]
    fn source_mapping_notifies_only_after_normalized_rules_are_persisted() {
        let (root, config) = config_fixture("source-mapping-save");
        let expected = vec![SourceMappingRule {
            domain: "example.com".into(),
            title_keyword: "Topic".into(),
            name: "Example".into(),
        }];
        let mut notified = false;
        let saved = save_source_mapping_rules(
            &config,
            vec![SourceMappingRule {
                domain: " Example.COM ".into(),
                title_keyword: " Topic ".into(),
                name: " Example ".into(),
            }],
            || {
                notified = true;
                // Both reads would expose a premature notification; taking
                // the mutex here also proves it is released before notifying.
                assert_eq!(config.lock().unwrap().source_mapping_rules(), expected);
                assert_eq!(
                    ConfigStore::load(&root).unwrap().source_mapping_rules(),
                    expected
                );
            },
        )
        .unwrap();
        assert!(notified);
        assert_eq!(saved, expected);
        drop(config);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_mapping_save_failure_keeps_rules_and_does_not_notify() {
        let (root, config) = config_fixture("source-mapping-failure");
        let original = vec![SourceMappingRule {
            domain: "example.com".into(),
            title_keyword: String::new(),
            name: "Original".into(),
        }];
        config
            .lock()
            .unwrap()
            .set_source_mapping_rules(original.clone())
            .unwrap();
        let path = config.lock().unwrap().path().to_owned();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let mut notified = false;
        let result = save_source_mapping_rules(&config, Vec::new(), || notified = true);
        assert!(result.is_err());
        assert!(!notified);
        assert_eq!(config.lock().unwrap().source_mapping_rules(), original);
        drop(config);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preview_auto_tag_rules_matches_pattern_source_app_and_kind() {
        let rules = vec![
            rule("TODO|FIXME", "work", "code", None),
            rule("TODO", "from-image", "", Some(ClipboardKind::Image)),
        ];

        // Text kind: the source-app-conditional rule matches, the image-only
        // rule must not.
        let tags = preview_auto_tag_rules(
            rules.clone(),
            "TODO: fix the parser".to_owned(),
            "Visual Studio Code".to_owned(),
            ClipboardKind::Text,
        )
        .unwrap();
        assert_eq!(tags, vec!["work".to_owned()]);

        // Image kind: both rules apply (kind-free rule + image-only rule).
        let tags = preview_auto_tag_rules(
            rules.clone(),
            "TODO: fix the parser".to_owned(),
            "code.exe".to_owned(),
            ClipboardKind::Image,
        )
        .unwrap();
        assert_eq!(tags, vec!["work".to_owned(), "from-image".to_owned()]);

        // Source app outside the condition suppresses the first rule even
        // though the pattern matches.
        let tags = preview_auto_tag_rules(
            rules,
            "TODO: fix the parser".to_owned(),
            "notepad".to_owned(),
            ClipboardKind::Text,
        )
        .unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn preview_auto_tag_rules_rejects_oversized_samples() {
        let oversized_text = "a".repeat(1_000_001);
        let error = preview_auto_tag_rules(
            Vec::new(),
            oversized_text,
            "app".to_owned(),
            ClipboardKind::Text,
        )
        .unwrap_err();
        assert_eq!(error, "sample is too large");

        let oversized_app = "a".repeat(4097);
        let error = preview_auto_tag_rules(
            Vec::new(),
            "text".to_owned(),
            oversized_app,
            ClipboardKind::Text,
        )
        .unwrap_err();
        assert_eq!(error, "sample is too large");

        // Exactly at the limits the preview still runs.
        let boundary_text = "b".repeat(1_000_000);
        let tags = preview_auto_tag_rules(
            Vec::new(),
            boundary_text,
            "a".repeat(4096),
            ClipboardKind::Text,
        )
        .unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn preview_auto_tag_rules_rejects_invalid_rule_patterns() {
        let error = preview_auto_tag_rules(
            vec![rule("([", "broken", "", None)],
            "text".to_owned(),
            "app".to_owned(),
            ClipboardKind::Text,
        )
        .unwrap_err();
        assert!(error.contains("invalid auto-tag pattern"), "{error}");

        let error = preview_auto_tag_rules(
            vec![rule("", "empty", "", None)],
            "text".to_owned(),
            "app".to_owned(),
            ClipboardKind::Text,
        )
        .unwrap_err();
        assert!(error.contains("needs a pattern, source or type"), "{error}");
    }

    #[test]
    fn apply_ignored_applications_normalizes_and_propagates_everywhere() {
        let (root, config) = config_fixture("ignored");
        let monitor = Mutex::new(ClipboardMonitor::new());
        let privacy = PrivacyManager::new();
        let capture = CaptureState::new(&privacy, Vec::new(), 10 * 1024 * 1024, 100 * 1024);

        // Trimming, empty-entry filtering, and case-insensitive dedup all
        // happen before the value reaches config, capture, and the monitor.
        let normalized = apply_ignored_applications(
            &config,
            &monitor,
            &capture,
            vec![" Notepad ".to_owned(), "notepad".to_owned(), String::new()],
        )
        .unwrap();
        assert_eq!(normalized, vec!["Notepad".to_owned()]);
        assert_eq!(
            config.lock().unwrap().ignored_applications().to_vec(),
            vec!["Notepad".to_owned()]
        );
        assert_eq!(
            monitor.lock().unwrap().ignored_applications,
            vec!["Notepad".to_owned()]
        );
        assert_eq!(capture.ignored_apps(), vec!["Notepad".to_owned()]);

        // A follow-up call replaces the list instead of appending to it.
        let normalized =
            apply_ignored_applications(&config, &monitor, &capture, vec!["chrome.exe".to_owned()])
                .unwrap();
        assert_eq!(normalized, vec!["chrome.exe".to_owned()]);
        assert_eq!(capture.ignored_apps(), vec!["chrome.exe".to_owned()]);
        assert_eq!(
            monitor.lock().unwrap().ignored_applications,
            vec!["chrome.exe".to_owned()]
        );

        drop(config);
        let _ = fs::remove_dir_all(&root);
    }
}
