// ---------------------------------------------------------------------------
//  Platform-specific adapter modules
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(not(target_os = "macos"))]
#[path = "macos.rs"]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux_x11;
#[cfg(not(target_os = "linux"))]
#[path = "linux_x11.rs"]
pub mod linux_x11;

#[cfg(target_os = "linux")]
pub mod linux_wayland;
#[cfg(not(target_os = "linux"))]
#[path = "linux_wayland.rs"]
pub mod linux_wayland;

pub mod windows_clipboard;
#[cfg(target_os = "windows")]
pub mod windows_hotkey;
#[cfg(not(target_os = "windows"))]
#[path = "windows_hotkey_stub.rs"]
pub mod windows_hotkey;

// ---------------------------------------------------------------------------
//  Shared submodules
// ---------------------------------------------------------------------------

pub mod autostart;
pub mod dispatch;
pub mod monitor;
pub mod platform_info;
pub mod shortcuts;
pub mod single_instance;
pub mod ui;

// infra module is deprecated — items migrated to autostart and single_instance
#[allow(deprecated)]
pub mod infra;

// ---------------------------------------------------------------------------
//  Re-exports so items remain at crate::platform::*
// ---------------------------------------------------------------------------

pub use autostart::{decide_autostart_action, sync_autostart, AutostartAction};
pub use dispatch::{platform, PlatformClipboard};
pub use monitor::ClipboardMonitor;
pub use platform_info::{
    current_capabilities, get_platform_info, runtime_info, ClipboardPlatform, ForegroundApp,
    Platform, PlatformCapabilities, PlatformInfo, RuntimeInfo,
};
pub use shortcuts::GlobalShortcutManager;
pub use single_instance::{SingleInstanceError, SingleInstanceGuard};
pub use ui::{
    apply_window_effect, apply_window_transparency, disk_space, show_main_window,
    window_transparency_alpha, DiskSpace, SystemTray, WindowManager,
};

