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
            if session == "x11" {
                return Platform::LinuxX11;
            }
            // systemd-activated sessions and some display managers never set
            // XDG_SESSION_TYPE. The Wayland socket is the authoritative probe
            // there: assuming X11 leaves the capture loop with a native X11
            // reader that finds nothing on a real Wayland session (no
            // XWayland clipboard bridge in this code path), while `wl-paste`
            // would have worked.
            if std::env::var_os("WAYLAND_DISPLAY")
                .map(|value| !value.is_empty())
                .unwrap_or(false)
            {
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

/// Capability flags reflect the backend compiled for the running platform.
/// Wayland becomes available only after its portal session binds shortcuts.
pub fn capabilities_for(platform: Platform) -> PlatformCapabilities {
    let real_shortcut_backend = platform == Platform::detect()
        && match platform {
            Platform::Windows | Platform::MacOS | Platform::LinuxX11 => true,
            Platform::LinuxWayland => {
                #[cfg(target_os = "linux")]
                {
                    super::linux::wayland::hotkeys::READY.load(std::sync::atomic::Ordering::SeqCst)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    false
                }
            }
            Platform::Unknown => false,
        };

    let (clipboard_monitoring, system_tray, requires_accessibility_permission) = match platform {
        Platform::Windows => (true, true, false),
        Platform::MacOS => (true, true, true),
        Platform::LinuxX11 | Platform::LinuxWayland => (true, true, false),
        Platform::Unknown => (false, false, false),
    };

    PlatformCapabilities {
        clipboard_monitoring,
        global_shortcut: real_shortcut_backend,
        quick_paste: platform == Platform::detect()
            && match platform {
                Platform::Windows | Platform::MacOS | Platform::LinuxX11 => true,
                Platform::LinuxWayland => {
                    #[cfg(target_os = "linux")]
                    {
                        super::linux::wayland::paste::available()
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        false
                    }
                }
                Platform::Unknown => false,
            },
        system_tray,
        requires_accessibility_permission,
    }
}

pub fn current_capabilities() -> PlatformCapabilities {
    capabilities_for(Platform::detect())
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
    pub wayland_capabilities: Option<super::linux::wayland::WaylandCapabilities>,
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
            let wayland_caps = super::linux::wayland::WaylandCapabilities::detect();
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
            Some(super::linux::wayland::WaylandCapabilities::detect())
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

#[cfg(test)]
mod tests {
    use super::{capabilities_for, Platform};

    /// Only the running platform may advertise its wired backend. Portal
    /// availability is separately determined after session authorization.
    #[test]
    fn no_platform_claims_an_unimplemented_shortcut_backend() {
        let running = Platform::detect();
        let real_backend = !matches!(running, Platform::LinuxWayland | Platform::Unknown);

        for platform in [
            Platform::Windows,
            Platform::MacOS,
            Platform::LinuxX11,
            Platform::LinuxWayland,
            Platform::Unknown,
        ] {
            let capabilities = capabilities_for(platform);
            let expected = real_backend && platform == running;

            assert_eq!(
                capabilities.global_shortcut, expected,
                "{platform}: global_shortcut must track the running shortcut backend"
            );
            assert_eq!(
                capabilities.quick_paste,
                platform == running
                    && match running {
                        Platform::Windows | Platform::MacOS | Platform::LinuxX11 => true,
                        Platform::LinuxWayland => {
                            #[cfg(target_os = "linux")]
                            {
                                crate::platform::linux::wayland::paste::available()
                            }
                            #[cfg(not(target_os = "linux"))]
                            {
                                false
                            }
                        }
                        Platform::Unknown => false,
                    },
                "{platform}: quick_paste must track the compiled restore_window_and_paste"
            );
        }
    }

    /// macOS needs Accessibility permission for the features that reach the
    /// system APIs; no other supported target gates on it. Unknown must not
    /// claim any capability at all.
    #[test]
    fn accessibility_permission_is_macos_only_and_unknown_claims_nothing() {
        assert!(capabilities_for(Platform::MacOS).requires_accessibility_permission);
        assert!(!capabilities_for(Platform::Windows).requires_accessibility_permission);
        assert!(!capabilities_for(Platform::LinuxX11).requires_accessibility_permission);
        assert!(!capabilities_for(Platform::LinuxWayland).requires_accessibility_permission);

        let unknown = capabilities_for(Platform::Unknown);
        assert!(!unknown.clipboard_monitoring);
        assert!(!unknown.system_tray);
        assert!(!unknown.global_shortcut);
        assert!(!unknown.quick_paste);
        assert!(!unknown.requires_accessibility_permission);
    }
}
