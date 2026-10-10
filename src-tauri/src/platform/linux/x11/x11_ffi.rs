#![allow(non_camel_case_types, dead_code)]

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
    pub fn XConnectionNumber(display: *mut Display) -> i32;
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
    pub fn XSetIOErrorHandler(
        handler: Option<unsafe extern "C" fn(*mut Display) -> i32>,
    ) -> Option<unsafe extern "C" fn(*mut Display) -> i32>;
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
    pub fn XFixesQueryVersion(
        display: *mut Display,
        major_version: *mut i32,
        minor_version: *mut i32,
    ) -> Status;
    pub fn XFixesSelectSelectionInput(
        display: *mut Display,
        window: Window,
        selection: Atom,
        event_mask: u64,
    ) -> i32;
}
