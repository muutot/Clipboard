//! CF_HTML ("HTML Format") decoding: header offset parsing and reads.

use super::*;
/// The registered clipboard format name used by browsers and office apps to
/// expose the HTML fragment of a rich-text copy.
const CF_HTML_REGISTERED_NAME: &str = "HTML Format";

#[cfg(target_os = "windows")]
pub(super) fn html_format_id() -> Option<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }

    static FORMAT_ID: OnceLock<Option<u32>> = OnceLock::new();
    *FORMAT_ID.get_or_init(|| {
        let name = OsStr::new(CF_HTML_REGISTERED_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        (format != 0).then_some(format)
    })
}

/// Extracts the HTML fragment from a CF_HTML payload.
///
/// CF_HTML is a text header followed by the markup; `StartFragment:` /
/// `EndFragment:` give byte offsets into the whole buffer. Headers use the
/// `StartFragment:0000000141` shape, so the label search includes the colon.
/// Falls back to `StartHTML:` / `EndHTML:` when the fragment offsets are
/// missing, and rejects empty results.
pub(super) fn parse_cf_html(data: &[u8]) -> Option<String> {
    fn field_value(data: &[u8], label: &str) -> Option<usize> {
        let needle = label.as_bytes();
        let pos = data
            .windows(needle.len())
            .position(|window| window == needle)?;
        let rest = &data[pos + needle.len()..];
        let line_end = rest
            .iter()
            .position(|&byte| byte == b'\r' || byte == b'\n')
            .unwrap_or(rest.len());
        let value = rest[..line_end]
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .collect::<Vec<_>>();
        std::str::from_utf8(&value).ok()?.trim().parse().ok()
    }

    let (start, end) = match (
        field_value(data, "StartFragment:"),
        field_value(data, "EndFragment:"),
    ) {
        (Some(start), Some(end)) if end > start && end <= data.len() => (start, end),
        _ => {
            let start = field_value(data, "StartHTML:")?;
            let end = field_value(data, "EndHTML:")?;
            if end <= start || end > data.len() {
                return None;
            }
            (start, end)
        }
    };

    let fragment = String::from_utf8_lossy(&data[start..end]);
    let trimmed = fragment.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Reads the HTML fragment of a rich-text clipboard copy, if present.
#[cfg(target_os = "windows")]
pub fn read_clipboard_html() -> Option<String> {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn GlobalLock(handle: isize) -> *const u8;
        fn GlobalUnlock(handle: isize) -> i32;
        fn GlobalSize(handle: isize) -> usize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    let format = html_format_id()?;
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

        parse_cf_html(&data)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_html() -> Option<String> {
    None
}
