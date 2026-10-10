//! Linux X11 platform adapter providing clipboard monitoring via Xlib/XCB,
//! global hotkey registration through XGrabKey, and system tray support.
//!
//! # X11 Selection Model
//!
//! X11 has three standard selections:
//!
//! | Selection  | Atom          | Purpose                              |
//! |------------|---------------|--------------------------------------|
//! | PRIMARY    | `XA_PRIMARY`  | Middle-click / selection paste       |
//! | CLIPBOARD  | `XA_CLIPBOARD`| Explicit copy-paste (Ctrl+C / Ctrl+V)|
//! | SECONDARY  | `XA_SECONDARY`| Rarely used; mostly historical       |
//!
//! This adapter monitors both PRIMARY and CLIPBOARD using `SelectionNotify`
//! events.  On non-Linux targets only the type definitions and documentation
//! are compiled.

#![allow(dead_code)]
#[cfg(any(test, target_os = "linux"))]
pub mod effect;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use crate::keyboard::ShortcutBinding;

pub struct LinuxX11Platform;

#[cfg(target_os = "linux")]
impl crate::platform::PlatformClipboard for LinuxX11Platform {
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

mod clipboard;
mod error;
mod hotkey;
mod keys;
mod monitor;
#[cfg(test)]
mod tests;
mod tray;
#[cfg(target_os = "linux")]
mod x11_ffi;

// ---------------------------------------------------------------------------
// Re-exports so items remain at `crate::platform::linux::x11::*`
// ---------------------------------------------------------------------------

pub use clipboard::*;
pub use error::*;
pub use hotkey::*;
pub use keys::*;
#[cfg(target_os = "linux")]
pub(crate) use monitor::try_spawn_xfixes_monitor;
pub use tray::*;
