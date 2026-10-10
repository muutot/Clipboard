use super::*;

#[test]
fn wayland_capabilities_summary_is_not_empty() {
    let caps = WaylandCapabilities::unknown();
    let summary = caps.summary();
    assert!(!summary.is_empty());
    assert!(summary.contains("Unknown"));
}

#[test]
fn classify_rejects_non_wayland_sessions() {
    let caps = WaylandCapabilities::classify("x11", "GNOME", false, false);
    assert_eq!(caps, WaylandCapabilities::unknown());
}

#[test]
fn classify_sway_uses_the_wlroots_profile() {
    let caps = WaylandCapabilities::classify("wayland", "sway", true, false);
    assert_eq!(caps.compositor, "Sway");
    assert!(caps.clipboard_read);
    assert!(!caps.requires_config);
}

#[test]
fn classify_falls_back_to_the_sway_socket_name() {
    // Some sessions export a generic desktop string but still own sway's IPC
    // socket; the socket is the stronger signal.
    let caps = WaylandCapabilities::classify("wayland", "wlroots", true, false);
    assert_eq!(caps.compositor, "Sway");
    assert!(caps.clipboard_write);
}

#[test]
fn classify_hyprland_from_its_instance_signature() {
    let caps = WaylandCapabilities::classify("wayland", "", false, true);
    assert_eq!(caps.compositor, "Hyprland");
    assert!(caps.global_shortcuts);
    assert!(!caps.requires_config);
}

#[test]
fn classify_kde_plasma_has_no_primary_selection() {
    let caps = WaylandCapabilities::classify("wayland", "KDE", false, false);
    assert_eq!(caps.compositor, "KDE Plasma");
    assert!(caps.clipboard_read);
    assert!(caps.clipboard_write);
    assert!(!caps.primary_selection);
    assert!(!caps.requires_config);
}

#[test]
fn classify_gnome_blocks_clipboard_and_requires_config() {
    let caps = WaylandCapabilities::classify("wayland", "ubuntu:GNOME", false, false);
    assert_eq!(caps.compositor, "GNOME Shell");
    assert!(!caps.clipboard_read);
    assert!(!caps.clipboard_write);
    assert!(caps.global_shortcuts);
    assert!(caps.requires_config);
    assert_eq!(caps.notes.len(), 2);
}

#[test]
fn classify_unknown_compositor_stays_conservative() {
    let caps = WaylandCapabilities::classify("wayland", "niri", false, false);
    assert_eq!(caps.compositor, "niri (unverified)");
    assert!(!caps.clipboard_read);
    assert!(caps.requires_config);
}

#[test]
fn classify_blank_compositor_falls_back_to_unknown() {
    let caps = WaylandCapabilities::classify("wayland", "", false, false);
    assert_eq!(caps, WaylandCapabilities::unknown());
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