// ---------------------------------------------------------------------------
//  Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, path::PathBuf, time::SystemTime};

    #[cfg(target_os = "windows")]
    use std::{sync::mpsc, time::Duration};

    use super::*;
    use crate::config::ConfigStore;
    use crate::keyboard::ShortcutBinding;
    use std::str::FromStr;

    #[test]
    fn platform_detect_returns_variant() {
        let p = Platform::detect();
        assert!(matches!(
            p,
            Platform::Windows
                | Platform::MacOS
                | Platform::LinuxX11
                | Platform::LinuxWayland
                | Platform::Unknown
        ));
    }

    #[test]
    fn platform_display_is_human_readable() {
        let p = Platform::detect();
        let s = p.to_string();
        assert!(!s.is_empty());
    }

    #[test]
    fn platform_info_returns_valid_data() {
        let info = get_platform_info();
        assert_eq!(info.operating_system, std::env::consts::OS);
        assert_eq!(info.architecture, std::env::consts::ARCH);
        assert!(!info.app_version.is_empty());
        assert_eq!(
            info.clipboard_monitoring_supported,
            info.capabilities.clipboard_monitoring
        );
        assert_eq!(
            info.global_shortcut_supported,
            info.capabilities.global_shortcut
        );
        assert_eq!(info.system_tray_supported, info.capabilities.system_tray);
    }

    #[test]
    fn clipboard_monitor_starts_and_stops() {
        let mut monitor = ClipboardMonitor::new();
        assert!(!monitor.running);

        monitor.start().unwrap();
        assert!(monitor.running);

        monitor.stop().unwrap();
        assert!(!monitor.running);
    }

    #[test]
    fn clipboard_monitor_manages_ignored_apps() {
        let mut monitor = ClipboardMonitor::new();
        monitor.set_ignored_apps(vec!["App1".to_owned(), "App2".to_owned()]);
        assert_eq!(monitor.ignored_applications.len(), 2);
    }

    #[test]
    fn global_shortcut_manager_registers_and_unregisters() {
        let mut manager = GlobalShortcutManager::new();
        let binding = ShortcutBinding::from_str("Ctrl+V").unwrap();

        manager.register("paste", &[binding]).unwrap();
        assert_eq!(manager.shortcuts.len(), 1);

        manager.unregister_all().unwrap();
        assert!(manager.shortcuts.is_empty());
    }

    #[test]
    fn autostart_action_only_changes_mismatched_state() {
        assert_eq!(
            decide_autostart_action(true, false),
            AutostartAction::Enable
        );
        assert_eq!(
            decide_autostart_action(false, true),
            AutostartAction::Disable
        );
        assert_eq!(
            decide_autostart_action(true, true),
            AutostartAction::NoChange
        );
        assert_eq!(
            decide_autostart_action(false, false),
            AutostartAction::NoChange
        );
    }

    #[test]
    fn window_manager_persists_position_through_config() {
        let project = temporary_test_directory("window-manager");
        let mut config = ConfigStore::load(&project).unwrap();

        WindowManager::save_position(&mut config, 10, 20, 800, 600).unwrap();
        let restored = WindowManager::restore_position(&config);

        assert_eq!(restored, Some((10, 20, 800, 600)));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn window_transparency_alpha_maps_percent_to_layered_alpha() {
        assert_eq!(window_transparency_alpha(100), 255);
        assert_eq!(window_transparency_alpha(95), 242);
        assert_eq!(window_transparency_alpha(60), 153);
        assert_eq!(window_transparency_alpha(0), 153);
        assert_eq!(window_transparency_alpha(255), 255);
    }

    #[test]
    fn window_transparency_rejects_a_null_handle() {
        assert!(apply_window_transparency(0, 95).is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn disk_space_reports_the_volume_capacity() {
        let space = disk_space(&std::env::temp_dir()).expect("temp volume should be queryable");
        assert!(space.total_bytes > 0);
        assert!(space.available_bytes <= space.total_bytes);
    }

    #[test]
    fn disk_space_returns_none_for_a_missing_path() {
        assert!(disk_space(Path::new("Z:\\definitely\\missing\\path")).is_none());
    }

    #[test]
    fn single_instance_guard_acquires_and_releases_lock() {
        let project = temporary_test_directory("instance-guard");
        fs::create_dir_all(&project).unwrap();
        let lock_path = project.join("instance.lock");

        let guard = SingleInstanceGuard::acquire(&project).unwrap();
        assert!(lock_path.exists());

        let second = SingleInstanceGuard::acquire(&project);
        assert!(matches!(
            second,
            Err(SingleInstanceError::AlreadyRunning(pid)) if pid == std::process::id()
        ));

        drop(guard);
        assert!(!lock_path.exists());

        fs::remove_dir_all(project).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn single_instance_conflict_notifies_wake_listener() {
        let project = temporary_test_directory("instance-wake");
        fs::create_dir_all(&project).unwrap();

        let mut guard = SingleInstanceGuard::acquire(&project).unwrap();
        let (sender, receiver) = mpsc::channel();
        guard
            .start_wake_listener(move || {
                let _ = sender.send(());
            })
            .unwrap();

        let Err(SingleInstanceError::AlreadyRunning(owner_pid)) =
            SingleInstanceGuard::acquire(&project)
        else {
            panic!("expected the second instance to detect the owner");
        };
        assert_eq!(owner_pid, std::process::id());
        assert!(matches!(
            receiver.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(SingleInstanceGuard::notify_existing_instance(
            &project, owner_pid
        ));
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();

        drop(guard);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn single_instance_notification_without_listener_is_safe() {
        assert!(!SingleInstanceGuard::notify_existing_instance(
            Path::new("missing"),
            0
        ));
    }

    #[test]
    fn single_instance_guard_recovers_corrupted_and_stale_locks() {
        let project = temporary_test_directory("instance-recovery");
        fs::create_dir_all(&project).unwrap();
        let lock_path = project.join("instance.lock");

        fs::write(&lock_path, "not-a-pid\n").unwrap();
        let corrupted_guard = SingleInstanceGuard::acquire(&project).unwrap();
        assert_eq!(
            fs::read_to_string(&lock_path).unwrap().trim(),
            std::process::id().to_string()
        );
        drop(corrupted_guard);

        fs::write(&lock_path, format!("{}\n", i32::MAX)).unwrap();
        let stale_guard = SingleInstanceGuard::acquire(&project).unwrap();
        assert_eq!(
            fs::read_to_string(&lock_path).unwrap().trim(),
            std::process::id().to_string()
        );
        drop(stale_guard);

        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn single_instance_guard_only_removes_the_lock_it_owns() {
        let project = temporary_test_directory("instance-ownership");
        fs::create_dir_all(&project).unwrap();
        let lock_path = project.join("instance.lock");

        let guard = SingleInstanceGuard::acquire(&project).unwrap();
        fs::write(&lock_path, format!("{}\n", i32::MAX)).unwrap();
        drop(guard);

        assert!(lock_path.exists());
        fs::remove_file(&lock_path).unwrap();
        fs::remove_dir_all(project).unwrap();
    }

    fn temporary_test_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "clipboard-platform-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
