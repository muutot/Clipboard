use super::*;

#[test]
fn x11_error_display() {
    let err = X11Error::DisplayOpenFailed(":0".to_owned());
    assert!(err.to_string().contains(":0"));
}

#[test]
fn clipboard_monitor_lifecycle() {
    let mut monitor = X11ClipboardMonitor::new();
    assert!(!monitor.is_running());

    // On non-Linux, start/stop are no-ops.
    monitor.start().unwrap();
    assert!(monitor.is_running());

    monitor.stop().unwrap();
    assert!(!monitor.is_running());
}

#[test]
fn ignored_apps_management() {
    let mut monitor = X11ClipboardMonitor::new();
    monitor.set_ignored_apps(vec!["firefox".to_owned(), "chromium".to_owned()]);
    let apps = monitor.ignored_apps();
    assert_eq!(apps.len(), 2);
}

#[test]
fn hotkey_manager_creates_and_destroys() {
    let mut hotkey = X11GlobalHotkey::new();
    assert!(!hotkey.is_active());
    assert_eq!(hotkey.count(), 0);

    hotkey.register("test", &[]).unwrap();
    hotkey.unregister_all().unwrap();
}

#[test]
fn tray_creation() {
    let tray = X11TrayManager::create().unwrap();
    assert_eq!(tray.backend(), X11TrayBackend::None);
}

#[test]
fn tray_default_menu_not_empty() {
    let menu = X11TrayManager::default_menu();
    assert!(!menu.is_empty());
    let has_quit = menu
        .iter()
        .any(|item| matches!(item, X11TrayMenuItem::Item { action, .. } if action == "quit"));
    assert!(has_quit);
}

#[test]
fn tray_backend_info_has_entries() {
    let info = X11TrayManager::backend_info();
    assert!(!info.is_empty());
}

#[test]
fn x11_selection_enum_values() {
    assert_ne!(X11Selection::Primary, X11Selection::Clipboard);
}
