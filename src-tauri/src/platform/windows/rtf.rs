//! "Rich Text Format" reads.

use super::*;
/// The registered clipboard format name used by rich-text-capable apps
/// (Word, Outlook, browsers) to expose the RTF payload of a rich-text copy.
const CF_RTF_REGISTERED_NAME: &str = "Rich Text Format";

#[cfg(target_os = "windows")]
pub(super) fn rtf_format_id() -> Option<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }

    static FORMAT_ID: OnceLock<Option<u32>> = OnceLock::new();
    *FORMAT_ID.get_or_init(|| {
        let name = OsStr::new(CF_RTF_REGISTERED_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        (format != 0).then_some(format)
    })
}

/// Reads the RTF payload of a rich-text clipboard copy, if present. RTF is
/// ASCII-superset text (`{\rtf1 ...}`) so the raw bytes decode lossily
/// without the fragment-header parsing CF_HTML requires.
#[cfg(target_os = "windows")]
pub fn read_clipboard_rtf() -> Option<String> {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn GlobalLock(handle: isize) -> *const u8;
        fn GlobalUnlock(handle: isize) -> i32;
        fn GlobalSize(handle: isize) -> usize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    let format = rtf_format_id()?;
    unsafe {
        if IsClipboardFormatAvailable(format) == 0 {
            return None;
        }

        if !open_clipboard_with_retry() {
            return None;
        }

        let handle = GetClipboardData(format);
        if handle == 0 {
            CloseClipboard();
            return None;
        }

        let size = GlobalSize(handle);
        if !native_payload_size_allowed(size, MAX_NATIVE_TEXT_BYTES) {
            CloseClipboard();
            return None;
        }

        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }

        let data = std::slice::from_raw_parts(ptr, size).to_vec();
        GlobalUnlock(handle);
        CloseClipboard();

        let rtf = String::from_utf8_lossy(&data);
        let trimmed = rtf.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_rtf() -> Option<String> {
    None
}
