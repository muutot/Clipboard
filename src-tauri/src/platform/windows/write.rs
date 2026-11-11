//! Write paths: CF_UNICODETEXT and CF_HDROP payloads, each
//! accompanied by the best-effort self-trigger marker format.

use super::*;
/// Writes CF_UNICODETEXT plus a private hash marker. Marker failures are
/// intentionally best-effort: the text write must retain its original
/// behavior even when a clipboard implementation rejects custom formats.
#[cfg(target_os = "windows")]
pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;

    let wide = OsStr::new(text)
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let wide_byte_len = wide
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| "clipboard text is too large".to_owned())?;
    let marker = self_trigger_marker_for_text(text);

    // Allocate and fill every buffer before touching the clipboard: once
    // EmptyClipboard succeeds the user's previous contents are gone, so
    // allocation failures must not be reachable after that point.
    let text_memory = allocate_global_bytes(unsafe {
        std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide_byte_len)
    })?;
    // `match` rather than `Option::zip`: the marker buffer must stay unallocated
    // when no self-trigger format is registered, and every cleanup path below
    // only frees the buffer when this is `Some`. `zip` would evaluate the
    // allocation eagerly and leak it in exactly that case.
    let marker_memory = match self_trigger_format_id() {
        Some(format) => allocate_global_bytes(&marker)
            .ok()
            .map(|memory| (format, memory)),
        None => None,
    };

    unsafe {
        if !open_clipboard_with_retry() {
            GlobalFree(text_memory);
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to open the system clipboard".to_owned());
        }
        let _clipboard_guard = ClipboardGuard;

        if EmptyClipboard() == 0 {
            GlobalFree(text_memory);
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to clear the system clipboard".to_owned());
        }

        if SetClipboardData(CF_UNICODETEXT, text_memory) == 0 {
            GlobalFree(text_memory);
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to write text to the system clipboard".to_owned());
        }

        if let Some((format, marker_memory)) = marker_memory
            && SetClipboardData(format, marker_memory) == 0
        {
            GlobalFree(marker_memory);
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn write_clipboard_text_with_self_trigger(_text: &str) -> Result<(), String> {
    Err("Windows clipboard text writing is not supported on this platform".to_owned())
}

/// Writes a rich-text payload: CF_HTML (primary), the plain text form as
/// CF_UNICODETEXT and the optional RTF form, plus the private hash marker
/// derived from the plain text. WebView2 rejects `navigator.clipboard.write`
/// once the window is hidden, and bulk paste stages every item after the
/// previous delivery already hid the window, so format writes must go
/// through the backend like the text writer above.
#[cfg(target_os = "windows")]
pub fn write_clipboard_html_with_self_trigger(
    html: &str,
    plain_text: Option<&str>,
    rtf: Option<&str>,
) -> Result<(), String> {
    use super::html::{build_cf_html_payload, html_format_id};
    use super::rtf::rtf_format_id;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let plain_text = plain_text.filter(|text| !text.is_empty());
    let rtf = rtf.filter(|payload| !payload.is_empty());
    let payload = build_cf_html_payload(html)?;
    if payload.len() > MAX_NATIVE_TEXT_BYTES {
        return Err("clipboard HTML payload is too large".to_owned());
    }
    // The marker mirrors the plain-text writer: the monitor re-reads the
    // staged clipboard through the text form, so the hash must cover exactly
    // that text. Without a plain text form there is nothing to mark, the
    // same as the frontend hash flow before this writer existed.
    let marker = plain_text.map(self_trigger_marker_for_text);

    // Resolve the registered formats before touching the clipboard: a
    // registration failure must not arrive after EmptyClipboard wiped the
    // user's previous contents.
    let html_format = html_format_id()
        .ok_or_else(|| "failed to register the HTML clipboard format".to_owned())?;
    let rtf_format = rtf.and_then(|payload| rtf_format_id().map(|format| (payload, format)));

    // Allocate and fill every buffer before touching the clipboard: once
    // EmptyClipboard succeeds the user's previous contents are gone, so the
    // CF_HTML allocation failure must not be reachable after that point.
    // The text/RTF/marker forms stay best-effort and may fail harmlessly
    // later, like the file writer below.
    let html_memory = allocate_global_bytes(&payload)?;
    let wide_text = plain_text.map(|text| {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    });
    let text_memory = wide_text.as_ref().and_then(|wide| {
        wide.len()
            .checked_mul(std::mem::size_of::<u16>())
            .and_then(|wide_byte_len| {
                allocate_global_bytes(unsafe {
                    std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide_byte_len)
                })
                .ok()
            })
    });
    let rtf_memory = rtf_format
        .as_ref()
        .and_then(|(payload, _)| allocate_global_bytes(payload.as_bytes()).ok());
    // Lazy on purpose, like the sites above: the cleanup paths only free the
    // marker buffer when this is `Some`, so an eager allocation would leak it
    // whenever no self-trigger format is registered.
    let marker_memory = match (self_trigger_format_id(), marker.as_ref()) {
        (Some(format), Some(marker)) => allocate_global_bytes(marker)
            .ok()
            .map(|memory| (format, memory)),
        _ => None,
    };

    unsafe {
        if !open_clipboard_with_retry() {
            GlobalFree(html_memory);
            if let Some(memory) = text_memory {
                GlobalFree(memory);
            }
            if let Some(memory) = rtf_memory {
                GlobalFree(memory);
            }
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to open the system clipboard".to_owned());
        }
        let _clipboard_guard = ClipboardGuard;

        if EmptyClipboard() == 0 {
            GlobalFree(html_memory);
            if let Some(memory) = text_memory {
                GlobalFree(memory);
            }
            if let Some(memory) = rtf_memory {
                GlobalFree(memory);
            }
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to clear the system clipboard".to_owned());
        }

        if SetClipboardData(html_format, html_memory) == 0 {
            GlobalFree(html_memory);
            if let Some(memory) = text_memory {
                GlobalFree(memory);
            }
            if let Some(memory) = rtf_memory {
                GlobalFree(memory);
            }
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to write HTML to the system clipboard".to_owned());
        }

        if let Some(memory) = text_memory
            && SetClipboardData(CF_UNICODETEXT, memory) == 0
        {
            GlobalFree(memory);
        }
        if let (Some(memory), Some((_, format))) = (rtf_memory, rtf_format.as_ref())
            && SetClipboardData(*format, memory) == 0
        {
            GlobalFree(memory);
        }
        if let Some((format, memory)) = marker_memory
            && SetClipboardData(format, memory) == 0
        {
            GlobalFree(memory);
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn write_clipboard_html_with_self_trigger(
    _html: &str,
    _plain_text: Option<&str>,
    _rtf: Option<&str>,
) -> Result<(), String> {
    Err("Windows clipboard HTML writing is not supported on this platform".to_owned())
}

/// Builds a `CF_HDROP` payload: a DROPFILES header (`pFiles`/`pt`/`fNC`/
/// `fWide`) followed by a double-NUL-terminated list of UTF-16LE paths.
#[cfg(target_os = "windows")]
pub(super) fn build_drop_files_bytes(paths: &[String]) -> Result<Vec<u8>, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let mut utf16 = Vec::new();
    for path in paths {
        if path.is_empty() {
            return Err("no file paths to copy".to_owned());
        }
        utf16.extend(OsStr::new(path).encode_wide().flat_map(u16::to_le_bytes));
        utf16.extend_from_slice(&[0, 0]);
    }
    utf16.extend_from_slice(&[0, 0]);
    let mut header = Vec::with_capacity(20 + utf16.len());
    header.extend_from_slice(&20u32.to_le_bytes());
    header.extend_from_slice(&0i32.to_le_bytes());
    header.extend_from_slice(&0i32.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&utf16);
    Ok(header)
}

/// Writes `CF_HDROP` (dropped file references) plus the joined paths as
/// `CF_UNICODETEXT` and the private hash marker. Pasting into a file manager
/// copies the referenced files; text consumers still receive the path list.
#[cfg(target_os = "windows")]
pub fn write_clipboard_files_with_self_trigger(paths: &[String]) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    if paths.is_empty() {
        return Err("no file paths to copy".to_owned());
    }

    let buffer = build_drop_files_bytes(paths)?;

    let text = paths.join("\n");
    let marker = self_trigger_marker_for_text(&text);
    let wide = OsStr::new(&text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    // Allocate every buffer before touching the clipboard: once
    // EmptyClipboard succeeds the user's previous contents are gone, so the
    // primary payload's allocation must not be reachable after that point.
    // The text/marker forms stay best-effort and may fail harmlessly later.
    let drop_memory = allocate_global_bytes(&buffer)?;
    let text_memory =
        wide.len()
            .checked_mul(std::mem::size_of::<u16>())
            .and_then(|wide_byte_len| {
                allocate_global_bytes(unsafe {
                    std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide_byte_len)
                })
                .ok()
            });
    // Lazy on purpose, like the site above: the cleanup paths only free the
    // marker buffer when this is `Some`, so an eager `Option::zip` allocation
    // would leak it whenever no self-trigger format is registered.
    let marker_memory = match self_trigger_format_id() {
        Some(format) => allocate_global_bytes(&marker)
            .ok()
            .map(|memory| (format, memory)),
        None => None,
    };

    unsafe {
        if !open_clipboard_with_retry() {
            GlobalFree(drop_memory);
            if let Some(memory) = text_memory {
                GlobalFree(memory);
            }
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to open the system clipboard".to_owned());
        }
        let _clipboard_guard = ClipboardGuard;

        if EmptyClipboard() == 0 {
            GlobalFree(drop_memory);
            if let Some(memory) = text_memory {
                GlobalFree(memory);
            }
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to clear the system clipboard".to_owned());
        }

        if SetClipboardData(CF_HDROP, drop_memory) == 0 {
            GlobalFree(drop_memory);
            if let Some(memory) = text_memory {
                GlobalFree(memory);
            }
            if let Some((_, memory)) = marker_memory {
                GlobalFree(memory);
            }
            return Err("failed to write files to the system clipboard".to_owned());
        }

        // The file list is the primary payload. Once CF_HDROP is in place
        // the user's previous clipboard contents are already gone, so a
        // failure in the text form must not abort with an error: the files
        // would still be pasteable while the caller reports failure. Treat
        // the text form as best-effort instead.
        if let Some(text_memory) = text_memory
            && SetClipboardData(CF_UNICODETEXT, text_memory) == 0
        {
            GlobalFree(text_memory);
        }

        if let Some((format, marker_memory)) = marker_memory
            && SetClipboardData(format, marker_memory) == 0
        {
            GlobalFree(marker_memory);
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn write_clipboard_files_with_self_trigger(_paths: &[String]) -> Result<(), String> {
    Err("Windows clipboard file writing is not supported on this platform".to_owned())
}
