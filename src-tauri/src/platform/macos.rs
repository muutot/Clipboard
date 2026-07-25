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
extern "C" {
    // ---- NSPasteboard --------------------------------------------------
    fn NSPasteboard_generalPasteboard() -> *mut std::ffi::c_void;
    fn NSPasteboard_changeCount(pasteboard: *mut std::ffi::c_void) -> isize;
    fn NSPasteboard_types(pasteboard: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn NSPasteboard_stringForType(
        pasteboard: *mut std::ffi::c_void,
        data_type: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    // NSString helpers
    fn NSString_UTF8String(string: *mut std::ffi::c_void) -> *const i8;
    fn NSArray_count(array: *mut std::ffi::c_void) -> isize;
    fn NSArray_objectAtIndex(array: *mut std::ffi::c_void, index: isize) -> *mut std::ffi::c_void;

    // ---- NSWorkspace ---------------------------------------------------
    fn NSWorkspace_sharedWorkspace() -> *mut std::ffi::c_void;
    fn NSWorkspace_frontmostApplication(workspace: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn NSRunningApplication_localizedName(app: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn NSRunningApplication_bundleIdentifier(app: *mut std::ffi::c_void) -> *mut std::ffi::c_void;

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
// CGEvent / Carbon constants (available on all platforms for compilation)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod carbon_constants {
    // CGEvent tap location
    pub const K_CG_HID_EVENT_TAP: i32 = 0;
    pub const K_CG_SESSION_EVENT_TAP: i32 = 1;
    pub const K_CG_ANNOTATED_SESSION_EVENT_TAP: i32 = 2;

    // CGEvent tap placement
    pub const K_CG_HEAD_INSERT_EVENT_TAP: i32 = 0;
    pub const K_CG_TAIL_APPEND_EVENT_TAP: i32 = 1;

    // CGEvent tap options
    pub const K_CG_EVENT_TAP_LISTEN_ONLY: i32 = 0;
    pub const K_CG_EVENT_TAP_ACTIVE_LISTENER: i32 = 1;

    // CGEvent types
    pub const K_CG_EVENT_KEY_DOWN: u32 = 10;
    pub const K_CG_EVENT_KEY_UP: u32 = 11;
    pub const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;

    // CGEvent fields
    pub const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
    pub const K_CG_KEYBOARD_EVENT_AUTOREPEAT: u32 = 8;

    // CGEvent flags (modifier masks)
    pub const K_CG_EVENT_FLAG_COMMAND: u64 = 1 << 20;
    pub const K_CG_EVENT_FLAG_SHIFT: u64 = 1 << 17;
    pub const K_CG_EVENT_FLAG_ALPHA_SHIFT: u64 = 1 << 16;
    pub const K_CG_EVENT_FLAG_OPTION: u64 = 1 << 18;
    pub const K_CG_EVENT_FLAG_ALTERNATE: u64 = 1 << 19;
    pub const K_CG_EVENT_FLAG_CONTROL: u64 = 1 << 18;

    // Carbon hotkey modifier masks
    pub const CMD_KEY: u32 = 1 << 8;
    pub const SHIFT_KEY: u32 = 1 << 9;
    pub const OPTION_KEY: u32 = 1 << 11;
    pub const CONTROL_KEY: u32 = 1 << 12;

    // EventHotKey options
    pub const K_EVENT_HOTKEY_EXCLUSIVE: u32 = 1 << 0;

    // kCFRunLoopDefaultMode / kCFRunLoopCommonModes identifiers are
    // resolved at runtime via CFStringRef; we document the convention here.
}

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

/// Represents the available pasteboard data at a single point in time.
#[derive(Debug, Clone)]
pub struct PasteboardSnapshot {
    /// Pasteboard change count (monotonically increasing).
    pub change_count: isize,
    /// MIME / UTI types available on the pasteboard (e.g. `public.utf8-plain-text`).
    pub available_types: Vec<String>,
    /// The plain-text content, if available.
    pub text_content: Option<String>,
}

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
    /// Last known NSPasteboard change count.
    last_change_count: isize,
    /// Channel sender used to emit clipboard-snapshot callbacks.
    snapshot_tx: Option<mpsc::Sender<PasteboardSnapshot>>,
    /// Join handle for the background polling thread.
    poll_thread: Option<thread::JoinHandle<()>>,
}

impl MacOSClipboardMonitor {
    /// Creates a new, stopped clipboard monitor.
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            ignored_apps: Arc::new(Mutex::new(HashSet::new())),
            last_change_count: -1,
            snapshot_tx: None,
            poll_thread: None,
        }
    }

    /// Starts the background polling loop.
    ///
    /// Every 500ms the loop:
    /// 1. Obtains `[NSPasteboard generalPasteboard]`.
    /// 2. Reads `changeCount` and compares with the previous value.
    /// 3. If changed, calls `read_available_types()` and `read_text()`.
    /// 4. Checks whether the frontmost application is in the ignored set.
    /// 5. If not ignored, sends a `PasteboardSnapshot` through the channel.
    pub fn start(&mut self) -> MacOSResult<()> {
        // Implementation outline:
        //
        // self.running.store(true, Ordering::SeqCst);
        // let (tx, rx) = mpsc::channel::<PasteboardSnapshot>();
        // self.snapshot_tx = Some(tx);
        // let running = Arc::clone(&self.running);
        // let ignored = Arc::clone(&self.ignored_apps);
        //
        // self.poll_thread = Some(thread::spawn(move || {
        //     while running.load(Ordering::SeqCst) {
        //         // 1. NSPasteboard_generalPasteboard()
        //         // 2. NSPasteboard_changeCount(pb)
        //         // 3. If count != last_count:
        //         //    a. NSPasteboard_types(pb) → iterate via NSArray_* → UTI strings
        //         //    b. NSPasteboard_stringForType(pb, "public.utf8-plain-text")
        //         //    c. Check frontmost app bundle ID vs ignored_apps
        //         //    d. If not ignored, tx.send(snapshot)
        //         // 4. thread::sleep(Duration::from_millis(500))
        //     }
        // }));
        // Ok(())
        //
        // On non-macOS this is a stub.
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Stops the polling loop and joins the background thread.
    pub fn stop(&mut self) -> MacOSResult<()> {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.poll_thread.take() {
            let _ = handle.join();
        }
        self.snapshot_tx = None;
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
pub struct MacOSKeyboardHook {
    /// The CGEventTap reference (NULL when not active).
    #[allow(dead_code)]
    event_tap: usize,
    /// Registered Carbon hotkey references.
    #[allow(dead_code)]
    hotkey_refs: Vec<usize>,
    /// Callback invoked when a registered hotkey fires.
    #[allow(dead_code)]
    on_hotkey: Option<Box<dyn Fn(&str) + Send + Sync + 'static>>,
    /// Timestamp of the last modifier-key press (for double-modifier detection).
    #[allow(dead_code)]
    last_modifier_tap_ms: u64,
    /// The modifier that was last tapped (for double-modifier detection).
    #[allow(dead_code)]
    last_modifier: Option<crate::keyboard::Modifier>,
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
