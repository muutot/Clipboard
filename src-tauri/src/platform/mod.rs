use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::config::ConfigStore;
use crate::keyboard::ShortcutBinding;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub clipboard_monitoring: bool,
    pub global_shortcut: bool,
    pub quick_paste: bool,
    pub system_tray: bool,
    pub requires_accessibility_permission: bool,
}

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

pub struct ClipboardMonitor {
    pub running: bool,
    pub last_check_at: i64,
    pub ignored_applications: Vec<String>,
}

impl ClipboardMonitor {
    pub fn new() -> Self {
        Self {
            running: false,
            last_check_at: 0,
            ignored_applications: Vec::new(),
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        println!("Clipboard monitoring started");
        self.running = true;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.running = false;
        Ok(())
    }

    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        self.ignored_applications = apps;
    }
}

pub struct GlobalShortcutManager {
    pub shortcuts: HashMap<String, Vec<ShortcutBinding>>,
}

impl GlobalShortcutManager {
    pub fn new() -> Self {
        Self {
            shortcuts: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        action: &str,
        shortcuts: &[ShortcutBinding],
    ) -> Result<(), String> {
        self.shortcuts
            .insert(action.to_owned(), shortcuts.to_vec());
        println!("registered {} shortcut(s) for action: {}", shortcuts.len(), action);
        Ok(())
    }

    pub fn unregister_all(&mut self) -> Result<(), String> {
        self.shortcuts.clear();
        println!("all global shortcuts unregistered");
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum TrayMenuItem {
    Item { label: String, action: String },
    Separator,
}

pub struct SystemTray;

impl SystemTray {
    pub fn create() -> Result<Self, String> {
        println!("system tray icon created (placeholder)");
        Ok(Self)
    }

    pub fn set_menu(&mut self, _items: Vec<TrayMenuItem>) -> Result<(), String> {
        println!("system tray menu updated (placeholder)");
        Ok(())
    }
}

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

pub struct SingleInstanceGuard {
    lock_path: PathBuf,
}

impl SingleInstanceGuard {
    pub fn acquire(project_dir: &Path) -> Result<Self, String> {
        let lock_path = project_dir.join("instance.lock");

        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&lock_path)
        {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id())
                    .map_err(|e| format!("failed to write lock file: {e}"))?;
                Ok(Self { lock_path })
            }
            Err(_) => {
                let pid = fs::read_to_string(&lock_path)
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                Err(format!(
                    "another instance is already running (PID: {})",
                    pid
                ))
            }
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;

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
        assert!(second.is_err());

        drop(guard);
        assert!(!lock_path.exists());

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

