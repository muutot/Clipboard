use super::*;

#[test]
fn clipboard_monitor_lifecycle() {
    let mut monitor = MacOSClipboardMonitor::new();
    assert!(!monitor.is_running());

    monitor.start().unwrap();
    assert!(monitor.is_running());

    monitor.stop().unwrap();
    assert!(!monitor.is_running());
}

#[test]
fn clipboard_monitor_ignored_apps() {
    let mut monitor = MacOSClipboardMonitor::new();
    monitor.set_ignored_apps(vec![
        "com.agilebits.onepassword".to_owned(),
        "com.bitwarden.desktop".to_owned(),
    ]);
    let apps = monitor.ignored_apps();
    assert_eq!(apps.len(), 2);
    assert!(apps.contains(&"com.agilebits.onepassword".to_owned()));
}

#[test]
fn keyboard_hook_creates_and_destroys() {
    let mut hook = MacOSKeyboardHook::new();
    assert!(!hook.is_active());

    hook.register("test", &[]).unwrap();
    hook.unregister_all().unwrap();
}

#[test]
fn key_code_to_name_returns_expected_values() {
    assert_eq!(MacOSKeyboardHook::key_code_to_name(0), "A");
    assert_eq!(MacOSKeyboardHook::key_code_to_name(49), "Space");
    assert_eq!(MacOSKeyboardHook::key_code_to_name(36), "Return");
    assert_eq!(MacOSKeyboardHook::key_code_to_name(53), "Escape");
    assert_eq!(MacOSKeyboardHook::key_code_to_name(999), "Unknown");
}

#[test]
fn accessibility_helper_status_is_string() {
    let status = MacOSAccessibilityHelper::status_description();
    assert!(!status.is_empty());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn accessibility_permission_request_on_non_macos() {
    // Should succeed on non-macOS (stub returns Ok).
    assert!(MacOSAccessibilityHelper::request_permission().is_ok());
}

#[test]
fn tray_default_menu_is_not_empty() {
    let menu = MacOSTrayManager::default_menu();
    assert!(!menu.is_empty());
    let has_separator = menu
        .iter()
        .any(|item| matches!(item, MacOSTrayMenuItem::Separator));
    assert!(has_separator);
}

#[test]
fn tray_create_and_set_menu() {
    let mut tray = MacOSTrayManager::create().unwrap();
    let menu = MacOSTrayManager::default_menu();
    tray.set_menu(&menu).unwrap();
}

/// Smoke test for the `NSFilenamesPboardType` reader: it must never crash
/// the process regardless of pasteboard contents (an absent file list
/// yields an empty vector, the historical behavior). Only meaningful on
/// macOS; elsewhere the stub is compiled instead.
#[cfg(target_os = "macos")]
#[test]
fn file_paths_read_does_not_panic() {
    let _ = read_clipboard_file_paths();
}
