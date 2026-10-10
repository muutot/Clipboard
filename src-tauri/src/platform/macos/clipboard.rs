//! Live macOS clipboard readers/writers plus foreground-app and icon lookup.

#[cfg(target_os = "macos")]
use super::objc;
#[cfg(target_os = "macos")]
use crate::platform::macos_image;

// ---------------------------------------------------------------------------
//  Top-level platform dispatch functions (called from mod.rs)
//  Uses command-line tools (pbpaste/pbcopy/osascript) since the Objective-C
//  runtime cannot be called via direct C FFI without the `objc` crate.
// ---------------------------------------------------------------------------

/// Reads plain text from the system clipboard using native NSPasteboard API.
#[cfg(target_os = "macos")]
pub fn read_clipboard_text() -> Option<String> {
    let pool = unsafe { objc::objc_autoreleasePoolPush() };
    let pb = objc::get_nspasteboard();
    let result = objc::pasteboard_string_for_type(pb, "public.utf8-plain-text");
    unsafe { objc::objc_autoreleasePoolPop(pool) };
    result
}

#[cfg(not(target_os = "macos"))]
pub fn read_clipboard_text() -> Option<String> {
    None
}

/// Reads the HTML fragment (`public.html` UTI) from the general pasteboard.
#[cfg(target_os = "macos")]
pub fn read_clipboard_html() -> Option<String> {
    let pool = unsafe { objc::objc_autoreleasePoolPush() };
    let pb = objc::get_nspasteboard();
    let result = objc::pasteboard_string_for_type(pb, "public.html");
    unsafe { objc::objc_autoreleasePoolPop(pool) };
    result
}

#[cfg(not(target_os = "macos"))]
pub fn read_clipboard_html() -> Option<String> {
    None
}

/// Reads NSData rather than stringForType: so legacy RTF code-page bytes
/// are preserved as hex escapes instead of being decoded lossily as UTF-8.
#[cfg(target_os = "macos")]
pub fn read_clipboard_rtf() -> Option<String> {
    let pool = unsafe { objc::objc_autoreleasePoolPush() };
    let result = (|| {
        let pb = objc::get_nspasteboard();
        if pb.is_null() {
            return None;
        }
        let data = unsafe {
            objc::msgSend_id_id(
                pb,
                objc::sel_registerName(c"dataForType:".as_ptr()),
                objc::nsstring_from_str("public.rtf"),
            )
        };
        if data.is_null() {
            return None;
        }
        let len = unsafe { objc::msgSend_isize(data, objc::sel_registerName(c"length".as_ptr())) };
        if !(1..=16 * 1024 * 1024).contains(&len) {
            return None;
        }
        let bytes = unsafe { objc::msgSend_ptr(data, objc::sel_registerName(c"bytes".as_ptr())) };
        if bytes.is_null() {
            return None;
        }
        // NSData remains alive inside this autorelease pool until copied.
        decode_rtf_bytes(unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len as usize) })
    })();
    unsafe { objc::objc_autoreleasePoolPop(pool) };
    result
}

/// The metadata contract stores RTF as a UTF-8 string. Hex escaping retains
/// original ANSI/code-page bytes. Raw binary runs cannot safely pass through
/// that contract, so reject them (HTML/plain text remain available).
fn decode_rtf_bytes(bytes: &[u8]) -> Option<String> {
    use std::fmt::Write;
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    if !bytes.starts_with(b"{\\rtf") || bytes.len() > 16 * 1024 * 1024 {
        return None;
    }
    let mut result = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == 0 {
            return None;
        }
        if byte == b'\\' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                end += 1;
            }
            if &bytes[start..end] == b"bin" {
                return None;
            }
            // Escaped backslashes/braces and hex prefixes are control symbols,
            // not starts of another control word.
            if end == start && start < bytes.len() && bytes[start] != 0 && bytes[start].is_ascii() {
                result.push('\\');
                result.push(bytes[start] as char);
                i += 2;
                continue;
            }
        }
        if byte.is_ascii() {
            result.push(byte as char);
        } else {
            write!(&mut result, "\\'{byte:02x}").ok()?;
        }
        i += 1;
    }
    Some(result)
}

#[cfg(test)]
mod rtf_tests {
    use super::decode_rtf_bytes;
    #[test]
    fn preserves_rtf_controls_and_code_page_bytes() {
        let rtf = b"{\\rtf1\\ansi\\ansicpg1252 caf\xe9}";
        assert_eq!(
            decode_rtf_bytes(rtf).as_deref(),
            Some("{\\rtf1\\ansi\\ansicpg1252 caf\\'e9}")
        );
        assert_eq!(
            decode_rtf_bytes(b"{\\rtf1 \\u20320?}"),
            Some("{\\rtf1 \\u20320?}".into())
        );
    }
    #[test]
    fn rejects_binary_runs_without_confusing_escaped_text() {
        assert!(decode_rtf_bytes(b"{\\rtf1\\bin3 abc}").is_none());
        assert!(decode_rtf_bytes(b"plain text").is_none());
        assert!(decode_rtf_bytes(b"{\\rtf1 \\\\bin3}").is_some());
        assert!(decode_rtf_bytes(b"{\\rtf1 \0}").is_none());
    }
}

