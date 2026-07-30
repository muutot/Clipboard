use std::path::Path;

use crate::keyboard::ShortcutBinding;

use super::{
    platform_info::{ForegroundApp, Platform, PlatformCapabilities},
};

// ---------------------------------------------------------------------------
//  Platform dispatch clipboard functions
// ---------------------------------------------------------------------------

pub fn get_foreground_app() -> ForegroundApp {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::get_foreground_app();
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::get_foreground_app();
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => super::linux_wayland::get_foreground_app(),
            _ => super::linux_x11::get_foreground_app(),
        };
    }

    #[allow(unreachable_code)]
    ForegroundApp::empty()
}

pub fn read_clipboard_text() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::read_clipboard_text();
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::read_clipboard_text();
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => super::linux_wayland::read_clipboard_text(),
            _ => super::linux_x11::read_clipboard_text(),
        };
    }

    #[allow(unreachable_code)]
    None
}

pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::read_clipboard_image();
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::read_clipboard_image();
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => super::linux_wayland::read_clipboard_image(),
            _ => super::linux_x11::read_clipboard_image(),
        };
    }

    #[allow(unreachable_code)]
    None
}

pub fn read_clipboard_file_paths() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::read_clipboard_file_paths();
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::read_clipboard_file_paths();
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => super::linux_wayland::read_clipboard_file_paths(),
            _ => super::linux_x11::read_clipboard_file_paths(),
        };
    }

    #[allow(unreachable_code)]
    {
        Vec::new()
    }
}

pub fn extract_app_icon(icon_dir: &Path, app_name: &str, exe_path: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::extract_app_icon(icon_dir, app_name, exe_path);
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::extract_app_icon(icon_dir, app_name, exe_path);
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => {
                super::linux_wayland::extract_app_icon(icon_dir, app_name, exe_path)
            }
            _ => super::linux_x11::extract_app_icon(icon_dir, app_name, exe_path),
        };
    }

    #[allow(unreachable_code)]
    None
}

pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        return windows_clipboard::write_clipboard_text_with_self_trigger(text);
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::write_clipboard_text_with_self_trigger(text);
    }
    #[cfg(target_os = "linux")]
    {
        return match Platform::detect() {
            Platform::LinuxWayland => {
                super::linux_wayland::write_clipboard_text_with_self_trigger(text)
            }
            _ => super::linux_x11::write_clipboard_text_with_self_trigger(text),
        };
    }

    #[allow(unreachable_code)]
    Err("clipboard writing is not supported on this platform".to_owned())
}

// ---------------------------------------------------------------------------
//  PlatformAdapter trait
// ---------------------------------------------------------------------------

pub trait PlatformAdapter: Send + Sync {
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> PlatformCapabilities;
    fn get_clipboard_text(&self) -> Option<String>;
    fn get_clipboard_types(&self) -> Vec<String>;
    fn register_global_shortcuts(
        &mut self,
        action_id: &str,
        shortcuts: &[ShortcutBinding],
    ) -> Result<(), String>;
    fn unregister_all_shortcuts(&mut self) -> Result<(), String>;
    fn set_ignored_applications(&mut self, apps: Vec<String>);
    fn ignored_applications(&self) -> Vec<String>;
    fn is_monitoring(&self) -> bool;
    fn start_monitoring(&mut self) -> Result<(), String>;
    fn stop_monitoring(&mut self) -> Result<(), String>;
    fn platform_info(&self) -> String;
}
