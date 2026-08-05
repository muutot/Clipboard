use serde::{Deserialize, Serialize};

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

    pub fn is_x11(&self) -> bool {
        matches!(self, Platform::LinuxX11)
    }

    pub fn is_wayland(&self) -> bool {
        matches!(self, Platform::LinuxWayland)
    }

    pub fn is_macos(&self) -> bool {
        matches!(self, Platform::MacOS)
    }

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
    pub executable_path: String,
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
        executable_path: std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        capabilities: current_capabilities(),
    }
}

pub fn current_capabilities() -> PlatformCapabilities {
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub platform: Platform,
    pub app_version: &'static str,
    pub operating_system: &'static str,
    pub architecture: &'static str,
    pub capabilities: PlatformCapabilities,
    pub desktop_environment: Option<String>,
    pub clipboard_monitoring_supported: bool,
    pub global_shortcut_supported: bool,
    pub system_tray_supported: bool,
    #[cfg(target_os = "linux")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wayland_capabilities: Option<super::linux_wayland::WaylandCapabilities>,
    pub platform_notes: Vec<String>,
}

pub fn get_platform_info() -> PlatformInfo {
    let platform = Platform::detect();
    let caps = current_capabilities();

    let desktop_environment = std::env::var("XDG_CURRENT_DESKTOP").ok();

    #[allow(unused_mut)]
    let mut notes = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if !super::macos::MacOSAccessibilityHelper::is_trusted() {
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
            let wayland_caps = super::linux_wayland::WaylandCapabilities::detect();
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
            Some(super::linux_wayland::WaylandCapabilities::detect())
        } else {
            None
        },

        platform_notes: notes,
    }
}

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
