use std::path::Path;

use crate::platform::platform_info::ForegroundApp;
#[cfg(target_os = "linux")]
use crate::platform::platform_info::Platform;

#[cfg(target_os = "linux")]
use super::linux_wayland::LinuxWaylandPlatform;
#[cfg(target_os = "linux")]
use super::linux_x11::LinuxX11Platform;
#[cfg(target_os = "macos")]
use super::macos::MacPlatform;
#[cfg(target_os = "windows")]
use super::windows_clipboard::WindowsPlatform;

// ---------------------------------------------------------------------------
//  PlatformClipboard trait — the per-platform clipboard contract
// ---------------------------------------------------------------------------

/// The clipboard/app contract every platform must satisfy. Each platform
/// implements it in its own adapter module and `platform()` returns the
/// adapter active for the running target.
///
/// Unsupported rich-text capture keeps the documented defaults (`None`) until
/// it is wired on a platform.
pub trait PlatformClipboard {
    fn get_foreground_app(&self) -> ForegroundApp;
    fn read_clipboard_text(&self) -> Option<String>;
    fn read_clipboard_image(&self) -> Option<(Vec<u8>, u32, u32)>;
    fn read_clipboard_file_paths(&self) -> Vec<String>;
    fn write_clipboard_text_with_self_trigger(&self, text: &str) -> Result<(), String>;
    fn extract_app_icon(&self, icon_dir: &Path, app_name: &str, exe_path: &str) -> Option<String>;

    /// Reads the optional HTML fragment used for paste-by-format.
    /// Windows: `HTML Format`/CF_HTML; macOS: `public.html`; Linux: `text/html`.
    fn read_clipboard_html(&self) -> Option<String> {
        None
    }

    /// Reads the optional RTF payload used for paste-by-format.
    /// Windows: registered `Rich Text Format`; macOS: not wired; Linux: `text/rtf`.
    fn read_clipboard_rtf(&self) -> Option<String> {
        None
    }

    /// Monotonic clipboard revision used to detect the clipboard being
    /// replaced while a multi-format capture read is in progress. Returns
    /// `None` on platforms without a sequence counter (checking skipped).
    fn read_clipboard_sequence(&self) -> Option<u32> {
        None
    }
}

// ---------------------------------------------------------------------------
//  Active platform factory
// ---------------------------------------------------------------------------

/// Returns the platform adapter for the running target. The Linux variant
/// picks the X11 or Wayland backend at runtime because the display server is
/// only known after launch.
pub fn platform() -> &'static dyn PlatformClipboard {
    #[cfg(target_os = "windows")]
    {
        &WindowsPlatform
    }
    #[cfg(target_os = "macos")]
    {
        &MacPlatform
    }
    #[cfg(target_os = "linux")]
    {
        match Platform::detect() {
            Platform::LinuxWayland => &LinuxWaylandPlatform,
            _ => &LinuxX11Platform,
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        unreachable!("unsupported target OS")
    }
}
