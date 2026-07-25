use std::{
    collections::HashMap,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};
use tauri_plugin_autostart::ManagerExt;

use crate::config::ConfigStore;
use crate::keyboard::ShortcutBinding;

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
pub mod windows_hotkey;

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
                 Open System Settings → Privacy & Security → Accessibility."
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
//  ClipboardMonitor
// ---------------------------------------------------------------------------

use windows_clipboard::{ClipboardChange, WindowsClipboardMonitor};

pub struct ClipboardMonitor {
    monitor: WindowsClipboardMonitor,
    pub running: bool,
    pub last_check_at: i64,
    pub ignored_applications: Vec<String>,
    receiver: Option<std::sync::mpsc::Receiver<ClipboardChange>>,
}

impl Default for ClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardMonitor {
    pub fn new() -> Self {
        Self {
            monitor: WindowsClipboardMonitor::new(),
            running: false,
            last_check_at: 0,
            ignored_applications: Vec::new(),
            receiver: None,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        let receiver = self.monitor.start()?;
        self.receiver = Some(receiver);
        self.running = true;
        self.last_check_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.monitor.stop();
        self.running = false;
        self.receiver = None;
        Ok(())
    }

    pub fn take_receiver(&mut self) -> Option<std::sync::mpsc::Receiver<ClipboardChange>> {
        self.receiver.take()
    }

    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        self.monitor.set_ignored_apps(apps.clone());
        self.ignored_applications = apps;
    }
}

// ---------------------------------------------------------------------------
//  GlobalShortcutManager (unchanged)
// ---------------------------------------------------------------------------

pub struct GlobalShortcutManager {
    pub shortcuts: HashMap<String, Vec<ShortcutBinding>>,
    registered_ids: Vec<i32>,
}

