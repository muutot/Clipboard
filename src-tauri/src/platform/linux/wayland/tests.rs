use super::*;

#[test]
fn wayland_capabilities_summary_is_not_empty() {
    let caps = WaylandCapabilities::unknown();
    let summary = caps.summary();
    assert!(!summary.is_empty());
    assert!(summary.contains("Unknown"));
}

#[test]
fn wlroots_capabilities_have_full_support() {
    let caps = WaylandCapabilities::wlroots_based("sway");
    assert!(caps.clipboard_read);
    assert!(caps.clipboard_write);
    assert!(caps.primary_selection);
    assert!(caps.global_shortcuts);
    assert!(caps.system_tray);
}

#[test]
fn clipboard_monitor_lifecycle() {
    let mut monitor = WaylandClipboardMonitor::new();
    assert!(!monitor.is_running());

    monitor.start().unwrap();
    assert!(monitor.is_running());

    monitor.stop().unwrap();
    assert!(!monitor.is_running());
}

#[test]
fn ignored_apps() {
    let mut monitor = WaylandClipboardMonitor::new();
    monitor.set_ignored_apps(vec!["kitty".to_owned(), "alacritty".to_owned()]);
    assert_eq!(monitor.ignored_apps().len(), 2);
}

#[test]
fn global_shortcut_instructions_not_empty() {
    let shortcut = WaylandGlobalShortcut::new();
    let instructions = shortcut.setup_instructions();
    assert!(!instructions.is_empty());
}

#[test]
fn tray_default_menu_has_items() {
    let menu = WaylandTrayManager::default_menu();
    assert!(!menu.is_empty());
}

#[test]
fn tray_compositor_support_table() {
    let table = WaylandTrayManager::compositor_tray_support();
    assert!(!table.is_empty());
    assert!(table.iter().any(|(name, _, _)| *name == "Sway / wlroots"));
}

#[test]
fn compositor_info_table_contains_entries() {
    let info = WaylandCompositorInfo::all();
    assert!(info.len() >= 4);
    assert!(info.iter().any(|c| c.name == "Sway"));
    assert!(info.iter().any(|c| c.name == "GNOME Shell"));
    assert!(info.iter().any(|c| c.name == "KDE Plasma"));
    assert!(info.iter().any(|c| c.name == "Hyprland"));
}

#[test]
fn compatibility_table_is_markdown() {
    let table = WaylandCompositorInfo::compatibility_table();
    assert!(table.starts_with("| Compositor"));
    assert!(table.contains("| Sway |"));
    assert!(table.contains("| GNOME Shell |"));
}
