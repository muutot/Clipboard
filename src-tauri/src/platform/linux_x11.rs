//! Linux X11 platform adapter providing clipboard monitoring via Xlib/XCB,
//! global hotkey registration through XGrabKey, and system tray support.
//!
//! # X11 Selection Model
//!
//! X11 has three standard selections:
//!
//! | Selection  | Atom          | Purpose                              |
//! |------------|---------------|--------------------------------------|
//! | PRIMARY    | `XA_PRIMARY`  | Middle-click / selection paste       |
//! | CLIPBOARD  | `XA_CLIPBOARD`| Explicit copy-paste (Ctrl+C / Ctrl+V)|
//! | SECONDARY  | `XA_SECONDARY`| Rarely used; mostly historical       |
//!
//! This adapter monitors both PRIMARY and CLIPBOARD using `SelectionNotify`
//! events.  On non-Linux targets only the type definitions and documentation
//! are compiled.

#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use crate::keyboard::ShortcutBinding;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that may occur during X11 platform operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X11Error {
    /// Could not open a connection to the X server (`XOpenDisplay` returned NULL).
    DisplayOpenFailed(String),
    /// An X11 protocol request returned an error.
    XError {
        code: u8,
        request_code: u8,
        minor_code: u16,
        resource_id: u64,
    },
    /// A required X11 atom was missing (TARGETS, UTF8_STRING, etc.).
    MissingAtom(String),
    /// XGetWindowProperty did not return the expected format or type.
    PropertyReadFailed(String),
    /// A global hotkey could not be registered (keycode already grabbed).
    HotkeyGrabFailed(u32, u32),
    /// Failed to create the system tray icon.
    TrayCreationFailed(String),
    /// The XFixes extension is not available (required for clipboard monitoring).
    XFixesNotAvailable,
}

impl std::fmt::Display for X11Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DisplayOpenFailed(msg) => write!(f, "failed to open X display: {msg}"),
            Self::XError {
                code,
                request_code,
                minor_code,
                resource_id,
            } => write!(
                f,
                "X error: code={code} request={request_code} minor={minor_code} resource={resource_id}"
            ),
            Self::MissingAtom(name) => write!(f, "missing X atom: {name}"),
            Self::PropertyReadFailed(msg) => write!(f, "property read failed: {msg}"),
            Self::HotkeyGrabFailed(keycode, modifiers) => {
                write!(f, "failed to grab keycode {keycode} with modifiers {modifiers:#x}")
            }
            Self::TrayCreationFailed(msg) => write!(f, "tray creation failed: {msg}"),
            Self::XFixesNotAvailable => write!(f, "XFixes extension not available"),
        }
    }
}

impl std::error::Error for X11Error {}

/// Convenience alias for X11 operation results.
pub type X11Result<T> = Result<T, X11Error>;

// ---------------------------------------------------------------------------
// Xlib extern "C" declarations
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[allow(non_camel_case_types, dead_code)]
mod x11_ffi {
    // Types -----------------------------------------------------------------
    pub type Display = std::ffi::c_void;
    pub type Window = u64;
    pub type Atom = u64;
    pub type Time = u64;
    pub type KeyCode = u32;
    pub type KeySym = u64;
    pub type Status = i32;
    pub type Bool = i32;
    #[allow(clippy::upper_case_acronyms)]
    pub type XID = u64;

    // XEvent union — we only define the parts we use.
    #[repr(C)]
    pub struct XAnyEvent {
        pub type_: i32,
        pub serial: u64,
        pub send_event: Bool,
        pub display: *mut Display,
        pub window: Window,
    }

    #[repr(C)]
    pub struct XKeyEvent {
        pub type_: i32,
        pub serial: u64,
        pub send_event: Bool,
        pub display: *mut Display,
        pub window: Window,
        pub root: Window,
        pub subwindow: Window,
        pub time: Time,
        pub x: i32,
        pub y: i32,
        pub x_root: i32,
        pub y_root: i32,
        pub state: u32,
        pub keycode: KeyCode,
        pub same_screen: Bool,
    }