impl Default for GlobalShortcutManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalShortcutManager {
    pub fn new() -> Self {
        Self {
            shortcuts: HashMap::new(),
            registered_ids: Vec::new(),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn register_platform_hotkeys(&mut self, hwnd: isize) -> Result<(), String> {
        use crate::platform::windows_clipboard;

        self.unregister_platform_hotkeys(hwnd)?;

        let mut next_id: i32 = 1;
        for shortcuts in self.shortcuts.values() {
            for binding in shortcuts {
                if let crate::keyboard::ShortcutBinding::Chord { modifiers, key } = binding {
                    let mut mod_flags: u32 = 0;
                    for m in modifiers {
                        match m {
                            crate::keyboard::Modifier::Alt => {
                                mod_flags |= windows_clipboard::MOD_ALT
                            }
                            crate::keyboard::Modifier::Control => {
                                mod_flags |= windows_clipboard::MOD_CONTROL
                            }
                            crate::keyboard::Modifier::Shift => {
                                mod_flags |= windows_clipboard::MOD_SHIFT
                            }
                            crate::keyboard::Modifier::Meta => {
                                mod_flags |= windows_clipboard::MOD_WIN
                            }
                        }
                    }
                    let vk = match key.to_uppercase().as_str() {
                        "V" => windows_clipboard::VK_V,
                        "SPACE" => 0x20,
                        other if other.len() == 1 => {
                            let c = other.chars().next().unwrap();
                            if c.is_ascii_alphabetic() {
                                c as u8 as u32
                            } else {
                                continue;
                            }
                        }
                        _ => continue,
                    };
                    windows_clipboard::register_global_hotkey(hwnd, next_id, mod_flags, vk)?;
                    self.registered_ids.push(next_id);
                    next_id += 1;
                }
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn register_platform_hotkeys(&mut self, _hwnd: isize) -> Result<(), String> {
        Ok(())
    }

    #[cfg(target_os = "windows")]
    pub fn unregister_platform_hotkeys(&mut self, hwnd: isize) -> Result<(), String> {
        use crate::platform::windows_clipboard;

        for id in &self.registered_ids {
            let _ = windows_clipboard::unregister_global_hotkey(hwnd, *id);
        }
        self.registered_ids.clear();
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn unregister_platform_hotkeys(&mut self, _hwnd: isize) -> Result<(), String> {
        self.registered_ids.clear();
        Ok(())
    }

    pub fn register(&mut self, action: &str, shortcuts: &[ShortcutBinding]) -> Result<(), String> {
        self.shortcuts.insert(action.to_owned(), shortcuts.to_vec());
        println!(
            "registered {} shortcut(s) for action: {}",
            shortcuts.len(),
            action
        );
        Ok(())
    }

    pub fn unregister_all(&mut self) -> Result<(), String> {
        self.shortcuts.clear();
        println!("all global shortcuts unregistered");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
//  Autostart + system tray
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartAction {
    Enable,
    Disable,
    NoChange,
}

pub fn decide_autostart_action(desired: bool, actual: bool) -> AutostartAction {
    match (desired, actual) {
        (true, false) => AutostartAction::Enable,
        (false, true) => AutostartAction::Disable,
        _ => AutostartAction::NoChange,
    }
}

pub fn sync_autostart<R: Runtime>(app: &AppHandle<R>, desired: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let actual = manager
        .is_enabled()
        .map_err(|error| format!("failed to inspect the autostart registration: {error}"))?;

    match decide_autostart_action(desired, actual) {
        AutostartAction::Enable => manager
            .enable()
            .map_err(|error| format!("failed to enable autostart: {error}")),
        AutostartAction::Disable => manager
            .disable()
            .map_err(|error| format!("failed to disable autostart: {error}")),
        AutostartAction::NoChange => Ok(()),
    }
}

pub struct SystemTray;

impl SystemTray {
    const SHOW_MENU_ID: &'static str = "tray-show";
    const QUIT_MENU_ID: &'static str = "tray-quit";

    pub fn create<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let show_item =
            MenuItem::with_id(app, Self::SHOW_MENU_ID, "显示主窗口", true, None::<&str>)
                .map_err(|error| format!("failed to create the tray show item: {error}"))?;
        let quit_item = MenuItem::with_id(app, Self::QUIT_MENU_ID, "退出", true, None::<&str>)
            .map_err(|error| format!("failed to create the tray quit item: {error}"))?;
        let menu = Menu::with_items(app, &[&show_item, &quit_item])
            .map_err(|error| format!("failed to create the tray menu: {error}"))?;

        let mut builder = TrayIconBuilder::with_id("main-tray")
            .menu(&menu)
            .tooltip("Clipboard")
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| {
                if event.id() == Self::SHOW_MENU_ID {
                    show_main_window(app);
                } else if event.id() == Self::QUIT_MENU_ID {
                    app.exit(0);
                }
            })
            .on_tray_icon_event(|tray, event| {
                if matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                ) {
                    show_main_window(tray.app_handle());
                }
            });

        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }

        builder
            .build(app)
            .map_err(|error| format!("failed to create the system tray icon: {error}"))?;

        Ok(Self)
    }
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("[tray] main window is unavailable");
        return;
    };

    if let Err(error) = window.show() {
        eprintln!("[tray] failed to show the main window: {error}");
    }
    if let Err(error) = window.unminimize() {
        eprintln!("[tray] failed to restore the main window: {error}");
    }
    if let Err(error) = window.set_focus() {
        eprintln!("[tray] failed to focus the main window: {error}");
    }
}

// ---------------------------------------------------------------------------
//  WindowManager (unchanged)
// ---------------------------------------------------------------------------

pub struct WindowManager;

impl WindowManager {
    pub fn save_position(
        config: &mut ConfigStore,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        config
            .set_window_position(x, y, width, height)
            .map_err(|e| e.to_string())
    }

    pub fn restore_position(config: &ConfigStore) -> Option<(i32, i32, u32, u32)> {
        config.window_position()
    }
}

// ---------------------------------------------------------------------------
//  SingleInstanceGuard
// ---------------------------------------------------------------------------

pub struct SingleInstanceGuard {
    lock_path: PathBuf,
    pid: u32,
}

#[derive(Debug)]
pub enum SingleInstanceError {
    AlreadyRunning(u32),
    LockFile(String),
}

impl fmt::Display for SingleInstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning(pid) => {
                write!(
                    formatter,
                    "another instance is already running (PID: {pid})"
                )
            }
            Self::LockFile(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SingleInstanceError {}

fn create_instance_lock(lock_path: &Path, pid: u32) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(lock_path)?;
    if let Err(error) = writeln!(file, "{pid}").and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(lock_path);
        return Err(error);
    }
    Ok(())
}

fn read_instance_lock_pid(lock_path: &Path) -> io::Result<Option<u32>> {
    let content = fs::read_to_string(lock_path)?;
    Ok(content.trim().parse::<u32>().ok().filter(|pid| *pid != 0))
}

impl SingleInstanceGuard {
    pub fn acquire(project_dir: &Path) -> Result<Self, SingleInstanceError> {
        let lock_path = project_dir.join("instance.lock");
        let pid = std::process::id();

        for _ in 0..3 {
            match create_instance_lock(&lock_path, pid) {
                Ok(()) => return Ok(Self { lock_path, pid }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    match read_instance_lock_pid(&lock_path) {
                        Ok(Some(owner_pid)) if is_process_running(owner_pid) => {
                            return Err(SingleInstanceError::AlreadyRunning(owner_pid));
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) => {
                            return Err(SingleInstanceError::LockFile(format!(
                                "failed to read instance lock {}: {error}",
                                lock_path.display()
                            )));
                        }
                    }

                    match fs::remove_file(&lock_path) {
                        Ok(()) => continue,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) => {
                            return Err(SingleInstanceError::LockFile(format!(
                                "failed to remove stale instance lock {}: {error}",
                                lock_path.display()
                            )));
                        }
                    }
                }
                Err(error) => {
                    return Err(SingleInstanceError::LockFile(format!(
                        "failed to create instance lock {}: {error}",
                        lock_path.display()
                    )));
                }
            }
        }

        Err(SingleInstanceError::LockFile(format!(
            "instance lock {} changed repeatedly during startup",
            lock_path.display()
        )))
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if read_instance_lock_pid(&self.lock_path).ok().flatten() == Some(self.pid) {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

#[cfg(target_os = "windows")]
fn is_process_running(pid: u32) -> bool {
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn GetExitCodeProcess(process: isize, exit_code: *mut u32) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return false;
        }
        let mut exit_code = 0u32;
        GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        exit_code == STILL_ACTIVE
    }
}

#[cfg(not(target_os = "windows"))]
fn is_process_running(pid: u32) -> bool {
    // On Unix, sending signal 0 checks if process exists
    unsafe {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        kill(pid as i32, 0) == 0
    }
}

// ---------------------------------------------------------------------------
//  Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;

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

    use std::str::FromStr;
}
