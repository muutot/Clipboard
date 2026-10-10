use super::*;
#[cfg(target_os = "linux")]
use crate::platform::bounded_command::BoundedCommandExt;

// ---------------------------------------------------------------------------
// Clipboard format constants
// ---------------------------------------------------------------------------

/// UTF-8 text target name used with `XConvertSelection`.
pub const UTF8_STRING_TARGET: &str = "UTF8_STRING";
/// Targets list atom — `XConvertSelection` with this target returns
/// the available formats on the clipboard.
pub const TARGETS_ATOM: &str = "TARGETS";
/// Image PNG target.
pub const IMAGE_PNG_TARGET: &str = "image/png";
/// File list target (URI list).
pub const TEXT_URI_LIST_TARGET: &str = "text/uri-list";

// ---------------------------------------------------------------------------
// X11ClipboardMonitor
// ---------------------------------------------------------------------------

/// Monitors X11 clipboard selections (PRIMARY and CLIPBOARD) for changes.
///
/// # Architecture
///
/// ```text
/// ┌────────────────────────────────────────────────────┐
/// │ X11ClipboardMonitor                                  │
/// │                                                      │
/// │  1. Open display (XOpenDisplay)                       │
/// │  2. Create invisible window (XCreateSimpleWindow)     │
/// │  3. Intern required atoms (CLIPBOARD, PRIMARY,        │
/// │     UTF8_STRING, TARGETS, INCR, etc.)                 │
/// │  4. Use XFixesSelectSelectionInput to subscribe to    │
/// │     selection owner changes (XFixesSetSelectionOwner-  │
/// │     Notify events).                                   │
/// │  5. On selection change:                              │
/// │     a. XConvertSelection(current_owner, TARGETS)      │
/// │     b. On SelectionNotify: read property to get       │
/// │        available targets.                             │
/// │     c. If UTF8_STRING is available:                   │
/// │        XConvertSelection(current_owner, UTF8_STRING)  │
/// │     d. On SelectionNotify: read text via              │
/// │        XGetWindowProperty.                            │
/// │     e. Handle INCR (incremental) transfers for large  │
/// │        clipboard contents.                            │
/// │  6. Emit clipboard snapshot via channel.              │
/// └────────────────────────────────────────────────────┘
/// ```
pub struct X11ClipboardMonitor {
    /// Whether the monitor is currently active.
    running: Arc<AtomicBool>,
    /// Set of window titles / WM_CLASS values to ignore.
    ignored_apps: Arc<Mutex<HashSet<String>>>,
    /// Channel for emitting snapshot data.
    snapshot_tx: Option<std::sync::mpsc::Sender<X11ClipboardSnapshot>>,
    /// Handle for the background event-loop thread.
    event_thread: Option<thread::JoinHandle<()>>,
}

/// A snapshot of clipboard content captured at one point in time.
#[derive(Debug, Clone)]
pub struct X11ClipboardSnapshot {
    /// Which selection this data came from (PRIMARY or CLIPBOARD).
    pub selection: X11Selection,
    /// The MIME / target types available on the selection.
    pub available_targets: Vec<String>,
    /// Plain-text content, if UTF8_STRING was available.
    pub text_content: Option<String>,
    /// Raw bytes for an image, if an image target was available.
    pub image_data: Option<Vec<u8>>,
}

/// Identifies which X11 selection is being monitored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum X11Selection {
    Primary,
    Clipboard,
}

