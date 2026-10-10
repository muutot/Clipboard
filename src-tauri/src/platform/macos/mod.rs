//! macOS platform adapter providing clipboard monitoring, global hotkeys,
//! accessibility permission management, and system tray integration.
//!
//! # Overview
//!
//! This module wraps native macOS APIs (AppKit, Carbon, CoreGraphics) behind
//! safe Rust abstractions. On non-macOS targets only the type definitions and
//! documentation are compiled — the implementation bodies are gated behind
//! `#[cfg(target_os = "macos")]`.
//!
//! # Module Layout
//!
//! | File                | Contents                                                                 |
//! | ------------------- | ------------------------------------------------------------------------ |
//! | `objc.rs`           | Objective-C message-send helpers (also used by quick_paste and ui)       |
//! | `clipboard.rs`      | Live pasteboard reads/writes, foreground app and icon lookup             |
//! | `accessibility.rs`  | Accessibility permission state reported to `platform_info`              |
//! | `error.rs`          | `MacOSError` / `MacOSResult`                                           |
//! | `ffi.rs`            | Native API declarations — every call site is still a commented outline   |
//! | `intended.rs`       | Recorded design intent (hotkey hook, monitor, tray) — **not wired yet**  |
//!
//! The live surface is the `MacPlatform` impl below plus `clipboard.rs`;
//! `intended.rs` is documentation kept compilable and exercised only by unit
//! tests, so never report its types as shipped behavior.

#![allow(dead_code)]

pub struct MacPlatform;

#[cfg(target_os = "macos")]
impl crate::platform::PlatformClipboard for MacPlatform {
    fn get_foreground_app(&self) -> crate::platform::ForegroundApp {
        get_foreground_app()
    }

    fn read_clipboard_sequence(&self) -> Option<u32> {
        let pool = unsafe { objc::objc_autoreleasePoolPush() };
        let pb = objc::get_nspasteboard();
        let count = if pb.is_null() {
            None
        } else {
            Some(objc::pasteboard_change_count(pb) as u32)
        };
        unsafe { objc::objc_autoreleasePoolPop(pool) };
        count
    }

    fn read_clipboard_text(&self) -> Option<String> {
        read_clipboard_text()
    }

    fn read_clipboard_html(&self) -> Option<String> {
        read_clipboard_html()
    }

    fn read_clipboard_rtf(&self) -> Option<String> {
        read_clipboard_rtf()
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
}

// ---------------------------------------------------------------------------
// Submodules
// ---------------------------------------------------------------------------

mod accessibility;
mod clipboard;
mod error;
mod ffi;
mod intended;
#[cfg(target_os = "macos")]
pub(crate) mod objc;
#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Re-exports so items remain at `crate::platform::macos::*`
// ---------------------------------------------------------------------------

pub use accessibility::*;
pub use clipboard::*;
pub use error::*;
pub use intended::*;