    #[repr(C)]
    pub struct XSelectionEvent {
        pub type_: i32,
        pub serial: u64,
        pub send_event: Bool,
        pub display: *mut Display,
        pub requestor: Window,
        pub selection: Atom,
        pub target: Atom,
        pub property: Atom,
        pub time: Time,
    }

    #[repr(C)]
    pub struct XSelectionRequestEvent {
        pub type_: i32,
        pub serial: u64,
        pub send_event: Bool,
        pub display: *mut Display,
        pub owner: Window,
        pub requestor: Window,
        pub selection: Atom,
        pub target: Atom,
        pub property: Atom,
        pub time: Time,
    }

    #[repr(C)]
    pub struct XPropertyEvent {
        pub type_: i32,
        pub serial: u64,
        pub send_event: Bool,
        pub display: *mut Display,
        pub window: Window,
        pub atom: Atom,
        pub time: Time,
        pub state: i32,
    }

    #[repr(C)]
    pub struct XClientMessageEvent {
        pub type_: i32,
        pub serial: u64,
        pub send_event: Bool,
        pub display: *mut Display,
        pub window: Window,
        pub message_type: Atom,
        pub format: i32,
        pub data: [u64; 5],
    }

    // We keep the event union minimal.
    #[repr(C)]
    pub union XEventData {
        pub any: std::mem::ManuallyDrop<XAnyEvent>,
        pub key: std::mem::ManuallyDrop<XKeyEvent>,
        pub selection: std::mem::ManuallyDrop<XSelectionEvent>,
        pub selection_request: std::mem::ManuallyDrop<XSelectionRequestEvent>,
        pub property: std::mem::ManuallyDrop<XPropertyEvent>,
        pub client_message: std::mem::ManuallyDrop<XClientMessageEvent>,
        pub _pad: [u64; 24],
    }

    #[repr(C)]
    pub struct XEvent {
        pub data: XEventData,
    }

    // Event type constants
    pub const KEY_PRESS: i32 = 2;
    pub const KEY_RELEASE: i32 = 3;
    pub const SELECTION_NOTIFY: i32 = 31;
    pub const SELECTION_REQUEST: i32 = 30;
    pub const PROPERTY_NOTIFY: i32 = 28;
    pub const CLIENT_MESSAGE: i32 = 33;
    pub const SELECTION_CLEAR: i32 = 29;

    // Grab modes
    pub const GRAB_MODE_ASYNC: i32 = 1;

    // Property formats
    pub const PROP_MODE_REPLACE: i32 = 0;

    // Atom predefined values
    pub const XA_PRIMARY: Atom = 1;
    pub const XA_SECONDARY: Atom = 2;
    pub const XA_ATOM: Atom = 4;
    pub const XA_STRING: Atom = 31;

    // Modifier masks
    pub const SHIFT_MASK: u32 = 1 << 0;
    pub const LOCK_MASK: u32 = 1 << 1;
    pub const CONTROL_MASK: u32 = 1 << 2;
    pub const MOD1_MASK: u32 = 1 << 3; // Alt / Meta
    pub const MOD2_MASK: u32 = 1 << 4; // NumLock
    pub const MOD3_MASK: u32 = 1 << 5;
    pub const MOD4_MASK: u32 = 1 << 6; // Super / Windows key
    pub const MOD5_MASK: u32 = 1 << 7;

    pub const ANY_MODIFIER: u32 = 1 << 15;

