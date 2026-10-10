//! Linux Wayland platform adapter.
//!
//! Wayland is a fundamentally different display protocol from X11.  It has
//! stronger security isolation: applications cannot spy on each other's
//! events, clipboard contents, or keyboard input by default.  This means
//! clipboard managers must use specific Wayland protocol extensions to
//! function.
//!
//! # Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Wayland Platform Layer                    │
//! │                                                              │
//! │  ┌──────────────────┐  ┌──────────────────┐  ┌───────────┐ │
//! │  │ Clipboard        │  │ Global Shortcuts  │  │ Tray      │ │
//! │  │ (wlr-data-ctrl)  │  │ (Portal)          │  │ (SNI)     │ │
//! │  └──────────────────┘  └──────────────────┘  └───────────┘ │
//! │          │                      │                  │         │
//! │          ▼                      ▼                  ▼         │
//! │  ┌──────────────────────────────────────────────────────┐   │
//! │  │              Compositor Capability Detection          │   │
//! │  │  (XDG_CURRENT_DESKTOP, XDG_SESSION_TYPE, etc.)       │   │
//! │  └──────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Compositor Compatibility Matrix
//!
//! | Feature          | wlroots (Sway) | KDE Plasma  | GNOME/Mutter | Hyprland | river |
//! |------------------|----------------|--------------|--------------|----------|-------|
//! | Clipboard read   | Full           | Full         | Partial*     | Full     | Full  |
//! | Clipboard write  | Full           | Full         | No†          | Full     | Full  |
//! | Global shortcuts | Custom DAEMON  | KGlobalAccel | Portal       | Portal   | No    |
//! | System tray      | SNI            | SNI          | SNI‡         | SNI      | No    |
//! | Multiple select.  | PRIMARY+CLIP  | CLIPBOARD    | CLIPBOARD    | PRIMARY+CLIP | CLIP |
//!
//! \* GNOME restricts clipboard access to the focused application when using
//!   the default Mutter compositor.  Use an extension or portal.
//!
//! † GNOME does not expose `wlr-data-control` by default.  You may need to
//!   use the GNOME Shell extension `Clipboard Indicator` or similar.
//!
//! ‡ GNOME hides the system tray by default.  Install the `AppIndicator` or
//!   `Tray Icons Reloaded` GNOME Shell extension.
//!
//! # Detection
//!
//! ```text
//! if XDG_SESSION_TYPE == "wayland":
//!     if XDG_CURRENT_DESKTOP contains "sway" or "Hyprland":
//!         → wlroots-based, full clipboard + SNI tray support
//!     if XDG_CURRENT_DESKTOP contains "KDE":
//!         → KDE Plasma, KGlobalAccel for shortcuts, SNI tray
//!     if XDG_CURRENT_DESKTOP contains "GNOME":
//!         → Reduced clipboard access, Portal shortcuts, SNI (with extension)
//!     else:
//!         → Best-effort: try wlr-data-control, fall back to Portal clipboard
//! ```
//!
//! # Module Layout
//!
//! | File           | Contents                                                             |
//! | -------------- | -------------------------------------------------------------------- |
//! | `caps.rs`      | `WaylandCapabilities` — reported through `platform_info`             |
//! | `clipboard.rs` | Live `wl-paste`/`wl-copy` clipboard, foreground app and icon lookup |
//! | `monitor.rs`   | Data-control event-driven clipboard monitor (Linux)                  |
//! | `dispatch.rs`  | Bounded single-shot round-trips against the Wayland socket           |
//! | `hotkeys.rs`   | Live GlobalShortcuts portal sessions                                 |
//! | `paste.rs`     | `wtype`-based quick paste for Sway/Hyprland                          |
//! | `intended.rs`  | Recorded design intent for tray/portal shortcuts — **not wired yet** |
//!
//! `intended.rs` is documentation kept compilable and exercised only by unit
//! tests; treat its types as design intent, never as shipped behavior.

#![allow(dead_code)]
#[cfg(target_os = "linux")]
pub mod dispatch;
#[cfg(target_os = "linux")]
pub mod hotkeys;
#[cfg(any(test, target_os = "linux"))]
pub mod paste;

pub struct LinuxWaylandPlatform;

#[cfg(target_os = "linux")]
impl crate::platform::PlatformClipboard for LinuxWaylandPlatform {
    fn get_foreground_app(&self) -> crate::platform::ForegroundApp {
        get_foreground_app()
    }

    fn read_clipboard_text(&self) -> Option<String> {
        read_clipboard_text()
    }

    fn read_clipboard_image(&self) -> Option<(crate::platform::ClipboardImageData, u32, u32)> {
        read_clipboard_image()
            .map(|(data, w, h)| (crate::platform::ClipboardImageData::Rgba(data), w, h))
    }

    fn read_clipboard_file_paths(&self) -> Vec<String> {
        read_clipboard_file_paths()
    }

    fn write_clipboard_text_with_self_trigger(&self, text: &str) -> Result<(), String> {
        write_clipboard_text_with_self_trigger(text)
    }

    fn extract_app_icon(
        &self,
        icon_dir: &std::path::Path,
        app_name: &str,
        exe_path: &str,
    ) -> Option<String> {
        extract_app_icon(icon_dir, app_name, exe_path)
    }

    fn read_clipboard_html(&self) -> Option<String> {
        read_clipboard_html()
    }

    fn read_clipboard_rtf(&self) -> Option<String> {
        read_clipboard_rtf()
    }
}

// ---------------------------------------------------------------------------
// Submodules
// ---------------------------------------------------------------------------

mod caps;
mod clipboard;
mod intended;
mod monitor;
#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Re-exports so items remain at `crate::platform::linux::wayland::*`
// ---------------------------------------------------------------------------

pub use caps::*;
pub use clipboard::*;
pub use intended::*;
#[cfg(target_os = "linux")]
pub(crate) use monitor::try_spawn_data_control_monitor;
