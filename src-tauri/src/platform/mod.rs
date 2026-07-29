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
//  New submodules
// ---------------------------------------------------------------------------

pub mod monitor;
pub mod shortcuts;
pub mod ui;
pub mod infra;

// ---------------------------------------------------------------------------
//  Re-exports so items remain at crate::platform::*
// ---------------------------------------------------------------------------

pub use monitor::ClipboardMonitor;
pub use shortcuts::GlobalShortcutManager;
pub use ui::{DiskSpace, SystemTray, WindowManager, apply_window_transparency, disk_space, window_transparency_alpha};
pub(crate) use ui::show_main_window;
pub use infra::{
    AutostartAction, decide_autostart_action, sync_autostart, SingleInstanceError,
    SingleInstanceGuard,
};

// ---------------------------------------------------------------------------
//  Imports needed by remaining sections (B, C, D)
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::keyboard::ShortcutBinding;

// ---------------------------------------------------------------------------
//  Runtime platform identifier
// ---------------------------------------------------------------------------

/// Identifies the platform the application is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Windows,
    MacOS,
    LinuxX11,
    LinuxWayland,
    Unknown,
}

impl Platform {
    /// Detects the current platform at runtime.
    ///
    /// On Linux this also checks `XDG_SESSION_TYPE` to distinguish between
    /// X11 and Wayland sessions.
    pub fn detect() -> Self {
        #[cfg(target_os = "windows")]
        {
            return Platform::Windows;
        }
        #[cfg(target_os = "macos")]
        {
            return Platform::MacOS;
        }
        #[cfg(target_os = "linux")]
        {
            let session = std::env::var("XDG_SESSION_TYPE")
                .unwrap_or_default()
                .to_lowercase();
            if session == "wayland" {
                return Platform::LinuxWayland;
            }
            return Platform::LinuxX11;
        }
        #[allow(unreachable_code)]
        Platform::Unknown
    }

    /// Returns whether this platform uses the X11 windowing system.
    pub fn is_x11(&self) -> bool {
        matches!(self, Platform::LinuxX11)
    }

    /// Returns whether this platform uses the Wayland protocol.
    pub fn is_wayland(&self) -> bool {
        matches!(self, Platform::LinuxWayland)
    }

    /// Returns whether this is a macOS system.
    pub fn is_macos(&self) -> bool {
        matches!(self, Platform::MacOS)
    }

    /// Returns whether this is a Windows system.
    pub fn is_windows(&self) -> bool {
        matches!(self, Platform::Windows)
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Windows => f.write_str("Windows"),
            Platform::MacOS => f.write_str("macOS"),
            Platform::LinuxX11 => f.write_str("Linux (X11)"),
            Platform::LinuxWayland => f.write_str("Linux (Wayland)"),
            Platform::Unknown => f.write_str("Unknown"),
        }
    }
}

// ---------------------------------------------------------------------------
//  PlatformInfo (replaces RuntimeInfo with richer data)
// ---------------------------------------------------------------------------

/// Detailed information about the current platform environment.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub platform: Platform,
    pub app_version: &'static str,
    pub operating_system: &'static str,
    pub architecture: &'static str,
    pub capabilities: PlatformCapabilities,
    /// The compositor / desktop environment name (Linux only).
    pub desktop_environment: Option<String>,
    /// Whether the platform provides clipboard monitoring.
    pub clipboard_monitoring_supported: bool,
    /// Whether the platform supports global hotkey registration.
    pub global_shortcut_supported: bool,
    /// Whether the system tray is functional.
    pub system_tray_supported: bool,
    /// For Wayland: compositor-specific capability details.
    #[cfg(target_os = "linux")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wayland_capabilities: Option<linux_wayland::WaylandCapabilities>,
    /// Human-readable notes about platform-specific quirks.
    pub platform_notes: Vec<String>,
}