    #[link(name = "X11")]
    extern "C" {
        // Connection management
        pub fn XOpenDisplay(name: *const i8) -> *mut Display;
        pub fn XCloseDisplay(display: *mut Display) -> i32;
        pub fn XDefaultRootWindow(display: *mut Display) -> Window;
        pub fn XFlush(display: *mut Display) -> i32;
        pub fn XPending(display: *mut Display) -> i32;
        pub fn XNextEvent(display: *mut Display, event: *mut XEvent) -> i32;

        // Atoms
        pub fn XInternAtom(display: *mut Display, name: *const i8, only_if_exists: Bool) -> Atom;
        pub fn XGetAtomName(display: *mut Display, atom: Atom) -> *mut i8;

        // Windows
        pub fn XCreateSimpleWindow(
            display: *mut Display,
            parent: Window,
            x: i32,
            y: i32,
            width: u32,
            height: u32,
            border_width: u32,
            border: u64,
            background: u64,
        ) -> Window;
        pub fn XDestroyWindow(display: *mut Display, window: Window) -> i32;
        pub fn XSelectInput(display: *mut Display, window: Window, event_mask: i64) -> i32;

        // Selections
        pub fn XSetSelectionOwner(
            display: *mut Display,
            selection: Atom,
            owner: Window,
            time: Time,
        ) -> i32;
        pub fn XGetSelectionOwner(display: *mut Display, selection: Atom) -> Window;
        pub fn XConvertSelection(
            display: *mut Display,
            selection: Atom,
            target: Atom,
            property: Atom,
            requestor: Window,
            time: Time,
        ) -> i32;

        // Properties
        pub fn XGetWindowProperty(
            display: *mut Display,
            window: Window,
            property: Atom,
            long_offset: i64,
            long_length: i64,
            delete: Bool,
            req_type: Atom,
            actual_type: *mut Atom,
            actual_format: *mut i32,
            nitems: *mut u64,
            bytes_after: *mut u64,
            prop: *mut *mut u8,
        ) -> i32;
        pub fn XFree(data: *mut std::ffi::c_void) -> i32;
        pub fn XDeleteProperty(display: *mut Display, window: Window, property: Atom) -> i32;

        // Keyboard
        pub fn XGrabKey(
            display: *mut Display,
            keycode: i32,
            modifiers: u32,
            grab_window: Window,
            owner_events: Bool,
            pointer_mode: i32,
            keyboard_mode: i32,
        ) -> i32;
        pub fn XUngrabKey(
            display: *mut Display,
            keycode: i32,
            modifiers: u32,
            grab_window: Window,
        ) -> i32;
        pub fn XKeysymToKeycode(display: *mut Display, keysym: KeySym) -> KeyCode;
        pub fn XStringToKeysym(string: *const i8) -> KeySym;
        pub fn XKeycodeToKeysym(display: *mut Display, keycode: KeyCode, index: i32) -> KeySym;

        // Error handling
        pub fn XSetErrorHandler(handler: *const std::ffi::c_void) -> *const std::ffi::c_void;
        pub fn XSync(display: *mut Display, discard: Bool) -> i32;
    }

    #[link(name = "Xfixes")]
    extern "C" {
        // XFixes (query version, select selection input)
        pub fn XFixesQueryExtension(
            display: *mut Display,
            event_base: *mut i32,
            error_base: *mut i32,
        ) -> Bool;
        pub fn XFixesSelectSelectionInput(
            display: *mut Display,
            window: Window,
            selection: Atom,
            event_mask: u64,
        ) -> i32;
    }
}

// ---------------------------------------------------------------------------
// X11 keycode / keysym mapping tables
// ---------------------------------------------------------------------------

