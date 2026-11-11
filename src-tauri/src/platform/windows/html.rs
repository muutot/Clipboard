//! CF_HTML ("HTML Format") encoding and decoding: header offset building,
//! header offset parsing and reads.

use super::*;
/// The registered clipboard format name used by browsers and office apps to
/// expose the HTML fragment of a rich-text copy.
const CF_HTML_REGISTERED_NAME: &str = "HTML Format";

/// The fragment-delimiter comments every CF_HTML writer emits around the
/// markup; readers locate the fragment through the offsets but keep the
/// markers so consumers that scan for them still work.
const CF_HTML_FRAGMENT_START: &str = "<!--StartFragment-->";
const CF_HTML_FRAGMENT_END: &str = "<!--EndFragment-->";
/// Fixed width of every offset field in the header (the shape browsers and
/// Office write). With a fixed width the header length never depends on the
/// values, so the offsets can be computed in a single pass.
const CF_HTML_OFFSET_DIGITS: usize = 10;

#[cfg(target_os = "windows")]
pub(super) fn html_format_id() -> Option<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    unsafe extern "system" {
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

/// Builds a CF_HTML payload (header + markup) whose fragment offsets bracket
/// `fragment` inside a standard document shell. Every offset field is
/// zero-padded to [`CF_HTML_OFFSET_DIGITS`] so the header keeps a constant
/// length and the byte offsets need no re-padding loop. Offsets above the
/// representable width are rejected rather than silently truncated — an
/// offset that wraps would hand the paste target the wrong markup region.
pub(super) fn build_cf_html_payload(fragment: &str) -> Result<Vec<u8>, String> {
    let max_offset = 10usize
        .checked_pow(CF_HTML_OFFSET_DIGITS as u32)
        .ok_or_else(|| "CF_HTML header offset width overflow".to_owned())?;
    // The markup is UTF-8; offsets count bytes, not characters.
    let body = format!(
        "<html><body>{CF_HTML_FRAGMENT_START}{fragment}{CF_HTML_FRAGMENT_END}</body></html>"
    );
    let header = format!(
        "Version:0100\r\nStartHTML:{z}\r\nEndHTML:{z}\r\nStartFragment:{z}\r\nEndFragment:{z}\r\n",
        z = "0".repeat(CF_HTML_OFFSET_DIGITS)
    );
    let header_len = header.len();
    let fragment_start =
        header_len + body.find(CF_HTML_FRAGMENT_START).unwrap() + CF_HTML_FRAGMENT_START.len();
    let fragment_end = header_len + body.find(CF_HTML_FRAGMENT_END).unwrap();
    let end_html = header_len + body.len();
    for offset in [header_len, end_html, fragment_start, fragment_end] {
        if offset >= max_offset {
            return Err("clipboard HTML payload exceeds the CF_HTML offset width".to_owned());
        }
    }
    let mut out = Vec::with_capacity(end_html);
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(body.as_bytes());
    let mut apply = |label: &str, value: usize| {
        let needle = label.as_bytes();
        // Search the header only: copied markup may itself contain a label.
        let header = &out[..header_len];
        let pos = header
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("header label");
        let digits = format!("{value:0width$}", width = CF_HTML_OFFSET_DIGITS);
        out[pos + needle.len()..pos + needle.len() + CF_HTML_OFFSET_DIGITS]
            .copy_from_slice(digits.as_bytes());
    };
    apply("StartHTML:", header_len);
    apply("EndHTML:", end_html);
    apply("StartFragment:", fragment_start);
    apply("EndFragment:", fragment_end);
    Ok(out)
}

fn header_field_value(data: &[u8], label: &str) -> Option<usize> {
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

/// Extracts the HTML fragment from a CF_HTML payload.
///
/// CF_HTML is a text header followed by the markup; `StartFragment:` /
/// `EndFragment:` give byte offsets into the whole buffer. Headers use the
/// `StartFragment:0000000141` shape, so the label search includes the colon.
/// Falls back to `StartHTML:` / `EndHTML:` when the fragment offsets are
/// missing, and rejects empty results.
pub(super) fn parse_cf_html(data: &[u8]) -> Option<String> {
    let (start, end) = match (
        header_field_value(data, "StartFragment:"),
        header_field_value(data, "EndFragment:"),
    ) {
        (Some(start), Some(end)) if end > start && end <= data.len() => (start, end),
        _ => {
            let start = header_field_value(data, "StartHTML:")?;
            let end = header_field_value(data, "EndHTML:")?;
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

/// Extracts the `SourceURL:` header line from a CF_HTML payload. Chromium
/// browsers record the originating page URL there; other browsers may omit it.
///
/// The scan is bounded to the header region (everything before the fragment
/// offset), so copied markup that itself mentions "SourceURL:" cannot shadow
/// the real header line.
pub(super) fn parse_cf_html_source_url(data: &[u8]) -> Option<String> {
    let bound = header_field_value(data, "StartFragment:")
        .or_else(|| header_field_value(data, "StartHTML:"))
        .unwrap_or(data.len());
    let header = &data[..bound.min(data.len())];

    let needle = b"SourceURL:";
    let pos = header
        .windows(needle.len())
        .position(|window| window == needle)?;
    let rest = &header[pos + needle.len()..];
    let line_end = rest
        .iter()
        .position(|&byte| byte == b'\r' || byte == b'\n')
        .unwrap_or(rest.len());
    let url = std::str::from_utf8(&rest[..line_end]).ok()?.trim();
    (!url.is_empty()).then(|| url.to_owned())
}

/// Reads the raw CF_HTML payload bytes, if the registered "HTML Format"
/// clipboard data is present and within the native size ceiling.
#[cfg(target_os = "windows")]
fn read_cf_html_raw() -> Option<Vec<u8>> {
    unsafe extern "system" {
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

        Some(data)
    }
}

/// Reads the HTML fragment of a rich-text clipboard copy, if present.
#[cfg(target_os = "windows")]
pub fn read_clipboard_html() -> Option<String> {
    parse_cf_html(&read_cf_html_raw()?)
}

/// Reads the originating page URL recorded by Chromium browsers in the
/// CF_HTML header of a rich-text clipboard copy, if present.
#[cfg(target_os = "windows")]
pub fn read_clipboard_source_url() -> Option<String> {
    parse_cf_html_source_url(&read_cf_html_raw()?)
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_html() -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_source_url() -> Option<String> {
    None
}