/// Returns a comprehensive picture of the current platform's capabilities.
pub fn get_platform_info() -> PlatformInfo {
    let platform = Platform::detect();
    let caps = current_capabilities();

    let desktop_environment = std::env::var("XDG_CURRENT_DESKTOP").ok();

    #[allow(unused_mut)]
    let mut notes = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if !macos::MacOSAccessibilityHelper::is_trusted() {
            notes.push(
                "Accessibility permission is not granted. Some features require it. \
                 Open System Settings \u{2192} Privacy & Security \u{2192} Accessibility."
                    .into(),
            );
        }
    }

    #[cfg(target_os = "linux")]
    {
        if platform.is_wayland() {
            let wayland_caps = linux_wayland::WaylandCapabilities::detect();
            if wayland_caps.requires_config {
                notes.extend(wayland_caps.notes.clone());
            }
        } else {
            notes.push(
                "X11 session detected. Clipboard monitoring should work on most \
                 desktop environments."
                    .into(),
            );
        }
    }

    PlatformInfo {
        platform,
        app_version: env!("CARGO_PKG_VERSION"),
        operating_system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        capabilities: caps,
        desktop_environment,
        clipboard_monitoring_supported: caps.clipboard_monitoring,
        global_shortcut_supported: caps.global_shortcut,
        system_tray_supported: caps.system_tray,

        #[cfg(target_os = "linux")]
        wayland_capabilities: if platform.is_wayland() {
            Some(linux_wayland::WaylandCapabilities::detect())
        } else {
            None
        },

        platform_notes: notes,
    }
}

// ---------------------------------------------------------------------------
//  ForegroundApp (platform-agnostic type)
// ---------------------------------------------------------------------------

/// Information about the foreground (focused) application.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForegroundApp {
    pub name: String,
    pub exe_path: String,
}

impl ForegroundApp {
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            exe_path: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
//  Platform dispatch clipboard functions
// ---------------------------------------------------------------------------

/// Returns the foreground application based on the current platform.
pub fn get_foreground_app() -> ForegroundApp {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::get_foreground_app();
    }
    #[cfg(target_os = "macos")]
    {
        return macos::get_foreground_app();
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => linux_wayland::get_foreground_app(),
            _ => linux_x11::get_foreground_app(),
        };
    }

    #[allow(unreachable_code)]
    ForegroundApp::empty()
}

/// Reads clipboard text.
pub fn read_clipboard_text() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::read_clipboard_text();
    }
    #[cfg(target_os = "macos")]
    {
        return macos::read_clipboard_text();
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => linux_wayland::read_clipboard_text(),
            _ => linux_x11::read_clipboard_text(),
        };
    }

    #[allow(unreachable_code)]
    None
}

/// Reads clipboard image data as PNG bytes with dimensions.
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::read_clipboard_image();
    }
    #[cfg(target_os = "macos")]
    {
        return macos::read_clipboard_image();
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => linux_wayland::read_clipboard_image(),
            _ => linux_x11::read_clipboard_image(),
        };
    }

    #[allow(unreachable_code)]
    None
}

/// Reads clipboard file paths.
pub fn read_clipboard_file_paths() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::read_clipboard_file_paths();
    }
    #[cfg(target_os = "macos")]
    {
        return macos::read_clipboard_file_paths();
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => linux_wayland::read_clipboard_file_paths(),
            _ => linux_x11::read_clipboard_file_paths(),
        };
    }

    #[allow(unreachable_code)]
    {
        Vec::new()
    }
}

/// Extracts and caches an application icon, returning the icon path.
pub fn extract_app_icon(icon_dir: &Path, app_name: &str, exe_path: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::extract_app_icon(icon_dir, app_name, exe_path);
    }
    #[cfg(target_os = "macos")]
    {
        return macos::extract_app_icon(icon_dir, app_name, exe_path);
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => linux_wayland::extract_app_icon(icon_dir, app_name, exe_path),
            _ => linux_x11::extract_app_icon(icon_dir, app_name, exe_path),
        };
    }

    #[allow(unreachable_code)]
    None
}

/// Writes text to the clipboard with a self-trigger marker.
pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::write_clipboard_text_with_self_trigger(text);
    }
    #[cfg(target_os = "macos")]
    {
        return macos::write_clipboard_text_with_self_trigger(text);
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => linux_wayland::write_clipboard_text_with_self_trigger(text),
            _ => linux_x11::write_clipboard_text_with_self_trigger(text),
        };
    }

    #[allow(unreachable_code)]
    Err("clipboard writing is not supported on this platform".to_owned())
}

// ---------------------------------------------------------------------------
//  PlatformAdapter trait
// ---------------------------------------------------------------------------

