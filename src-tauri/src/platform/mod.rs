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
pub mod dpapi;
pub mod hotkey_common;
pub mod linux_icons;
pub mod monitor;
pub mod platform_info;
pub mod secret_store;
pub mod single_instance;
pub mod ui;

use std::path::Path;

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
pub use single_instance::{SingleInstanceError, SingleInstanceGuard};
pub use ui::{
    apply_window_effect, apply_window_transparency, disk_space, refresh_tray_recent_menu,
    show_main_window, window_transparency_alpha, DiskSpace, SystemTray, WindowManager,
};

// ---------------------------------------------------------------------------
//  Shared helpers
// ---------------------------------------------------------------------------

/// Parses `text/uri-list` clipboard output into local file paths.
///
/// Skips blank lines and `#` comments and keeps only `file://` entries with
/// the scheme prefix stripped. Shared by the X11 (`xclip`) and Wayland
/// (`wl-paste`) readers so both backends agree on edge cases.
///
/// Note: `file://host/path` prefixes and percent-encoding pass through
/// unchanged; downstream storage treats the result as an opaque path hint.
pub fn parse_uri_list(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            line.strip_prefix("file://").map(|p| p.to_owned())
        })
        .collect()
}

/// Extracts the `.app` bundle directory from a macOS executable path:
/// `/Applications/Foo.app/Contents/MacOS/Foo` -> `/Applications/Foo.app`.
/// Returns `None` when the path is not inside a `.app` bundle.
pub fn macos_app_bundle_from_exe(exe_path: &str) -> Option<&Path> {
    let exe = Path::new(exe_path);
    let bundle = exe.parent()?.parent()?.parent()?;
    bundle
        .extension()
        .is_some_and(|extension| extension == "app")
        .then_some(bundle)
}

// ---------------------------------------------------------------------------
//  Non-Windows clipboard polling
// ---------------------------------------------------------------------------

/// Change detector for the polling clipboard monitor used on platforms
/// without a clipboard sequence number. It fingerprints text, file paths, and
/// image pixels so image/file copies are detected even when the clipboard
/// never transitions through text. The first text observation follows the
/// original text comparison (a pre-existing text selection is reported once);
/// the first non-text observation is not reported, so an image or file list
/// already on the clipboard at startup is not captured.
#[derive(Default)]
pub struct ClipboardPollState {
    first_tick: bool,
    text: Option<String>,
    files: Vec<String>,
    image: Option<(Vec<u8>, u32, u32)>,
}

impl ClipboardPollState {
    pub fn new() -> Self {
        Self {
            first_tick: true,
            ..Self::default()
        }
    }

    /// Records the current clipboard state and returns whether it differs from
    /// the previous state. `image` is `None` when the caller did not probe one
    /// (for example because the clipboard had text or files).
    pub fn observe(
        &mut self,
        text: Option<String>,
        files: Vec<String>,
        image: Option<(Vec<u8>, u32, u32)>,
    ) -> bool {
        let changed = if text.is_some() {
            self.text != text
        } else if self.first_tick {
            false
        } else {
            // A text->non-text transition, or changed file/image content.
            self.text.is_some() || files != self.files || image != self.image
        };

        if text.is_some() {
            self.files.clear();
            self.image = None;
        } else {
            self.files = files;
            self.image = image;
        }
        self.text = text;
        self.first_tick = false;
        changed
    }
}

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
    fn parse_uri_list_keeps_only_file_entries() {
        let parsed = super::parse_uri_list(
            "# comment\n\nfile:///home/user/a.txt\n  file:///home/user/b.txt  \nhttp://example.com/x\ntext/plain\n",
        );
        assert_eq!(parsed, vec!["/home/user/a.txt", "/home/user/b.txt"]);
    }

    #[test]
    fn parse_uri_list_empty_input_yields_no_paths() {
        assert!(super::parse_uri_list("").is_empty());
        assert!(super::parse_uri_list("# only a comment\n   \n").is_empty());
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

    #[test]
    fn macos_app_bundle_extraction_walks_to_the_bundle() {
        assert_eq!(
            super::macos_app_bundle_from_exe("/Applications/Foo.app/Contents/MacOS/Foo"),
            Some(Path::new("/Applications/Foo.app"))
        );
        // Not inside a .app bundle.
        assert_eq!(super::macos_app_bundle_from_exe("/usr/bin/ls"), None);
        assert_eq!(super::macos_app_bundle_from_exe("Foo"), None);
    }

    #[test]
    fn poll_state_detects_text_changes() {
        let mut state = ClipboardPollState::new();
        // Pre-existing text is reported once, matching the original monitor.
        assert!(state.observe(Some("first".to_owned()), vec![], None));
        assert!(!state.observe(Some("first".to_owned()), vec![], None));
        assert!(state.observe(Some("second".to_owned()), vec![], None));
    }

    #[test]
    fn poll_state_detects_non_text_copies_without_a_text_transition() {
        let mut state = ClipboardPollState::new();
        let image_a = Some((vec![1, 2, 3], 2, 2));
        let image_b = Some((vec![4, 5, 6], 2, 2));

        // Startup image is ignored; a different image while the clipboard
        // stays text-free is still reported.
        assert!(!state.observe(None, vec![], image_a));
        assert!(state.observe(None, vec![], image_b.clone()));
        // Same content again is not a change.
        assert!(!state.observe(None, vec![], image_b));

        // File paths are compared the same way.
        assert!(state.observe(None, vec!["/a".to_owned()], None));
        assert!(!state.observe(None, vec!["/a".to_owned()], None));
        assert!(state.observe(None, vec!["/b".to_owned()], None));
    }

    #[test]
    fn poll_state_reports_text_to_non_text_transitions() {
        let mut state = ClipboardPollState::new();
        assert!(state.observe(Some("text".to_owned()), vec![], None));
        // Text -> image must report even though the image itself was unseen.
        assert!(state.observe(None, vec![], Some((vec![9], 1, 1))));
        // Image -> text must report.
        assert!(state.observe(Some("again".to_owned()), vec![], None));
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