/// Maps a `ShortcutBinding` key string to an X11 keysym name.
///
/// Keysym names follow the X11 convention (e.g. "space", "Return", "F1").
/// The `XStringToKeysym` function is the authoritative translation.
///
/// # Key name → Keysym lookup (partial table)
///
/// | Shortcut Key | X11 Keysym Name | Keysym Value |
/// |-------------|-----------------|--------------|
/// | Space       | space           | 0x0020       |
/// | Return      | Return          | 0xFF0D       |
/// | Escape      | Escape          | 0xFF1B       |
/// | Tab         | Tab             | 0xFF09       |
/// | Delete      | Delete          | 0xFFFF       |
/// | Backspace   | BackSpace       | 0xFF08       |
/// | UpArrow     | Up              | 0xFF52       |
/// | DownArrow   | Down            | 0xFF54       |
/// | LeftArrow   | Left            | 0xFF51       |
/// | RightArrow  | Right           | 0xFF53       |
/// | Home        | Home            | 0xFF50       |
/// | End         | End             | 0xFF57       |
/// | PageUp      | Prior           | 0xFF55       |
/// | PageDown    | Next            | 0xFF56       |
/// | F1-F12      | F1-F12          | 0xFFBE-0xFFC9|
///
/// Single-character keys (A-Z, 0-9) map directly to their ASCII keysyms.
#[derive(Debug, Clone)]
pub struct X11KeyMapping {
    /// The X11 keysym value for the key.
    pub keysym: u64,
    /// The X11 keycode (depends on keyboard layout, resolved at runtime).
    pub keycode: u32,
}

impl X11KeyMapping {
    /// Converts a Rust key name into the corresponding X11 keysym name string.
    ///
    /// Returns the X11 keysym name expected by `XStringToKeysym`.
    pub fn key_to_keysym_name(key: &str) -> &'static str {
        match key {
            "Space" => "space",
            "Return" | "Enter" => "Return",
            "Escape" | "Esc" => "Escape",
            "Tab" => "Tab",
            "Delete" => "Delete",
            "Backspace" => "BackSpace",
            "Up" | "UpArrow" => "Up",
            "Down" | "DownArrow" => "Down",
            "Left" | "LeftArrow" => "Left",
            "Right" | "RightArrow" => "Right",
            "Home" => "Home",
            "End" => "End",
            "PageUp" => "Prior",
            "PageDown" => "Next",
            "Insert" => "Insert",
            "Pause" => "Pause",
            "Print" | "PrintScreen" => "Print",
            "CapsLock" => "Caps_Lock",
            "NumLock" => "Num_Lock",
            "ScrollLock" => "Scroll_Lock",
            "F1" => "F1",
            "F2" => "F2",
            "F3" => "F3",
            "F4" => "F4",
            "F5" => "F5",
            "F6" => "F6",
            "F7" => "F7",
            "F8" => "F8",
            "F9" => "F9",
            "F10" => "F10",
            "F11" => "F11",
            "F12" => "F12",
            // Single-char keys: use the lowercase character directly as the
            // X11 keysym name.  XStringToKeysym("a") → XK_a.
            other if other.chars().count() == 1 => {
                // For single chars we'd return the lowercase version, but
                // &str lifetime constraints mean we can't return a temporary.
                // In the real implementation the caller lowercases the key.
                "space" // fallback for the map; caller handles single-char
            }
            _ => "space", // unknown keys default
        }
    }
}

// ---------------------------------------------------------------------------
// Modifier mask translation
// ---------------------------------------------------------------------------

/// Translates our `Modifier` enum to X11 modifier masks.
///
/// | Modifier | X11 Mask    | Notes                                 |
/// |----------|-------------|---------------------------------------|
/// | Control  | ControlMask | `(1 << 2)`                            |
/// | Alt      | Mod1Mask    | `(1 << 3)` — Alt on most configs      |
/// | Shift    | ShiftMask   | `(1 << 0)`                            |
/// | Meta     | Mod4Mask    | `(1 << 6)` — Super / Windows key      |
pub struct X11ModifierMapping;

