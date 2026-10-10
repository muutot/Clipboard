//! Live Wayland clipboard and source-app helpers: `wl-paste`/`wl-copy`
//! subprocess calls plus foreground-app and icon lookup.

#[cfg(target_os = "linux")]
use crate::platform::bounded_command::BoundedCommandExt;

// ---------------------------------------------------------------------------
//  Top-level platform dispatch functions (called from mod.rs)
// ---------------------------------------------------------------------------

/// Reads plain text from the Wayland clipboard using `wl-paste`.
#[cfg(target_os = "linux")]
pub fn read_clipboard_text() -> Option<String> {
    std::process::Command::new("wl-paste")
        .args(["--no-newline"])
        .bounded_output(64 * 1024 * 1024)
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_text() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    for target in &["image/png", "image/bmp", "image/jpeg", "image/tiff"] {
        if let Ok(output) = std::process::Command::new("wl-paste")
            .args(["--type", target])
            .bounded_output(64 * 1024 * 1024)
        {
            if output.status.success() && !output.stdout.is_empty() {
                if let Some(img) = crate::content::hash::decode_image_bytes(&output.stdout) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    return Some((rgba.into_raw(), w, h));
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    None
}

/// Reads the HTML fragment from the Wayland clipboard via wl-paste.
#[cfg(target_os = "linux")]
pub fn read_clipboard_html() -> Option<String> {
    if let Ok(output) = std::process::Command::new("wl-paste")
        .args(["--type", "text/html"])
        .bounded_output(64 * 1024 * 1024)
    {
        if output.status.success() {
            let text = String::from_utf8(output.stdout).ok()?;
            if text.trim().is_empty() {
                return None;
            }
            return Some(text);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_html() -> Option<String> {
    None
}

/// Reads the RTF payload from the Wayland clipboard via wl-paste.
#[cfg(target_os = "linux")]
pub fn read_clipboard_rtf() -> Option<String> {
    if let Ok(output) = std::process::Command::new("wl-paste")
        .args(["--type", "text/rtf"])
        .bounded_output(64 * 1024 * 1024)
    {
        if output.status.success() {
            let text = String::from_utf8(output.stdout).ok()?;
            if text.trim().is_empty() {
                return None;
            }
            return Some(text);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_rtf() -> Option<String> {
    None
}

/// Reads file paths from the Wayland clipboard via wl-paste (text/uri-list).
#[cfg(target_os = "linux")]
pub fn read_clipboard_file_paths() -> Vec<String> {
    if let Ok(output) = std::process::Command::new("wl-paste")
        .args(["--type", "text/uri-list"])
        .bounded_output(64 * 1024 * 1024)
    {
        if output.status.success() {
            let text = String::from_utf8(output.stdout).unwrap_or_default();
            return crate::platform::linux::parse_uri_list(&text);
        }
    }
    vec![]
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_file_paths() -> Vec<String> {
    vec![]
}

/// Returns the foreground application on Wayland using `swaymsg` or `hyprctl`.
#[cfg(target_os = "linux")]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    // Compositor window ids are not OS process ids. Reuse the JSON query
    // used by quick paste, but retain our own pid for source attribution.
    let pid = crate::platform::linux::wayland::paste::foreground_target()
        .map(|target| u64::from(target.pid))
        .or_else(|| {
            // Fallback: use xdotool (works in XWayland sessions)
            std::process::Command::new("xdotool")
                .args(["getactivewindow", "getwindowpid"])
                .bounded_output(64 * 1024 * 1024)
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        String::from_utf8(output.stdout)
                            .ok()
                            .and_then(|s| s.trim().parse::<u64>().ok())
                    } else {
                        None
                    }
                })
        });

    let pid_u32 = pid.and_then(|p| u32::try_from(p).ok());

    let (name, exe_path) = match pid_u32 {
        Some(pid) => {
            let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .ok()
                .map(|s| s.trim().to_owned())
                .unwrap_or_default();

            let exe_path = std::fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            (name, exe_path)
        }
        None => (String::new(), String::new()),
    };

    crate::platform::ForegroundApp { name, exe_path }
}

#[cfg(not(target_os = "linux"))]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    crate::platform::ForegroundApp::empty()
}

#[cfg(target_os = "linux")]
pub fn extract_app_icon(
    icon_dir: &std::path::Path,
    app_name: &str,
    exe_path: &str,
) -> Option<String> {
    crate::platform::linux::icons::ensure_cached_app_icon(icon_dir, app_name, exe_path)
}

#[cfg(not(target_os = "linux"))]
pub fn extract_app_icon(
    _icon_dir: &std::path::Path,
    _app_name: &str,
    _exe_path: &str,
) -> Option<String> {
    None
}

/// Writes text to the Wayland clipboard using `wl-copy`.
#[cfg(target_os = "linux")]
pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    use std::io::Write;

    let mut child = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn wl-copy: {e}"))?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("failed to write to wl-copy stdin: {e}"))?;
    }

    child
        .wait()
        .map_err(|e| format!("wl-copy wait failed: {e}"))?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn write_clipboard_text_with_self_trigger(_text: &str) -> Result<(), String> {
    Err("Wayland clipboard writing is not supported on this platform".to_owned())
}
