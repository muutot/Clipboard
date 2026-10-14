//! Declarations of the native macOS APIs used by this adapter
//! (CoreGraphics, Carbon Event Manager, Accessibility, CoreFoundation,
//! NSStatusBar).
//!
//! They are only linkable on macOS.  Only the accessibility probe, the
//! CoreFoundation locale calls, `CFRelease` and the quick-paste event APIs are
//! wired to call sites; the remaining declarations document the FFI boundary
//! for the outlined hotkey/tray work and stay unused, so this file is not
//! evidence that those paths run.

// `ApplicationServices` is the umbrella that exports the CoreGraphics and
// HIServices symbols declared below; nothing else links it for us.
#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
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
    /// `AXIsProcessTrusted` returns a `Boolean` (`UInt8`), not a Rust `bool`.
    pub fn AXIsProcessTrusted() -> u8;
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

// CoreFoundation is NOT re-exported by ApplicationServices, so it needs its
// own block. The declarations live in this file rather than in `locale.rs`:
// the extern block in that module never reached the final link line on the
// macOS arm64 CI runner (`ld: symbol(s) not found: "_CFLocaleCopyCurrentLocale"`),
// while every block in this file links. `CFRelease` is declared once here with
// the canonical `*const` pointee; a second declaration with a different
// signature (the CoreGraphics side takes `*mut`) trips the
// `clashing_extern_declarations` error on newer toolchains.
#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub fn CFRelease(cf: *const std::ffi::c_void);
    pub fn CFLocaleCopyCurrentLocale() -> *const std::ffi::c_void;
    pub fn CFLocaleGetIdentifier(locale: *const std::ffi::c_void) -> *const std::ffi::c_void;
    pub fn CFStringGetCString(
        the_string: *const std::ffi::c_void,
        buffer: *mut u8,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
}