/// The `PlatformAdapter` trait abstracts clipboard monitoring, global
/// keyboard shortcut registration, and system tray management across
/// Windows, macOS, and Linux (X11 + Wayland).
///
/// # Implementing a new platform
///
/// 1. Create a new module under `src/platform/` (e.g. `windows_impl.rs`).
/// 2. Implement `PlatformAdapter` for a platform-specific struct.
/// 3. Register the adapter in `PlatformAdapter::detect()`.
///
/// # Example
///
/// ```ignore
/// use crate::platform::{PlatformAdapter, Platform, PlatformCapabilities};
///
/// let adapter = PlatformAdapter::detect();
/// println!("Running on: {:?}", adapter.platform());
/// println!("Capabilities: {:?}", adapter.capabilities());
/// ```
pub trait PlatformAdapter: Send + Sync {
    /// Returns which platform this adapter is for.
    fn platform(&self) -> Platform;

    /// Returns the capabilities available on this platform.
    fn capabilities(&self) -> PlatformCapabilities;

    /// Retrieves the current clipboard text content, if available.
    fn get_clipboard_text(&self) -> Option<String>;

    /// Returns the available clipboard format identifiers (MIME/UTI).
    fn get_clipboard_types(&self) -> Vec<String>;

    /// Registers global keyboard shortcuts for the given action.
    ///
    /// # Arguments
    ///
    /// * `action_id` - An opaque identifier passed to the callback when the
    ///   shortcut fires.
    /// * `shortcuts` - The key bindings to register.
    fn register_global_shortcuts(
        &mut self,
        action_id: &str,
        shortcuts: &[ShortcutBinding],
    ) -> Result<(), String>;

    /// Removes all previously registered global shortcuts.
    fn unregister_all_shortcuts(&mut self) -> Result<(), String>;

    /// Sets the ignored application list for clipboard monitoring.
    fn set_ignored_applications(&mut self, apps: Vec<String>);

    /// Returns the current list of ignored applications.
    fn ignored_applications(&self) -> Vec<String>;

    /// Returns whether clipboard monitoring is active.
    fn is_monitoring(&self) -> bool;

    /// Starts clipboard content monitoring.
    fn start_monitoring(&mut self) -> Result<(), String>;

    /// Stops clipboard content monitoring.
    fn stop_monitoring(&mut self) -> Result<(), String>;

    /// Returns platform-specific information as a human-readable string.
    fn platform_info(&self) -> String;
}

// ---------------------------------------------------------------------------
//  PlatformCapabilities (unchanged, kept for backward compatibility)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub clipboard_monitoring: bool,
    pub global_shortcut: bool,
    pub quick_paste: bool,
    pub system_tray: bool,
    pub requires_accessibility_permission: bool,
}

// ---------------------------------------------------------------------------
//  RuntimeInfo (kept for backward compatibility)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub app_version: &'static str,
    pub operating_system: &'static str,
    pub architecture: &'static str,
    pub capabilities: PlatformCapabilities,
}

pub trait ClipboardPlatform: Send + Sync {
    fn capabilities(&self) -> PlatformCapabilities;
}

pub fn runtime_info() -> RuntimeInfo {
    RuntimeInfo {
        app_version: env!("CARGO_PKG_VERSION"),
        operating_system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        capabilities: current_capabilities(),
    }
}

fn current_capabilities() -> PlatformCapabilities {
    #[cfg(target_os = "windows")]
    {
        return PlatformCapabilities {
            clipboard_monitoring: true,
            global_shortcut: true,
            quick_paste: true,
            system_tray: true,
            requires_accessibility_permission: false,
        };
    }

    #[cfg(target_os = "macos")]
    {
        return PlatformCapabilities {
            clipboard_monitoring: true,
            global_shortcut: true,
            quick_paste: true,
            system_tray: true,
            requires_accessibility_permission: true,
        };
    }

    #[cfg(target_os = "linux")]
    {
        return PlatformCapabilities {
            clipboard_monitoring: true,
            global_shortcut: true,
            quick_paste: false,
            system_tray: true,
            requires_accessibility_permission: false,
        };
    }

    #[allow(unreachable_code)]
    PlatformCapabilities {
        clipboard_monitoring: false,
        global_shortcut: false,
        quick_paste: false,
        system_tray: false,
        requires_accessibility_permission: false,
    }
}

// ---------------------------------------------------------------------------
//  Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    #[cfg(target_os = "windows")]
    use std::{sync::mpsc, time::Duration};

    use super::*;
    use std::str::FromStr;
    use crate::config::ConfigStore;

    // ---- Platform detection tests ----

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

    // ---- Existing tests (unchanged) ----

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
        // Out-of-range values clamp to the settings slider bounds.
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
