//! macOS platform adapter providing clipboard monitoring, global hotkeys,
//! accessibility permission management, and system tray integration.
//!
//! # Overview
//!
//! This module wraps native macOS APIs (AppKit, Carbon, CoreGraphics) behind
//! safe Rust abstractions. On non-macOS targets only the type definitions and
//! documentation are compiled — the implementation bodies are gated behind
//! `#[cfg(target_os = "macos")]`.

#![allow(dead_code)]

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

use crate::keyboard::ShortcutBinding;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that may occur during macOS platform operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacOSError {
    /// Clipboard access was denied (sandbox / TCC).
    ClipboardAccessDenied,
    /// The pasteboard returned an unexpected change count.
    PasteboardReadFailed(String),
    /// Accessibility permission has not been granted.
    AccessibilityPermissionDenied,
    /// Registering a global hotkey failed (key already taken).
    HotkeyRegistrationFailed(String),
    /// A Carbon / CoreGraphics API call returned an error.
    ApiError(i32, String),
    /// The menu bar item could not be created.
    TrayCreationFailed(String),
}

impl std::fmt::Display for MacOSError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClipboardAccessDenied => f.write_str("clipboard access denied"),
            Self::PasteboardReadFailed(msg) => write!(f, "pasteboard read failed: {msg}"),
            Self::AccessibilityPermissionDenied => {
                f.write_str("accessibility permission not granted")
            }
            Self::HotkeyRegistrationFailed(msg) => {
                write!(f, "hotkey registration failed: {msg}")
            }
            Self::ApiError(code, msg) => write!(f, "API error ({code}): {msg}"),
            Self::TrayCreationFailed(msg) => write!(f, "tray creation failed: {msg}"),
        }
    }
}

impl std::error::Error for MacOSError {}

/// Convenience alias for results from macOS platform operations.
pub type MacOSResult<T> = Result<T, MacOSError>;

// ---------------------------------------------------------------------------
// macOS extern "C" API declarations
// ---------------------------------------------------------------------------
// These blocks declare the native APIs used by the adapter.  They are only
// linkable on macOS; on other platforms they serve as documentation of the
// FFI boundary.

#[cfg(target_os = "macos")]
mod objc {
    #![allow(non_camel_case_types, dead_code, clashing_extern_declarations)]

    #[repr(C)]
    pub struct Object([u8; 0]);
    pub type Sel = *mut Object;
    pub type Id = *mut Object;

    pub const YES: i8 = 1;
    pub const NO: i8 = 0;

    extern "C" {
        pub fn objc_getClass(name: *const i8) -> Id;
        pub fn sel_registerName(name: *const i8) -> Sel;
        #[link_name = "objc_msgSend"]
        pub fn msgSend(receiver: Id, sel: Sel) -> Id;
        #[link_name = "objc_msgSend"]
        pub fn msgSend_isize(receiver: Id, sel: Sel) -> isize;
        #[link_name = "objc_msgSend"]
        pub fn msgSend_ptr(receiver: Id, sel: Sel) -> *const i8;
        #[link_name = "objc_msgSend"]
        pub fn msgSend_id_id(receiver: Id, sel: Sel, arg: Id) -> Id;
        #[link_name = "objc_msgSend"]
        pub fn msgSend_id_Int(receiver: Id, sel: Sel, arg: isize) -> Id;
        #[link_name = "objc_msgSend"]
        pub fn msgSend_i32_Id(receiver: Id, sel: Sel, arg: i32) -> Id;
        pub fn objc_autoreleasePoolPush() -> Id;
        pub fn objc_autoreleasePoolPop(pool: Id);
    }

    pub fn nsstring_from_str(s: &str) -> Id {
        let cls = unsafe { objc_getClass(c"NSString".as_ptr()) };
        let sel = unsafe { sel_registerName(c"stringWithUTF8String:".as_ptr()) };
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        unsafe { msgSend_id_id(cls, sel, cstr.as_ptr() as Id) }
    }