#[cfg(not(target_os = "macos"))]
pub fn read_clipboard_rtf() -> Option<String> {
    None
}

/// Reads clipboard images with bounded tools and private temporary files.
#[cfg(target_os = "macos")]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    macos_image::read_clipboard_image()
}

#[cfg(not(target_os = "macos"))]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    None
}

/// Reads file paths from files copied in Finder via the pasteboard's
/// `NSFilenamesPboardType` list (still written alongside the modern
/// `public.file-url` type). An absent list yields an empty vector — the
/// previous behavior — so this can only add coverage, never regress.
#[cfg(target_os = "macos")]
pub fn read_clipboard_file_paths() -> Vec<String> {
    let pool = unsafe { objc::objc_autoreleasePoolPush() };
    let paths = read_nsfilenames_paths();
    unsafe { objc::objc_autoreleasePoolPop(pool) };
    paths
}

#[cfg(target_os = "macos")]
fn read_nsfilenames_paths() -> Vec<String> {
    let pb = objc::get_nspasteboard();
    if pb.is_null() {
        return Vec::new();
    }
    let list = objc::pasteboard_property_list_for_type(pb, "NSFilenamesPboardType");
    if list.is_null() {
        return Vec::new();
    }
    let count = objc::array_count(list).max(0) as usize;
    (0..count)
        .filter_map(|index| {
            let item = objc::array_object_at_index(list, index as isize);
            objc::nsstring_to_str(item)
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn read_clipboard_file_paths() -> Vec<String> {
    vec![]
}

/// Returns the foreground (frontmost) application using native NSWorkspace API.
#[cfg(target_os = "macos")]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    let pool = unsafe { objc::objc_autoreleasePoolPush() };
    let ws = objc::get_nsworkspace();
    let app = objc::workspace_frontmost_app(ws);
    let name = objc::running_app_name(app).unwrap_or_default();
    let exe_path = objc::running_app_exe_path(app).unwrap_or_default();
    unsafe { objc::objc_autoreleasePoolPop(pool) };
    crate::platform::ForegroundApp { name, exe_path }
}

#[cfg(not(target_os = "macos"))]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    crate::platform::ForegroundApp::empty()
}

/// Extracts an app icon from the .app bundle on macOS.
#[cfg(target_os = "macos")]
pub fn extract_app_icon(
    icon_dir: &std::path::Path,
    app_name: &str,
    exe_path: &str,
) -> Option<String> {
    use crate::platform::bounded_command::BoundedCommandExt;

    let icon_key = crate::content::icon_key(app_name);
    let dest = icon_dir.join(format!("{}.png", icon_key));
    if dest.exists() {
        return Some(dest.to_string_lossy().to_string());
    }

    // Walk up from exe_path to find the .app bundle.
    let bundle = crate::platform::macos_app_bundle_from_exe(exe_path)?;

    // Read CFBundleIconFile from Info.plist via plutil
    let info_plist = bundle.join("Contents/Info.plist");
    if !info_plist.exists() {
        return None;
    }
    let plist_json = std::process::Command::new("plutil")
        .args(["-convert", "json", "-o", "-", &info_plist.to_string_lossy()])
        .bounded_output(1024 * 1024)
        .ok()?;
    if !plist_json.status.success() {
        return None;
    }
    let plist: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_slice(&plist_json.stdout).ok()?;

    let icon_name = plist
        .get("CFBundleIconFile")
        .and_then(|v| v.as_str())
        .unwrap_or("icon");
    let icon_name = icon_name.trim_end_matches(".icns");

    // Search for the .icns in Resources
    let resources = bundle.join("Contents/Resources");
    let mut icns_path = None;
    for entry in std::fs::read_dir(&resources).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == format!("{icon_name}.icns")
            || name_str.eq_ignore_ascii_case(&format!("{icon_name}.icns"))
        {
            icns_path = Some(entry.path());
            break;
        }
    }

    let icns_path = icns_path?;

    std::fs::create_dir_all(icon_dir).ok()?;
    macos_image::convert_icon(&icns_path, &dest)?;
    Some(dest.to_string_lossy().to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn extract_app_icon(
    _icon_dir: &std::path::Path,
    _app_name: &str,
    _exe_path: &str,
) -> Option<String> {
    None
}

/// Writes text to the system clipboard using `pbcopy`.
#[cfg(target_os = "macos")]
pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    use std::io::Write;

    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn pbcopy: {e}"))?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("failed to write to pbcopy stdin: {e}"))?;
    }

    child
        .wait()
        .map_err(|e| format!("pbcopy wait failed: {e}"))?;

    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn write_clipboard_text_with_self_trigger(_text: &str) -> Result<(), String> {
    Err("macOS clipboard writing is not supported on this platform".to_owned())
}