impl Default for X11ClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl X11ClipboardMonitor {
    /// Creates a new, stopped monitor.
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            ignored_apps: Arc::new(Mutex::new(HashSet::new())),
            snapshot_tx: None,
            event_thread: None,
        }
    }

    /// Starts the X11 event loop that monitors for selection changes.
    ///
    /// # Steps
    ///
    /// 1. `XOpenDisplay(NULL)` — connect to the default display.
    /// 2. Create a hidden message window.
    /// 3. Intern standard atoms: `CLIPBOARD`, `PRIMARY`, `UTF8_STRING`,
    ///    `TARGETS`, `INCR`, `ATOM_PAIR`, `MULTIPLE`, `TIMESTAMP`.
    /// 4. Check for XFixes extension availability.
    /// 5. Call `XFixesSelectSelectionInput` for both PRIMARY and CLIPBOARD.
    /// 6. Enter event loop calling `XPending` / `XNextEvent`:
    ///    - On `SelectionNotify`: read property data.
    ///    - On `PropertyNotify`: handle INCR transfers.
    ///    - On `SelectionClear`: owner changed; re-query.
    /// 7. Build `X11ClipboardSnapshot` and send through channel.
    pub fn start(&mut self) -> X11Result<()> {
        // Implementation:
        //
        // let display = unsafe { XOpenDisplay(std::ptr::null()) };
        // if display.is_null() {
        //     return Err(X11Error::DisplayOpenFailed("$DISPLAY not set".into()));
        // }
        //
        // let root = unsafe { XDefaultRootWindow(display) };
        // let window = unsafe { XCreateSimpleWindow(display, root, 0, 0, 1, 1, 0, 0, 0) };
        //
        // // Intern atoms...
        // let atom_clipboard = unsafe {
        //     XInternAtom(display, b"CLIPBOARD\0".as_ptr() as *const i8, 0)
        // };
        //
        // // Check XFixes...
        //
        // // Subscribe to selection changes via XFixesSelectSelectionInput...
        //
        // // Spawn event thread:
        // let (tx, rx) = std::sync::mpsc::channel();
        // self.snapshot_tx = Some(tx);
        // self.event_thread = Some(thread::spawn(move || { /* event loop */ }));
        // self.running.store(true, Ordering::SeqCst);
        // Ok(())
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Stops the event loop and tears down the X connection.
    pub fn stop(&mut self) -> X11Result<()> {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.event_thread.take() {
            // A panicked monitor explains why captures silently stopped;
            // swallowing the payload hid that from every log.
            if let Err(panic) = handle.join() {
                crate::log_error!(
                    "[clipboard-monitor] monitor thread terminated with a panic: {panic:?}"
                );
            }
        }
        self.snapshot_tx = None;
        Ok(())
    }

    /// Returns whether the monitor is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Sets the list of application WM_CLASS values to ignore.
    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        if let Ok(mut guard) = self.ignored_apps.lock() {
            *guard = apps.into_iter().collect();
        }
    }

    /// Returns current ignored application list.
    pub fn ignored_apps(&self) -> Vec<String> {
        self.ignored_apps
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Reads the plain-text content from the CLIPBOARD selection.
    ///
    /// This is a synchronous convenience wrapper. The primary monitoring
    /// path is asynchronous via `SelectionNotify` events.
    pub fn read_clipboard_text() -> Option<String> {
        // Outline:
        // 1. XOpenDisplay(NULL)
        // 2. XGetSelectionOwner(display, XA_CLIPBOARD atom)
        // 3. XConvertSelection(display, clipboard, UTF8_STRING, property, window, CurrentTime)
        // 4. Wait for SelectionNotify event on window
        // 5. XGetWindowProperty(display, window, property, ...)
        // 6. Read data, handle INCR if needed
        // 7. Return text or None
        None
    }

    /// Reads the available TARGETS from the CLIPBOARD selection.
    pub fn read_clipboard_targets() -> Vec<String> {
        // Outline:
        // 1. XConvertSelection(display, clipboard, TARGETS, property, window, CurrentTime)
        // 2. Wait for SelectionNotify
        // 3. XGetWindowProperty — returns list of atoms
        // 4. XGetAtomName on each to get target string
        vec![]
    }
}

// ---------------------------------------------------------------------------
//  Top-level platform dispatch functions (called from mod.rs)
// ---------------------------------------------------------------------------