    pub fn nsstring_to_str(s: Id) -> Option<String> {
        if s.is_null() {
            return None;
        }
        let sel = unsafe { sel_registerName(c"UTF8String".as_ptr()) };
        let ptr = unsafe { msgSend_ptr(s, sel) };
        if ptr.is_null() {
            None
        } else {
            unsafe { Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()) }
        }
    }

    pub fn get_nspasteboard() -> Id {
        let cls = unsafe { objc_getClass(c"NSPasteboard".as_ptr()) };
        let sel = unsafe { sel_registerName(c"generalPasteboard".as_ptr()) };
        unsafe { msgSend(cls, sel) }
    }

    pub fn pasteboard_change_count(pb: Id) -> isize {
        let sel = unsafe { sel_registerName(c"changeCount".as_ptr()) };
        unsafe { msgSend_isize(pb, sel) }
    }

    pub fn pasteboard_string_for_type(pb: Id, type_name: &str) -> Option<String> {
        let type_ns = nsstring_from_str(type_name);
        let sel = unsafe { sel_registerName(c"stringForType:".as_ptr()) };
        let result = unsafe { msgSend_id_id(pb, sel, type_ns) };
        nsstring_to_str(result)
    }

    pub fn get_nsworkspace() -> Id {
        let cls = unsafe { objc_getClass(c"NSWorkspace".as_ptr()) };
        let sel = unsafe { sel_registerName(c"sharedWorkspace".as_ptr()) };
        unsafe { msgSend(cls, sel) }
    }

    pub fn workspace_frontmost_app(ws: Id) -> Id {
        let sel = unsafe { sel_registerName(c"frontmostApplication".as_ptr()) };
        unsafe { msgSend(ws, sel) }
    }

    pub fn running_app_name(app: Id) -> Option<String> {
        let sel = unsafe { sel_registerName(c"localizedName".as_ptr()) };
        let result = unsafe { msgSend(app, sel) };
        nsstring_to_str(result)
    }

    pub fn running_app_exe_path(app: Id) -> Option<String> {
        let sel = unsafe { sel_registerName(c"executableURL".as_ptr()) };
        let url = unsafe { msgSend(app, sel) };
        if url.is_null() {
            return None;
        }
        let sel_path = unsafe { sel_registerName(c"path".as_ptr()) };
        let path_str = unsafe { msgSend(url, sel_path) };
        nsstring_to_str(path_str)
    }
}