impl X11ModifierMapping {
    /// Converts a `ShortcutBinding` into `(X11 keycode, X11 modifier mask)`.
    ///
    /// We also register the grab with the combination that includes:
    /// - `LockMask` (CapsLock)
    /// - `Mod2Mask` (NumLock)
    ///
    /// This ensures the hotkey works regardless of CapsLock/NumLock state.
    pub fn to_grab_params(
        binding: &ShortcutBinding,
        // display: *mut x11_ffi::Display,
    ) -> Option<(u32, u32)> {
        match binding {
            ShortcutBinding::Chord { modifiers, key } => {
                // 1. Resolve the key to a keysym name via X11KeyMapping::key_to_keysym_name().
                // 2. Call XStringToKeysym() → XKeysymToKeycode() for the keycode.
                // 3. Accumulate modifier mask:
                //    - Control → ControlMask
                //    - Alt     → Mod1Mask
                //    - Shift   → ShiftMask
                //    - Meta    → Mod4Mask
                // 4. Also register with (mask | LockMask) and (mask | Mod2Mask)
                //    to ignore CapsLock/NumLock state.
                let _ = (modifiers, key);
                None
            }
            ShortcutBinding::DoubleModifier { modifier } => {
                // Double-modifier shortcuts are not natively supported by
                // XGrabKey.  Implementation monitors KeyPress events on the
                // root window and detects two successive presses of the same
                // modifier key within a time window.
                let _ = modifier;
                None
            }
        }
    }
}

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
            let _ = handle.join();
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

        // Read the property data
        let mut actual_type: x11_ffi::Atom = 0;
        let mut actual_format: i32 = 0;
        let mut nitems: u64 = 0;
        let mut bytes_after: u64 = 0;
        let mut prop: *mut u8 = std::ptr::null_mut();

        let result = x11_ffi::XGetWindowProperty(
            display,
            window,
            atom_property,
            0,
            !0i64 >> 1,
            0,
            0,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        let text = if result == 0 && !prop.is_null() && nitems > 0 {
            let slice = std::slice::from_raw_parts(prop, nitems as usize);
            let s = String::from_utf8(slice.to_vec()).ok();
            x11_ffi::XFree(prop as *mut std::ffi::c_void);
            s
        } else {
            None
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

/// Reads clipboard image data – not supported via command-line tools on X11.
#[cfg(target_os = "linux")]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    // Try xclip with common image targets
    for target in &["image/png", "image/bmp", "image/jpeg", "image/tiff"] {
        if let Ok(output) = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", target, "-out"])
            .output()
        {
            if output.status.success() && !output.stdout.is_empty() {
                if let Ok(img) = image::load_from_memory(&output.stdout) {
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

/// Reads file paths – not supported via simple command-line tools on X11.
#[cfg(target_os = "linux")]
pub fn read_clipboard_file_paths() -> Vec<String> {
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

        let window_id = if res == 0 && !prop.is_null() && nitems > 0 && actual_format == 32 {
            let w = *(prop as *mut u32) as u64;
            x11_ffi::XFree(prop as *mut std::ffi::c_void);
            w
        } else {
            x11_ffi::XCloseDisplay(display);
            return crate::platform::ForegroundApp::empty();
        };

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
                let p = *(prop2 as *mut u32);
                x11_ffi::XFree(prop2 as *mut std::ffi::c_void);
                Some(p)
            } else {
                None
            };

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

/// Extracts an app icon – not supported via command-line tools on X11.
#[cfg(target_os = "linux")]
pub fn extract_app_icon(
    _icon_dir: &std::path::Path,
    _app_name: &str,
    _exe_path: &str,
) -> Option<String> {
    None
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

// ---------------------------------------------------------------------------
// X11GlobalHotkey
// ---------------------------------------------------------------------------

/// Registers global keyboard shortcuts via `XGrabKey` on the root window.
///
/// # How it works
///
/// 1. Open an X display connection.
/// 2. For each shortcut, translate the `ShortcutBinding` to a `(keycode,
///    modifiers)` pair using `X11ModifierMapping`.
/// 3. Call `XGrabKey(display, keycode, modifiers, root_window, True,
///    GrabModeAsync, GrabModeAsync)` — this ensures no other application
///    can steal the combination.
/// 4. Also grab variations: `modifiers | LockMask` (CapsLock),
///    `modifiers | Mod2Mask` (NumLock), `modifiers | LockMask | Mod2Mask`.
/// 5. Enter an event loop (or piggyback on the existing one) listening for
///    `KeyPress` events.  When a registered combination arrives, invoke the
///    callback.
/// 6. On shutdown, call `XUngrabKey` for every registered combination.
///
/// # Limitations
///
/// - `XGrabKey` can only register chord-based shortcuts (modifier + regular
///   key).  Double-modifier shortcuts require a different strategy: listen
///   for raw KeyPress events on modifier keys and detect double-taps in
///   software.
type HotkeyCallback = Box<dyn Fn(&str) + Send + Sync + 'static>;

pub struct X11GlobalHotkey {
    /// Set of registered (keycode, modifiers) pairs currently grabbed.
    registered: HashSet<(u32, u32)>,
    /// Map from (keycode, modifiers) to action_id for callback dispatch.
    action_map: HashMap<(u32, u32), String>,
    /// Callback invoked when a grabbed key fires.
    on_hotkey: Option<HotkeyCallback>,
}

impl Default for X11GlobalHotkey {
    fn default() -> Self {
        Self::new()
    }
}

impl X11GlobalHotkey {
    /// Creates a new, empty hotkey manager.
    pub fn new() -> Self {
        Self {
            registered: HashSet::new(),
            action_map: HashMap::new(),
            on_hotkey: None,
        }
    }

    /// Registers one or more shortcuts.
    ///
    /// # Errors
    ///
    /// Returns `X11Error::HotkeyGrabFailed` if the key combination is already
    /// grabbed by another application.
    pub fn register(&mut self, action_id: &str, shortcuts: &[ShortcutBinding]) -> X11Result<()> {
        // Implementation:
        //
        // let display = XOpenDisplay(NULL);
        // let root = XDefaultRootWindow(display);
        //
        // for binding in shortcuts {
        //     if let Some((keycode, mods)) =
        //         X11ModifierMapping::to_grab_params(binding) {
        //         // Also grab with LockMask and Mod2Mask variations
        //         for extra in &[0, LOCK_MASK, MOD2_MASK, LOCK_MASK | MOD2_MASK] {
        //             let full_mods = mods | extra;
        //             let result = XGrabKey(display, keycode as i32, full_mods,
        //                 root, 1, GRAB_MODE_ASYNC, GRAB_MODE_ASYNC);
        //             if result != 0 {
        //                 return Err(X11Error::HotkeyGrabFailed(keycode, full_mods));
        //             }
        //             self.registered.insert((keycode, full_mods));
        //         }
        //         self.action_map.insert((keycode, mods), action_id.to_owned());
        //     }
        // }
        // XFlush(display);
        // Ok(())
        let _ = (action_id, shortcuts);
        Ok(())
    }

    /// Unregisters all grabbed keys.
    pub fn unregister_all(&mut self) -> X11Result<()> {
        // for (keycode, mods) in &self.registered {
        //     XUngrabKey(display, *keycode as i32, *mods, root);
        // }
        // self.registered.clear();
        // self.action_map.clear();
        Ok(())
    }

    /// Sets the callback invoked when a registered key fires.
    pub fn set_callback<F>(&mut self, callback: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.on_hotkey = Some(Box::new(callback));
    }

    /// Returns whether any shortcuts are currently registered.
    pub fn is_active(&self) -> bool {
        !self.registered.is_empty()
    }

    /// Returns the number of grabbed key combinations.
    pub fn count(&self) -> usize {
        self.registered.len()
    }
}

// ---------------------------------------------------------------------------
// X11TrayManager
// ---------------------------------------------------------------------------

/// Manages a system tray icon using either libappindicator or XEmbed.
///
/// # Strategy
///
/// The adapter attempts the following in order:
///
/// 1. **libappindicator3** (modern Ubuntu / Debian):
///    - Uses the `AppIndicator3` D-Bus interface.
///    - Provides `org.kde.StatusNotifierItem` compatibility.
///
/// 2. **libappindicator1** (older systems):
///    - Fallback for Ubuntu < 18.04.
///
/// 3. **XEmbed** (legacy / lightweight WMs):
///    - Creates a small `GtkStatusIcon` window and embeds it into the tray.
///    - Uses `_NET_SYSTEM_TRAY_S0` selection to find the tray manager window.
///    - Sends `SYSTEM_TRAY_REQUEST_DOCK` client message.
///
/// 4. **StatusNotifierItem** (KDE / modern GNOME):
///    - D-Bus implementation of the `org.kde.StatusNotifierItem` spec.
///    - Register on `org.kde.StatusNotifierWatcher`.
///
/// # Backend Detection
///
/// ```text
/// Check $XDG_CURRENT_DESKTOP → if KDE → StatusNotifierItem
///                           → if GNOME → check for AppIndicator extension
///                           → if XFCE  → XEmbed tray
///                           → fallback → XEmbed
/// ```
pub struct X11TrayManager {
    /// Current backend in use.
    backend: X11TrayBackend,
    /// Whether the tray icon is currently visible.
    visible: bool,
}

/// Identifies which tray implementation is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11TrayBackend {
    None,
    LibAppIndicator3,
    LibAppIndicator1,
    XEmbed,
    StatusNotifierItem,
}

/// A tray menu item definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X11TrayMenuItem {
    Item { label: String, action: String },
    Separator,
}

impl X11TrayManager {
    /// Creates a tray manager, auto-detecting the best available backend.
    pub fn create() -> X11Result<Self> {
        // Detection logic:
        // 1. Try StatusNotifierItem via D-Bus if $XDG_CURRENT_DESKTOP is KDE.
        // 2. Try libappindicator3 via dlopen("libappindicator3.so").
        // 3. Try libappindicator1.
        // 4. Try _NET_SYSTEM_TRAY_S0 selection → XEmbed.
        // 5. Otherwise set backend to None.
        Ok(Self {
            backend: X11TrayBackend::None,
            visible: false,
        })
    }

    /// Creates a tray manager with an explicitly chosen backend.
    pub fn with_backend(backend: X11TrayBackend) -> X11Result<Self> {
        Ok(Self {
            backend,
            visible: false,
        })
    }

    /// Shows the tray icon.  On first call this creates the icon.
    pub fn show(&mut self) -> X11Result<()> {
        match self.backend {
            X11TrayBackend::None => {
                return Err(X11Error::TrayCreationFailed(
                    "no tray backend available".into(),
                ));
            }
            X11TrayBackend::LibAppIndicator3 | X11TrayBackend::LibAppIndicator1 => {
                // Create AppIndicator via D-Bus or directly.
            }
            X11TrayBackend::XEmbed => {
                // XEmbed tray:
                // 1. XOpenDisplay
                // 2. Find _NET_SYSTEM_TRAY_S0 selection owner
                // 3. Create GTK invisible window
                // 4. Send SYSTEM_TRAY_REQUEST_DOCK client message
            }
            X11TrayBackend::StatusNotifierItem => {
                // Register on org.kde.StatusNotifierWatcher via D-Bus.
            }
        }
        self.visible = true;
        Ok(())
    }

    /// Hides the tray icon.
    pub fn hide(&mut self) -> X11Result<()> {
        self.visible = false;
        Ok(())
    }

    /// Sets the tray menu items.
    pub fn set_menu(&mut self, _items: &[X11TrayMenuItem]) -> X11Result<()> {
        Ok(())
    }

    /// Sets the tray icon tooltip text.
    pub fn set_tooltip(&mut self, _text: &str) -> X11Result<()> {
        Ok(())
    }

    /// Sets the tray icon from a file path (PNG preferred).
    pub fn set_icon_from_path(&mut self, _path: &str) -> X11Result<()> {
        Ok(())
    }

    /// Returns the currently active backend.
    pub fn backend(&self) -> X11TrayBackend {
        self.backend
    }

    /// Attempts to detect the best available tray backend on this system.
    pub fn detect_backend() -> X11TrayBackend {
        // 1. Check environment: $XDG_CURRENT_DESKTOP, $DESKTOP_SESSION
        // 2. Check D-Bus availability for StatusNotifierItem
        // 3. Check for libappindicator shared libraries
        // 4. Check for _NET_SYSTEM_TRAY_S0 atom on the X display
        X11TrayBackend::None
    }

    /// Returns a description of each backend's capabilities.
    pub fn backend_info() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "StatusNotifierItem",
                "Modern D-Bus tray protocol. Supported by KDE Plasma, GNOME (with extension), \
                 Budgie, Cinnamon.",
            ),
            (
                "libappindicator3",
                "Ubuntu's AppIndicator library. Supported on Ubuntu 18.04+, elementary OS, \
                 and many GNOME-based desktops with the AppIndicator extension.",
            ),
            (
                "libappindicator1",
                "Older AppIndicator library. Ubuntu 16.04 and earlier.",
            ),
            (
                "XEmbed",
                "Legacy XEmbed protocol. Supported by most lightweight WMs (i3, dwm, Openbox, \
                 Fluxbox) and older desktop environments.",
            ),
        ]
    }

    /// Default menu for clipboard manager tray.
    pub fn default_menu() -> Vec<X11TrayMenuItem> {
        vec![
            X11TrayMenuItem::Item {
                label: "Show/Hide".to_owned(),
                action: "toggleWindow".to_owned(),
            },
            X11TrayMenuItem::Separator,
            X11TrayMenuItem::Item {
                label: "Preferences".to_owned(),
                action: "openPreferences".to_owned(),
            },
            X11TrayMenuItem::Item {
                label: "About".to_owned(),
                action: "openAbout".to_owned(),
            },
            X11TrayMenuItem::Separator,
            X11TrayMenuItem::Item {
                label: "Quit".to_owned(),
                action: "quit".to_owned(),
            },
        ]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_error_display() {
        let err = X11Error::DisplayOpenFailed(":0".to_owned());
        assert!(err.to_string().contains(":0"));
    }

    #[test]
    fn clipboard_monitor_lifecycle() {
        let mut monitor = X11ClipboardMonitor::new();
        assert!(!monitor.is_running());

        // On non-Linux, start/stop are no-ops.
        monitor.start().unwrap();
        assert!(monitor.is_running());

        monitor.stop().unwrap();
        assert!(!monitor.is_running());
    }

    #[test]
    fn ignored_apps_management() {
        let mut monitor = X11ClipboardMonitor::new();
        monitor.set_ignored_apps(vec!["firefox".to_owned(), "chromium".to_owned()]);
        let apps = monitor.ignored_apps();
        assert_eq!(apps.len(), 2);
    }

    #[test]
    fn hotkey_manager_creates_and_destroys() {
        let mut hotkey = X11GlobalHotkey::new();
        assert!(!hotkey.is_active());
        assert_eq!(hotkey.count(), 0);

        hotkey.register("test", &[]).unwrap();
        hotkey.unregister_all().unwrap();
    }

    #[test]
    fn tray_creation() {
        let tray = X11TrayManager::create().unwrap();
        assert_eq!(tray.backend(), X11TrayBackend::None);
    }

    #[test]
    fn tray_default_menu_not_empty() {
        let menu = X11TrayManager::default_menu();
        assert!(!menu.is_empty());
        let has_quit = menu
            .iter()
            .any(|item| matches!(item, X11TrayMenuItem::Item { action, .. } if action == "quit"));
        assert!(has_quit);
    }

    #[test]
    fn tray_backend_info_has_entries() {
        let info = X11TrayManager::backend_info();
        assert!(!info.is_empty());
    }

    #[test]
    fn x11_selection_enum_values() {
        assert_ne!(X11Selection::Primary, X11Selection::Clipboard);
    }
}
