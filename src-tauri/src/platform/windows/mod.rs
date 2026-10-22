//! Windows clipboard access, split by concern: monitoring, open/format
//! plumbing, per-format read/write paths, icon extraction and hotkeys.
//! Shared constants and the `PlatformClipboard` adapter stay here; every
//! submodule reaches them with `use super::*`. The split is a pure move:
//! no behavior, signatures or test expectations changed.

#![allow(non_snake_case, dead_code)]
// On non-Windows targets nearly every item in this module is compiled out,
// which leaves the submodules' `use super::*` imports unused. Keep the lint
// enabled on Windows, where those imports are real.
#![cfg_attr(not(target_os = "windows"), allow(unused_imports))]

pub struct WindowsPlatform;

#[cfg(target_os = "windows")]
impl crate::platform::PlatformClipboard for WindowsPlatform {
    fn get_foreground_app(&self) -> crate::platform::ForegroundApp {
        get_foreground_app()
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

    fn read_clipboard_sequence(&self) -> Option<u32> {
        read_clipboard_sequence()
    }

    fn read_clipboard_image(&self) -> Option<(crate::platform::ClipboardImageData, u32, u32)> {
        read_clipboard_image()
            .map(|(data, w, h)| (crate::platform::ClipboardImageData::Png(data), w, h))
    }

    fn read_clipboard_file_paths(&self) -> Vec<String> {
        read_clipboard_file_paths()
    }

    fn write_clipboard_text_with_self_trigger(&self, text: &str) -> Result<(), String> {
        write_clipboard_text_with_self_trigger(text)
    }

    fn write_clipboard_files_with_self_trigger(&self, paths: &[String]) -> Result<(), String> {
        write_clipboard_files_with_self_trigger(paths)
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

pub const CF_UNICODETEXT: u32 = 13;
pub const CF_DIB: u32 = 8;
pub const CF_DIBV5: u32 = 17;
pub const CF_HDROP: u32 = 15;
pub const CF_BITMAP: u32 = 2;

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

const APP_ICON_SIZE: u32 = 32;
pub const SELF_TRIGGER_FORMAT_NAME: &str = "ClipboardDesktop.SelfTrigger.v1";

// Native allocation ceilings precede configurable persistence limits. Text
// matches the bounded Unix helpers; decoded pixels match the shared decoder.
const MAX_NATIVE_TEXT_BYTES: usize = 64 * 1024 * 1024;
const MAX_NATIVE_IMAGE_BYTES: usize = 512 * 1024 * 1024;
const MAX_SELF_TRIGGER_BYTES: usize = 4096;

fn native_payload_size_allowed(size: usize, limit: usize) -> bool {
    size > 0 && size <= limit
}

/// Encodes all hashes that the capture pipeline may derive from a text write.
/// Keeping the marker as a small private clipboard format lets a separate CLI
/// process tell the running monitor that the next change originated here.
pub fn self_trigger_marker_for_text(text: &str) -> Vec<u8> {
    crate::content::hash::compute_clipboard_write_hashes(text)
        .join("\n")
        .into_bytes()
}

pub fn clipboard_change_is_self_write(marker: &[u8], observed_text: &str) -> bool {
    let Ok(marker_text) = std::str::from_utf8(marker) else {
        return false;
    };
    let marker_hashes = marker_text.trim_matches('\0').split('\n');
    let expected_hashes = crate::content::hash::compute_clipboard_write_hashes(observed_text);
    expected_hashes
        .iter()
        .any(|expected| marker_hashes.clone().any(|marked| marked == expected))
}

pub(crate) fn normalize_app_icon(image: image::RgbaImage) -> image::RgbaImage {
    image::imageops::resize(
        &image,
        APP_ICON_SIZE,
        APP_ICON_SIZE,
        image::imageops::FilterType::Lanczos3,
    )
}

fn is_normalized_app_icon(path: &std::path::Path) -> bool {
    image::image_dimensions(path)
        .map(|(width, height)| width == APP_ICON_SIZE && height == APP_ICON_SIZE)
        .unwrap_or(false)
}

mod access;
mod bitmap;
pub mod dpapi;
mod files;
mod foreground;
mod html;
mod monitor;
mod read_text;
mod register;
mod rtf;
mod write;

#[cfg(target_os = "windows")]
pub mod hotkey;
#[cfg(not(target_os = "windows"))]
#[path = "hotkey_stub.rs"]
pub mod hotkey;

#[cfg(all(test, target_os = "windows"))]
#[path = "hotkey_stub.rs"]
pub(crate) mod portable_hotkey_compile_test;

#[cfg(test)]
mod tests;

pub use bitmap::read_clipboard_image;
pub use files::read_clipboard_file_paths;
pub use foreground::extract_app_icon;
#[cfg(target_os = "windows")]
pub use foreground::get_foreground_app;
pub use html::read_clipboard_html;
pub use monitor::{ClipboardChange, ClipboardMonitor, WindowsClipboardMonitor};
pub use read_text::read_clipboard_text;
pub use register::{register_global_hotkey, unregister_global_hotkey};
pub use rtf::read_clipboard_rtf;
pub use write::{write_clipboard_files_with_self_trigger, write_clipboard_text_with_self_trigger};

use access::*;
use bitmap::*;
use monitor::*;