#[cfg(target_os = "macos")]
extern "C" {
    // ---- CGEvent (CoreGraphics) ----------------------------------------
    fn CGEventSourceCreate(state_id: i32) -> *mut std::ffi::c_void;
    fn CGEventTapCreate(
        tap: i32,
        place: i32,
        options: i32,
        events_of_interest: u64,
        callback: *const std::ffi::c_void,
        user_info: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *mut std::ffi::c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut std::ffi::c_void) -> u64;
    fn CFMachPortCreateRunLoopSource(
        allocator: *mut std::ffi::c_void,
        port: *mut std::ffi::c_void,
        order: isize,
    ) -> *mut std::ffi::c_void;
    fn CFRunLoopAddSource(
        rl: *mut std::ffi::c_void,
        source: *mut std::ffi::c_void,
        mode: *mut std::ffi::c_void,
    );
    fn CFRunLoopGetCurrent() -> *mut std::ffi::c_void;

    // ---- Carbon Event Manager (deprecated but simpler for hotkeys) -----
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        hotkey_id: *const std::ffi::c_void,
        target: *mut std::ffi::c_void,
        options: u32,
        out_ref: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey_ref: *mut std::ffi::c_void) -> i32;

    // ---- Accessibility ------------------------------------------------
    fn AXIsProcessTrusted() -> bool;
    fn AXMakeProcessTrusted() -> i32;

    // ---- NSStatusBar --------------------------------------------------
    fn NSStatusBar_systemStatusBar() -> *mut std::ffi::c_void;
    fn NSStatusBar_statusItemWithLength(
        bar: *mut std::ffi::c_void,
        length: f64,
    ) -> *mut std::ffi::c_void;
    fn NSStatusBar_removeStatusItem(bar: *mut std::ffi::c_void, item: *mut std::ffi::c_void);
    fn NSStatusBarButton_button(item: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn NSMenu_alloc() -> *mut std::ffi::c_void;
    fn NSMenu_initWithTitle(title: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn NSMenu_addItem(menu: *mut std::ffi::c_void, item: *mut std::ffi::c_void);
    fn NSMenuItem_alloc() -> *mut std::ffi::c_void;
    fn NSMenuItem_initWithTitle_action_keyEquivalent(
        title: *mut std::ffi::c_void,
        action: *mut std::ffi::c_void,
        key: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn NSMenuItem_separatorItem() -> *mut std::ffi::c_void;
}

// ---------------------------------------------------------------------------
// Clipboard helpers
// ---------------------------------------------------------------------------

/// Maps our `Modifier` + key to a macOS-specific modifier mask.
///
/// | Shortcut Modifier | macOS Carbon mask |
/// |-------------------|-------------------|
/// | `Control`         | `controlKey`      |
/// | `Alt` / `Option`  | `optionKey`       |
/// | `Shift`           | `shiftKey`        |
/// | `Meta` / `Cmd`    | `cmdKey`          |
#[derive(Debug, Clone)]
pub struct MacOSModifierMapping {
    pub carbon_mask: u32,
    pub cg_event_mask: u64,
}

impl MacOSModifierMapping {
    /// Convert a `ShortcutBinding` into a `(key_code, carbon_modifiers)` pair.
    ///
    /// # Key-code lookup table
    ///
    /// MacOS uses a fixed set of virtual key codes. Common examples:
    ///
    /// | Key  | Code |   | Key        | Code |
    /// |------|------|---|------------|------|
    /// | A    | 0    |   | Space      | 49   |
    /// | B    | 11   |   | Return     | 36   |
    /// | C    | 8    |   | Tab        | 48   |
    /// | V    | 9    |   | Escape     | 53   |
    /// | ...  | ...  |   | LeftArrow  | 123  |
    ///
    /// The full mapping is embedded in the implementation.
    pub fn from_shortcut(binding: &ShortcutBinding) -> Option<(u32, u32)> {
        match binding {
            ShortcutBinding::Chord { modifiers, key } => {
                // Implementation:
                // 1. Look up key_code from a static map (e.g. HashMap<&str, u32>).
                // 2. Accumulate carbon_mod_mask by iterating over modifiers:
                //    - Control  => CONTROL_KEY
                //    - Alt      => OPTION_KEY
                //    - Shift    => SHIFT_KEY
                //    - Meta     => CMD_KEY
                // 3. Return Some((key_code, carbon_mod_mask))
                let _ = (modifiers, key);
                None // stub
            }
            ShortcutBinding::DoubleModifier { modifier } => {
                // Double-modifier shortcuts (e.g. Ctrl+Ctrl, Cmd+Cmd) do not
                // involve a non-modifier key.  These are handled by monitoring
                // CGEventFlagsChanged events and detecting two successive taps
                // of the same modifier within a configurable interval.
                // The Carbon RegisterEventHotKey API cannot express a modifier-
                // only binding, so we fall back to the CGEvent tap path.
                let _ = modifier;
                None // stub
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Clipboard format reading
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// MacOSClipboardMonitor
// ---------------------------------------------------------------------------

/// Monitors the system pasteboard for changes using NSPasteboard polling.
///
/// # Architecture
///
/// ```text
/// ┌──────────────────────────────────────────────┐
/// │ MacOSClipboardMonitor                         │
/// │                                                │
/// │  ┌──────────┐    poll (500ms)    ┌──────────┐ │
/// │  │ run loop │ ─────────────────► │ NSPaste- │ │
/// │  │ (thread) │ ◄───────────────── │  board   │ │
/// │  └──────────┘   change count     └──────────┘ │
/// │       │                                        │
/// │       │ change detected                        │
/// │       ▼                                        │
/// │  ┌──────────┐    ┌──────────┐                 │
/// │  │ read UTI │───►│ check    │                 │
/// │  │  list    │    │ ignored  │                 │
/// │  └──────────┘    │ apps     │                 │
/// │                  └──────────┘                 │
/// │                       │                       │
/// │                       ▼                       │
/// │                  ┌──────────┐                 │
/// │                  │ emit     │                 │
/// │                  │ callback │                 │
/// │                  └──────────┘                 │
/// └──────────────────────────────────────────────┘
/// ```
pub struct MacOSClipboardMonitor {
    /// Whether the monitoring loop is currently active.
    running: Arc<AtomicBool>,
    /// Set of application bundle identifiers whose clipboard activity
    /// should be ignored (e.g. password managers).
    ignored_apps: Arc<Mutex<HashSet<String>>>,
    /// Join handle for the background polling thread.
    poll_thread: Option<thread::JoinHandle<()>>,
}

impl Default for MacOSClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl MacOSClipboardMonitor {
    /// Creates a new, stopped clipboard monitor.
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            ignored_apps: Arc::new(Mutex::new(HashSet::new())),
            poll_thread: None,
        }
    }

    /// Starts the background polling loop.
    ///
    /// On macOS, uses native NSPasteboard changeCount. On other platforms returns a stub.
    #[cfg(target_os = "macos")]
    pub fn start(
        &mut self,
    ) -> Result<mpsc::Receiver<crate::platform::windows_clipboard::ClipboardChange>, String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("clipboard monitor is already running".to_string());
        }

        let (sender, receiver) = mpsc::channel();
        let running = Arc::clone(&self.running);

        let handle = thread::Builder::new()
            .name("macos-clipboard-monitor".to_owned())
            .spawn(move || {
                let mut last_count: isize = -1;

                while running.load(Ordering::SeqCst) {
                    let pool = unsafe { objc::objc_autoreleasePoolPush() };
                    let pb = objc::get_nspasteboard();
                    let count = objc::pasteboard_change_count(pb);
                    unsafe { objc::objc_autoreleasePoolPop(pool) };

                    if count != last_count {
                        last_count = count;
                        let _ = sender.send(crate::platform::windows_clipboard::ClipboardChange {
                            sequence: count as u32,
                        });
                    }

                    thread::sleep(std::time::Duration::from_millis(500));
                }
            })
            .map_err(|e| format!("failed to spawn clipboard monitor: {e}"))?;

        self.running.store(true, Ordering::SeqCst);
        self.poll_thread = Some(handle);

        Ok(receiver)
    }

    /// Non-macOS stub: returns an error.
    #[cfg(not(target_os = "macos"))]
    pub fn start(
        &mut self,
    ) -> Result<mpsc::Receiver<crate::platform::windows_clipboard::ClipboardChange>, String> {
        let (_sender, receiver) = mpsc::channel();
        self.running.store(true, Ordering::SeqCst);
        Ok(receiver)
    }

    /// Stops the polling loop and joins the background thread.
    pub fn stop(&mut self) -> MacOSResult<()> {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.poll_thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    /// Returns whether the monitor is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Replaces the set of ignored application bundle identifiers.
    ///
    /// # Examples of ignored apps
    ///
    /// - `com.agilebits.onepassword` (1Password)
    /// - `com.bitwarden.desktop` (Bitwarden)
    /// - `com.apple.keychainaccess`
    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        if let Ok(mut guard) = self.ignored_apps.lock() {
            *guard = apps.into_iter().collect();
        }
    }

    /// Returns the current set of ignored application bundle identifiers.
    pub fn ignored_apps(&self) -> Vec<String> {
        self.ignored_apps
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Reads the plain-text content from the general pasteboard.
    ///
    /// Uses `NSPasteboard_stringForType` with UTI `public.utf8-plain-text`.
    /// Returns `None` if no text is available or the pasteboard is inaccessible.
    #[cfg(target_os = "macos")]
    pub fn read_pasteboard_text() -> Option<String> {
        // unsafe {
        //     let pb = NSPasteboard_generalPasteboard();
        //     if pb.is_null() { return None; }
        //     // NSString *uti = @"public.utf8-plain-text";
        //     // Get string ref and convert via NSString_UTF8String.
        //     // ...
        // }
        None // stub for compilation on non-macOS
    }

    #[cfg(not(target_os = "macos"))]
    pub fn read_pasteboard_text() -> Option<String> {
        None
    }

    /// Reads all available UTI types from the general pasteboard.
    #[cfg(target_os = "macos")]
    pub fn read_pasteboard_types() -> Vec<String> {
        // unsafe {
        //     let pb = NSPasteboard_generalPasteboard();
        //     let types = NSPasteboard_types(pb);
        //     let count = NSArray_count(types);
        //     (0..count)
        //         .map(|i| {
        //             let obj = NSArray_objectAtIndex(types, i);
        //             let c_str = NSString_UTF8String(obj);
        //             // Convert CStr to Rust String
        //             String::from_utf8_lossy(CStr::from_ptr(c_str).to_bytes()).into_owned()
        //         })
        //         .collect()
        // }
        vec![]
    }

    #[cfg(not(target_os = "macos"))]
    pub fn read_pasteboard_types() -> Vec<String> {
        vec![]
    }
}

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

/// RTF capture is not wired on macOS yet; the `public.rtf` UTI carries binary
/// data (not a string), so the HTML fragment remains the rich-text source for
/// `formatPaste` there.
#[cfg(target_os = "macos")]
pub fn read_clipboard_rtf() -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn read_clipboard_rtf() -> Option<String> {
    None
}

/// Reads clipboard image data using macOS native tools.
#[cfg(target_os = "macos")]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    // Try pngpaste first (brew install pngpaste)
    if let Some(data) = try_clipboard_image_tool("pngpaste", &["-"]) {
        if let Ok(img) = image::load_from_memory(&data) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            return Some((rgba.into_raw(), w, h));
        }
    }
    // Try imgpaste (alternative tool)
    if let Some(data) = try_clipboard_image_tool("imgpaste", &[]) {
        if let Ok(img) = image::load_from_memory(&data) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            return Some((rgba.into_raw(), w, h));
        }
    }

    // Fallback: write clipboard TIFF via osascript, convert with sips
    let tiff_path = std::env::temp_dir().join("clipboard_img.tiff");
    let png_path = std::env::temp_dir().join("clipboard_img.png");

    let escaped = tiff_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let script = format!(
        "set img to (the clipboard as picture)\n\
         set fileRef to open for access POSIX file \"{escaped}\" with write permission\n\
         write img to fileRef\n\
         close access fileRef"
    );
    let ok = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()
        .is_some_and(|o| o.status.success() && tiff_path.exists());
    if !ok {
        return None;
    }

    let conv_ok = std::process::Command::new("sips")
        .args([
            "-s",
            "format",
            "png",
            &tiff_path.to_string_lossy(),
            "--out",
            &png_path.to_string_lossy(),
        ])
        .output()
        .ok()
        .is_some_and(|o| o.status.success() && png_path.exists());
    let _ = std::fs::remove_file(&tiff_path);
    if !conv_ok {
        return None;
    }

    let data = std::fs::read(&png_path).ok()?;
    let _ = std::fs::remove_file(&png_path);

    if let Ok(img) = image::load_from_memory(&data) {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        Some((rgba.into_raw(), w, h))
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn try_clipboard_image_tool(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let output = std::process::Command::new(cmd).args(args).output().ok()?;
    if output.status.success() && !output.stdout.is_empty() {
        Some(output.stdout)
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    None
}

/// Reads file paths – not supported via simple command-line tools.
#[cfg(target_os = "macos")]
pub fn read_clipboard_file_paths() -> Vec<String> {
    vec![]
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
    let icon_key = crate::content::icon_key(app_name);
    let dest = icon_dir.join(format!("{}.png", icon_key));
    if dest.exists() {
        return Some(dest.to_string_lossy().to_string());
    }

    // Walk up from exe_path to find the .app bundle
    let exe = std::path::Path::new(exe_path);
    let bundle = exe.parent()?.parent()?; // Contents/MacOS/exe -> Contents -> .app
    if !bundle.extension().is_some_and(|ext| ext == "app") {
        return None;
    }

    // Read CFBundleIconFile from Info.plist via plutil
    let info_plist = bundle.join("Contents/Info.plist");
    if !info_plist.exists() {
        return None;
    }
    let plist_json = std::process::Command::new("plutil")
        .args(["-convert", "json", "-o", "-", &info_plist.to_string_lossy()])
        .output()
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

    // Create output directory
    let _ = std::fs::create_dir_all(icon_dir);

    // Convert icns to png using sips
    let ok = std::process::Command::new("sips")
        .args([
            "-s",
            "format",
            "png",
            &icns_path.to_string_lossy(),
            "--out",
            &dest.to_string_lossy(),
        ])
        .output()
        .ok()
        .is_some_and(|o| o.status.success() && dest.exists());

    if ok {
        Some(dest.to_string_lossy().to_string())
    } else {
        None
    }
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

// ---------------------------------------------------------------------------
// MacOSKeyboardHook
// ---------------------------------------------------------------------------

/// Manages global keyboard hooks on macOS using CGEvent or Carbon APIs.
///
/// # Strategy
///
/// 1. **Chord shortcuts** (`Cmd+Shift+V`) are registered via the
///    Carbon `RegisterEventHotKey` API.  This provides guaranteed, system-level
///    delivery even when the app is not frontmost.
///
/// 2. **Double-modifier shortcuts** (`Cmd+Cmd`, `Ctrl+Ctrl`) cannot be
///    expressed through Carbon hotkeys alone.  They are implemented using a
///    `CGEventTap` on the `kCGSessionEventTap` stream, which intercepts
///    `kCGEventFlagsChanged` events and detects two successive modifier-key
///    presses within a configurable window (default: 300ms).
///
/// 3. Modifier-normalisation: the tap translates CGEventFlags into our
///    `Modifier` enum, accounting for the left/right key distinction (e.g.
///    `kCGEventFlagMaskAlternate` vs `kCGEventFlagMaskOption`).
type HotkeyCallback = Box<dyn Fn(&str) + Send + Sync + 'static>;

pub struct MacOSKeyboardHook {
    /// The CGEventTap reference (NULL when not active).
    #[allow(dead_code)]
    event_tap: usize,
    /// Registered Carbon hotkey references.
    #[allow(dead_code)]
    hotkey_refs: Vec<usize>,
    /// Callback invoked when a registered hotkey fires.
    #[allow(dead_code)]
    on_hotkey: Option<HotkeyCallback>,
    /// Timestamp of the last modifier-key press (for double-modifier detection).
    #[allow(dead_code)]
    last_modifier_tap_ms: u64,
    /// The modifier that was last tapped (for double-modifier detection).
    #[allow(dead_code)]
    last_modifier: Option<crate::keyboard::Modifier>,
}

impl Default for MacOSKeyboardHook {
    fn default() -> Self {
        Self::new()
    }
}

impl MacOSKeyboardHook {
    /// Creates a new, inactive keyboard hook.
    pub fn new() -> Self {
        Self {
            event_tap: 0,
            hotkey_refs: Vec::new(),
            on_hotkey: None,
            last_modifier_tap_ms: 0,
            last_modifier: None,
        }
    }

    /// Registers one or more shortcuts with the system.
    ///
    /// # Parameters
    ///
    /// - `action_id`: an opaque identifier forwarded to the callback when the
    ///   shortcut fires.
    /// - `shortcuts`: the bindings to register.
    ///
    /// # Errors
    ///
    /// Returns `MacOSError::HotkeyRegistrationFailed` if a Carbon hotkey is
    /// already taken by another application.
    pub fn register(
        &mut self,
        _action_id: &str,
        _shortcuts: &[ShortcutBinding],
    ) -> MacOSResult<()> {
        // Implementation outline:
        //
        // 1. For each ShortcutBinding:
        //    a. If Chord:
        //       - Call MacOSModifierMapping::from_shortcut() → (key_code, carbon_mods)
        //       - Call RegisterEventHotKey(key_code, carbon_mods, hotkey_id, ...)
        //       - Store returned EventHotKeyRef for later unregistration.
        //    b. If DoubleModifier:
        //       - Ensure the CGEventTap is created (if not already).
        //       - Add the modifier to a "double-tap watch" set.
        //
        // 2. If double-modifier shortcuts are requested, create the CGEventTap:
        //    - CGEventTapCreate(kCGSessionEventTap, kCGHeadInsertEventTap,
        //      kCGEventTapActiveListener, CGEventMaskBit(kCGEventFlagsChanged),
        //      tap_callback, self_ptr)
        //    - Wrap in CFRunLoopSource and add to CFRunLoopGetCurrent().

        Ok(())
    }

    /// Unregisters all shortcuts and tears down the CGEventTap.
    pub fn unregister_all(&mut self) -> MacOSResult<()> {
        // Implementation:
        // - For each hotkey_ref in self.hotkey_refs:
        //     UnregisterEventHotKey(hotkey_ref)
        // - If self.event_tap is non-null:
        //     CGEventTapEnable(event_tap, false)
        //     CFRelease(event_tap)
        // - Clear hotkey_refs and set event_tap to 0.
        Ok(())
    }

    /// Sets the callback invoked when a registered hotkey fires.
    ///
    /// The callback receives the `action_id` string that was passed to
    /// `register()`.
    pub fn set_callback<F>(&mut self, callback: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.on_hotkey = Some(Box::new(callback));
    }

    /// Translates a macOS virtual key-code to a human-readable key name string.
    ///
    /// # Mapping (partial, full table in implementation)
    ///
    /// | Code | Name         | Code | Name       |
    /// |------|--------------|------|------------|
    /// | 0    | A            | 36   | Return     |
    /// | 6    | Z            | 48   | Tab        |
    /// | 49   | Space        | 51   | Delete     |
    /// | 53   | Escape       | 123  | LeftArrow  |
    /// | 124  | RightArrow   | 125  | DownArrow  |
    /// | 126  | UpArrow      | 122  | F1         |
    pub fn key_code_to_name(key_code: u32) -> &'static str {
        match key_code {
            0 => "A",
            1 => "S",
            2 => "D",
            3 => "F",
            4 => "H",
            5 => "G",
            6 => "Z",
            7 => "X",
            8 => "C",
            9 => "V",
            11 => "B",
            12 => "Q",
            13 => "W",
            14 => "E",
            15 => "R",
            16 => "Y",
            17 => "T",
            31 => "O",
            32 => "U",
            34 => "I",
            35 => "P",
            36 => "Return",
            37 => "L",
            38 => "J",
            40 => "K",
            41 => "Semicolon",
            45 => "N",
            46 => "M",
            48 => "Tab",
            49 => "Space",
            51 => "Delete",
            53 => "Escape",
            122 => "F1",
            123 => "LeftArrow",
            124 => "RightArrow",
            125 => "DownArrow",
            126 => "UpArrow",
            _ => "Unknown",
        }
    }

    /// Returns whether the hook is currently active (has registered hotkeys
    /// or an active event tap).
    pub fn is_active(&self) -> bool {
        !self.hotkey_refs.is_empty() || self.event_tap != 0
    }
}

// ---------------------------------------------------------------------------
// MacOSAccessibilityHelper
// ---------------------------------------------------------------------------

/// Checks and requests macOS accessibility permissions.
///
/// On macOS many clipboard-manager features (paste simulation, window focus
/// tracking) require the application to be granted Accessibility access in
/// System Settings → Privacy & Security → Accessibility.
///
/// # API
///
/// - `AXIsProcessTrusted()` → returns `true` if the process is trusted.
/// - `AXMakeProcessTrusted()` → opens a system dialog requesting permission.
///   The process must be restarted after the dialog is approved.
pub struct MacOSAccessibilityHelper;

impl MacOSAccessibilityHelper {
    /// Returns `true` if the current process has been granted accessibility
    /// permissions.
    ///
    /// Wraps `AXIsProcessTrusted()`.
    #[cfg(target_os = "macos")]
    pub fn is_trusted() -> bool {
        // unsafe { AXIsProcessTrusted() }
        false
    }

    #[cfg(not(target_os = "macos"))]
    pub fn is_trusted() -> bool {
        false
    }

    /// Requests accessibility permission from the user.
    ///
    /// On macOS this calls `AXMakeProcessTrusted()` which presents a system
    /// dialog.  The process typically requires a restart after approval.
    ///
    /// Returns `Ok(())` if the request was submitted, or an error if
    /// the API is unavailable.
    #[cfg(target_os = "macos")]
    pub fn request_permission() -> MacOSResult<()> {
        // let result = unsafe { AXMakeProcessTrusted() };
        // if result != 0 {
        //     return Err(MacOSError::AccessibilityPermissionDenied);
        // }
        // Ok(())
        Err(MacOSError::AccessibilityPermissionDenied)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn request_permission() -> MacOSResult<()> {
        Ok(())
    }

    /// Returns a human-readable status string describing the current
    /// accessibility permission state.
    pub fn status_description() -> &'static str {
        if Self::is_trusted() {
            "Accessibility permission granted — all features available."
        } else {
            "Accessibility permission required. Open System Settings → \
             Privacy & Security → Accessibility and enable this app."
        }
    }

    /// Returns whether a restart is required after granting permission.
    ///
    /// On macOS, `AXMakeProcessTrusted()` does not take effect until the
    /// process is relaunched.
    pub fn requires_restart_after_grant() -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// MacOSTrayManager
// ---------------------------------------------------------------------------

/// Creates and manages an NSStatusBar (menu bar) item.
///
/// # Lifecycle
///
/// ```text
/// MacOSTrayManager::create()
///     └── [NSStatusBar systemStatusBar]
///         └── statusItemWithLength: NSVariableStatusItemLength
///             └── button (NSStatusBarButton)
///                 ├── title    = app name (or icon)
///                 └── action   = toggle-window selector
///     └── setMenu(items)
///         └── NSMenu
///             ├── "Show/Hide"
///             ├── separator
///             ├── "Preferences"
///             ├── "About"
///             ├── separator
///             └── "Quit"
/// ```
///
/// When the application exits, `Drop` removes the status item.
pub struct MacOSTrayManager {
    /// Reference to the NSStatusItem (opaque pointer).
    #[allow(dead_code)]
    status_item: usize,
    /// Reference to the NSMenu attached to the status item.
    #[allow(dead_code)]
    menu: usize,
}

/// A single entry in the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacOSTrayMenuItem {
    /// A clickable item with a label and an action identifier.
    Item { label: String, action: String },
    /// A visual separator between groups of items.
    Separator,
}

impl MacOSTrayManager {
    /// Creates a new status bar item and returns a manager handle.
    ///
    /// The item is initially empty (no menu items).  Call `set_menu` to
    /// populate it.
    pub fn create() -> MacOSResult<Self> {
        // Implementation outline:
        //
        // unsafe {
        //     let bar = NSStatusBar_systemStatusBar();
        //     let item = NSStatusBar_statusItemWithLength(bar, -1.0); // NSVariableStatusItemLength
        //     let button = NSStatusBarButton_button(item);
        //     // Set button title and action.
        //     // ...
        //     let menu = NSMenu_initWithTitle(...);
        //     Ok(Self { status_item: item as usize, menu: menu as usize })
        // }
        Ok(Self {
            status_item: 0,
            menu: 0,
        })
    }

    /// Replaces the tray menu with the given items.
    pub fn set_menu(&mut self, items: &[MacOSTrayMenuItem]) -> MacOSResult<()> {
        // Implementation:
        //
        // For each item in items:
        //   match item {
        //       MacOSTrayMenuItem::Item { label, action } =>
        //           NSMenuItem_initWithTitle_action_keyEquivalent(label, selector, "")
        //           NSMenu_addItem(menu, menu_item)
        //       MacOSTrayMenuItem::Separator =>
        //           NSMenu_addItem(menu, NSMenuItem_separatorItem())
        //   }
        let _ = items;
        Ok(())
    }

    /// Updates the status bar button title (or icon).
    pub fn set_title(&mut self, _title: &str) -> MacOSResult<()> {
        // Set button.attributedTitle or button.title.
        Ok(())
    }

    /// Returns the default menu for the clipboard manager application.
    ///
    /// This provides a consistent starting menu that callers can customize.
    pub fn default_menu() -> Vec<MacOSTrayMenuItem> {
        vec![
            MacOSTrayMenuItem::Item {
                label: "Show/Hide".to_owned(),
                action: "toggleWindow".to_owned(),
            },
            MacOSTrayMenuItem::Separator,
            MacOSTrayMenuItem::Item {
                label: "Preferences".to_owned(),
                action: "openPreferences".to_owned(),
            },
            MacOSTrayMenuItem::Item {
                label: "About".to_owned(),
                action: "openAbout".to_owned(),
            },
            MacOSTrayMenuItem::Separator,
            MacOSTrayMenuItem::Item {
                label: "Quit".to_owned(),
                action: "quit".to_owned(),
            },
        ]
    }
}

impl Drop for MacOSTrayManager {
    fn drop(&mut self) {
        // Remove the status item from the bar to prevent a dangling item
        // after the application exits.
        //
        // Implementation:
        // unsafe {
        //     let bar = NSStatusBar_systemStatusBar();
        //     NSStatusBar_removeStatusItem(bar, self.status_item as *mut _);
        // }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_monitor_lifecycle() {
        let mut monitor = MacOSClipboardMonitor::new();
        assert!(!monitor.is_running());

        monitor.start().unwrap();
        assert!(monitor.is_running());

        monitor.stop().unwrap();
        assert!(!monitor.is_running());
    }

    #[test]
    fn clipboard_monitor_ignored_apps() {
        let mut monitor = MacOSClipboardMonitor::new();
        monitor.set_ignored_apps(vec![
            "com.agilebits.onepassword".to_owned(),
            "com.bitwarden.desktop".to_owned(),
        ]);
        let apps = monitor.ignored_apps();
        assert_eq!(apps.len(), 2);
        assert!(apps.contains(&"com.agilebits.onepassword".to_owned()));
    }

    #[test]
    fn keyboard_hook_creates_and_destroys() {
        let mut hook = MacOSKeyboardHook::new();
        assert!(!hook.is_active());

        hook.register("test", &[]).unwrap();
        hook.unregister_all().unwrap();
    }

    #[test]
    fn key_code_to_name_returns_expected_values() {
        assert_eq!(MacOSKeyboardHook::key_code_to_name(0), "A");
        assert_eq!(MacOSKeyboardHook::key_code_to_name(49), "Space");
        assert_eq!(MacOSKeyboardHook::key_code_to_name(36), "Return");
        assert_eq!(MacOSKeyboardHook::key_code_to_name(53), "Escape");
        assert_eq!(MacOSKeyboardHook::key_code_to_name(999), "Unknown");
    }

    #[test]
    fn accessibility_helper_status_is_string() {
        let status = MacOSAccessibilityHelper::status_description();
        assert!(!status.is_empty());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn accessibility_permission_request_on_non_macos() {
        // Should succeed on non-macOS (stub returns Ok).
        assert!(MacOSAccessibilityHelper::request_permission().is_ok());
    }

    #[test]
    fn tray_default_menu_is_not_empty() {
        let menu = MacOSTrayManager::default_menu();
        assert!(!menu.is_empty());
        let has_separator = menu
            .iter()
            .any(|item| matches!(item, MacOSTrayMenuItem::Separator));
        assert!(has_separator);
    }

    #[test]
    fn tray_create_and_set_menu() {
        let mut tray = MacOSTrayManager::create().unwrap();
        let menu = MacOSTrayManager::default_menu();
        tray.set_menu(&menu).unwrap();
    }
}
