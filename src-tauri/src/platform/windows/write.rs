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

        if let Some((format, marker_memory)) = marker_memory {
            if SetClipboardData(format, marker_memory) == 0 {
                GlobalFree(marker_memory);
            }
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn write_clipboard_text_with_self_trigger(_text: &str) -> Result<(), String> {
    Err("Windows clipboard text writing is not supported on this platform".to_owned())
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
        if let Some(text_memory) = text_memory {
            if SetClipboardData(CF_UNICODETEXT, text_memory) == 0 {
                GlobalFree(text_memory);
            }
        }

        if let Some((format, marker_memory)) = marker_memory {
            if SetClipboardData(format, marker_memory) == 0 {
                GlobalFree(marker_memory);
            }
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn write_clipboard_files_with_self_trigger(_paths: &[String]) -> Result<(), String> {
    Err("Windows clipboard file writing is not supported on this platform".to_owned())
}