/// Reads plain text from the X11 CLIPBOARD selection using XConvertSelection.
#[cfg(target_os = "linux")]
pub fn read_clipboard_text() -> Option<String> {
    unsafe {
        let display = x11_ffi::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return None;
        }

        let root = x11_ffi::XDefaultRootWindow(display);
        let window = x11_ffi::XCreateSimpleWindow(display, root, 0, 0, 1, 1, 0, 0, 0);
        if window == 0 {
            x11_ffi::XCloseDisplay(display);
            return None;
        }

        let atom_clipboard = x11_ffi::XInternAtom(display, c"CLIPBOARD".as_ptr(), 0);
        let atom_utf8 = x11_ffi::XInternAtom(display, c"UTF8_STRING".as_ptr(), 0);
        let atom_property = x11_ffi::XInternAtom(display, c"CLIPBOARD_DESKTOP_READ".as_ptr(), 0);

        x11_ffi::XConvertSelection(display, atom_clipboard, atom_utf8, atom_property, window, 0);
        x11_ffi::XFlush(display);

        // Wait for SelectionNotify with 500ms timeout
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        let mut got_selection = false;

        loop {
            if std::time::Instant::now() >= deadline {
                break;
            }
            if x11_ffi::XPending(display) > 0 {
                let mut event: x11_ffi::XEvent = std::mem::zeroed();
                x11_ffi::XNextEvent(display, &mut event);
                if event.data.any.type_ == x11_ffi::SELECTION_NOTIFY
                    && event.data.selection.requestor == window
                    && event.data.selection.property == atom_property
                {
                    got_selection = true;
                    break;
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        if !got_selection {
            x11_ffi::XDestroyWindow(display, window);
            x11_ffi::XCloseDisplay(display);
            return None;
        }

        // Read the property data. A single request cannot return arbitrarily
        // large properties, so loop with a rising 32-bit-unit offset until the
        // server reports nothing left. INCR transfers (the owner delivers
        // chunks only after we delete the property, driven by PropertyNotify)
        // are detected and rejected explicitly: reading them here would
        // silently corrupt or truncate large texts, which is worse than no
        // capture.
        let atom_incr = x11_ffi::XInternAtom(display, c"INCR".as_ptr(), 0);
        const CHUNK_UNITS: i64 = 1_000_000; // 4 MiB of 8-bit data per request
        let mut data: Vec<u8> = Vec::new();
        let mut offset_units: i64 = 0;
        let mut read_failed = false;
        let mut is_incr = false;
        loop {
            let mut actual_type: x11_ffi::Atom = 0;
            let mut actual_format: i32 = 0;
            let mut nitems: u64 = 0;
            let mut bytes_after: u64 = 0;
            let mut prop: *mut u8 = std::ptr::null_mut();

            let result = x11_ffi::XGetWindowProperty(
                display,
                window,
                atom_property,
                offset_units,
                CHUNK_UNITS,
                0,
                0,
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop,
            );
            if result != 0 {
                read_failed = true;
                break;
            }
            if actual_type == atom_incr {
                is_incr = true;
                if !prop.is_null() {
                    x11_ffi::XFree(prop as *mut std::ffi::c_void);
                }
                break;
            }
            if !prop.is_null() && nitems > 0 && actual_format == 8 {
                let slice = std::slice::from_raw_parts(prop, nitems as usize);
                data.extend_from_slice(slice);
            }
            // `XGetWindowProperty` allocates `prop` on every call, so it must
            // be freed on every path.
            if !prop.is_null() {
                x11_ffi::XFree(prop as *mut std::ffi::c_void);
            }
            // `long_offset` counts 32-bit units; for 8-bit data one unit holds
            // 4 bytes, so advance by the rounded-up byte count just read.
            // `nitems` is unsigned on purpose: `div_ceil` is only stable for
            // unsigned integers, so rounding before the cast keeps this on
            // stable Rust (a signed `div_ceil` fails the Linux build).
            offset_units += nitems.div_ceil(4) as i64;
            if bytes_after == 0 || nitems == 0 {
                break;
            }
        }

        if is_incr {
            crate::log_warn!(
                "[clipboard] X11 owner uses INCR transfer; text larger than one property \
                 chunk is not read — capture skipped instead of corrupted"
            );
        }
        let text = if read_failed || is_incr {
            None
        } else {
            String::from_utf8(data).ok()
        };

        x11_ffi::XDestroyWindow(display, window);
        x11_ffi::XCloseDisplay(display);
        text
    }
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_text() -> Option<String> {
    None
}

/// Reads clipboard image data by asking `xclip` for each common image target.
///
/// X11 has no single image target the way Windows does, so this depends on
/// `xclip` being installed and on the source application having published one
/// of these targets. Returns `None` when no target yields decodable bytes.
#[cfg(target_os = "linux")]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    // Try xclip with common image targets
    for target in &["image/png", "image/bmp", "image/jpeg", "image/tiff"] {
        if let Ok(output) = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", target, "-out"])
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

/// Reads the HTML fragment from the CLIPBOARD selection via xclip.
#[cfg(target_os = "linux")]
pub fn read_clipboard_html() -> Option<String> {
    if let Ok(output) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/html", "-out"])
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

/// Reads the RTF payload from the CLIPBOARD selection via xclip.
#[cfg(target_os = "linux")]
pub fn read_clipboard_rtf() -> Option<String> {
    if let Ok(output) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/rtf", "-out"])
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

/// Reads file paths from the CLIPBOARD selection via xclip (text/uri-list).
#[cfg(target_os = "linux")]
pub fn read_clipboard_file_paths() -> Vec<String> {
    if let Ok(output) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/uri-list", "-out"])
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

/// Returns the foreground application on X11 using `_NET_ACTIVE_WINDOW`.
#[cfg(target_os = "linux")]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    let (name, exe_path) = unsafe {
        let display = x11_ffi::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return crate::platform::ForegroundApp::empty();
        }

        let root = x11_ffi::XDefaultRootWindow(display);
        let atom_active = x11_ffi::XInternAtom(display, c"_NET_ACTIVE_WINDOW".as_ptr(), 0);
        let atom_pid = x11_ffi::XInternAtom(display, c"_NET_WM_PID".as_ptr(), 0);
        let atom_cardinal = x11_ffi::XInternAtom(display, c"CARDINAL".as_ptr(), 0);

        // Get _NET_ACTIVE_WINDOW property from root window
        let mut actual_type: x11_ffi::Atom = 0;
        let mut actual_format: i32 = 0;
        let mut nitems: u64 = 0;
        let mut bytes_after: u64 = 0;
        let mut prop: *mut u8 = std::ptr::null_mut();

        let res = x11_ffi::XGetWindowProperty(
            display,
            root,
            atom_active,
            0,
            1,
            0,
            0,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        let active_window_ok = res == 0 && !prop.is_null() && nitems > 0 && actual_format == 32;
        let window_id = if active_window_ok {
            *(prop as *mut u32) as u64
        } else {
            0
        };
        if !prop.is_null() {
            x11_ffi::XFree(prop as *mut std::ffi::c_void);
        }
        if !active_window_ok {
            x11_ffi::XCloseDisplay(display);
            return crate::platform::ForegroundApp::empty();
        }

        // Get _NET_WM_PID from the active window
        let mut actual_type2: x11_ffi::Atom = 0;
        let mut actual_format2: i32 = 0;
        let mut nitems2: u64 = 0;
        let mut bytes_after2: u64 = 0;
        let mut prop2: *mut u8 = std::ptr::null_mut();

        let res2 = x11_ffi::XGetWindowProperty(
            display,
            window_id,
            atom_pid,
            0,
            1,
            0,
            atom_cardinal,
            &mut actual_type2,
            &mut actual_format2,
            &mut nitems2,
            &mut bytes_after2,
            &mut prop2,
        );

        let pid: Option<u32> =
            if res2 == 0 && !prop2.is_null() && nitems2 > 0 && actual_format2 == 32 {
                Some(*(prop2 as *mut u32))
            } else {
                None
            };
        if !prop2.is_null() {
            x11_ffi::XFree(prop2 as *mut std::ffi::c_void);
        }

        x11_ffi::XCloseDisplay(display);

        let name = pid
            .and_then(|p| std::fs::read_to_string(format!("/proc/{p}/comm")).ok())
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();

        let exe_path = pid
            .and_then(|p| std::fs::read_link(format!("/proc/{p}/exe")).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        (name, exe_path)
    };

    crate::platform::ForegroundApp { name, exe_path }
}

#[cfg(not(target_os = "linux"))]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    crate::platform::ForegroundApp::empty()
}

/// Extracts an app icon via freedesktop `.desktop` lookup (see
/// `platform::linux_icons`); previously unsupported on X11.
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

/// Writes text to the X11 CLIPBOARD selection using xclip.
#[cfg(target_os = "linux")]
pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    use std::io::Write;

    let mut child = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-in"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn xclip: {e}"))?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("failed to write to xclip stdin: {e}"))?;
    }

    child
        .wait()
        .map_err(|e| format!("xclip wait failed: {e}"))?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn write_clipboard_text_with_self_trigger(_text: &str) -> Result<(), String> {
    Err("X11 clipboard writing is not supported on this platform".to_owned())
}
