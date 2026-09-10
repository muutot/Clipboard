use std::collections::{BTreeSet, HashSet};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
#[cfg(target_os = "windows")]
use std::time::Duration;

use super::windows_clipboard;
use crate::keyboard::{Modifier, DEFAULT_DOUBLE_TAP_INTERVAL_MS};

const WM_HOTKEY: u32 = 0x0312;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
const WH_KEYBOARD_LL: i32 = 13;
const FIRST_HOTKEY_ID: i32 = 1;
#[cfg(target_os = "windows")]
const QUICK_PASTE_FOCUS_DELAY: Duration = Duration::from_millis(60);

#[derive(Default)]
struct QuickPasteTarget {
    window_handle: Mutex<Option<isize>>,
}

impl QuickPasteTarget {
    fn remember(&self, window_handle: isize) {
        if window_handle == 0 {
            return;
        }
        if let Ok(mut target) = self.window_handle.lock() {
            *target = Some(window_handle);
        }
    }

    fn take(&self) -> Option<isize> {
        self.window_handle
            .lock()
            .ok()
            .and_then(|mut target| target.take())
    }

    fn clear(&self) {
        if let Ok(mut target) = self.window_handle.lock() {
            *target = None;
        }
    }
}

type HotkeyRegistration = (i32, u32, u32);

/// OS hotkey action selected by the fired registration id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleMain,
    ToggleFloat,
}

/// Id base for float-panel registrations. Toggle registrations occupy
/// `FIRST_HOTKEY_ID..`, so the ranges never overlap by construction.
pub const FLOAT_HOTKEY_ID_BASE: i32 = 1000;

/// Maps a fired `WM_HOTKEY` id onto its action. Pure so the routing is
/// unit-testable without a message loop.
pub fn action_for_hotkey_id(id: i32) -> HotkeyAction {
    if id >= FLOAT_HOTKEY_ID_BASE {
        HotkeyAction::ToggleFloat
    } else {
        HotkeyAction::ToggleMain
    }
}

fn modifier_from_virtual_key(virtual_key: u32) -> Option<Modifier> {
    match virtual_key {
        0x10 | 0xA0 | 0xA1 => Some(Modifier::Shift),
        0x11 | 0xA2 | 0xA3 => Some(Modifier::Control),
        0x12 | 0xA4 | 0xA5 => Some(Modifier::Alt),
        0x5B | 0x5C => Some(Modifier::Meta),
        _ => None,
    }
}

/// Detects bare modifier double taps from low-level key events.
/// Mirrors `ShortcutMatcher::record_modifier_tap`: a tap is one clean press
/// and release, and any other key in between cancels the pending sequence.
struct DoubleModifierTracker {
    registered: BTreeSet<Modifier>,
    double_tap_interval_ms: u64,
    active_press: Option<Modifier>,
    press_interrupted: bool,
    last_tap: Option<(Modifier, u64)>,
}

impl DoubleModifierTracker {
    fn new(registered: impl IntoIterator<Item = Modifier>) -> Self {
        Self {
            registered: registered.into_iter().collect(),
            double_tap_interval_ms: DEFAULT_DOUBLE_TAP_INTERVAL_MS,
            active_press: None,
            press_interrupted: false,
            last_tap: None,
        }
    }

    fn on_key_event(&mut self, virtual_key: u32, is_key_down: bool, timestamp_ms: u64) -> bool {
        let Some(modifier) = modifier_from_virtual_key(virtual_key) else {
            if is_key_down {
                self.press_interrupted = self.active_press.is_some();
                self.last_tap = None;
            }
            return false;
        };

        if is_key_down {
            if self.active_press == Some(modifier) {
                // Key auto-repeat while the modifier stays held down.
                return false;
            }
            if self.active_press.is_some() {
                // Two different modifiers held together are not a bare tap.
                self.press_interrupted = true;
                self.last_tap = None;
                return false;
            }
            self.active_press = Some(modifier);
            self.press_interrupted = false;
            return false;
        }

        if self.active_press != Some(modifier) {
            return false;
        }
        self.active_press = None;
        if self.press_interrupted {
            self.press_interrupted = false;
            return false;
        }

        let is_double_tap = self
            .last_tap
            .is_some_and(|(previous_modifier, previous_timestamp)| {
                previous_modifier == modifier
                    && timestamp_ms >= previous_timestamp
                    && timestamp_ms - previous_timestamp <= self.double_tap_interval_ms
            });
        if is_double_tap && self.registered.contains(&modifier) {
            self.last_tap = None;
            true
        } else {
            self.last_tap = Some((modifier, timestamp_ms));
            false
        }
    }
}

fn deduplicate_hotkeys(bindings: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut seen = HashSet::new();
    bindings
        .iter()
        .copied()
        .filter(|binding| seen.insert(*binding))
        .collect()
}

fn assign_hotkey_ids(bindings: &[(u32, u32)]) -> Vec<HotkeyRegistration> {
    assign_hotkey_ids_with_base(bindings, FIRST_HOTKEY_ID)
}

fn assign_hotkey_ids_with_base(bindings: &[(u32, u32)], base: i32) -> Vec<HotkeyRegistration> {
    deduplicate_hotkeys(bindings)
        .into_iter()
        .enumerate()
        .map(|(index, (modifiers, vk))| (base + index as i32, modifiers, vk))
        .collect()
}

/// Toggle plus float registrations for one shared message loop. Id ranges
/// are disjoint by construction (see [`FLOAT_HOTKEY_ID_BASE`]).
pub fn combined_hotkey_registrations(
    toggle_bindings: &[(u32, u32)],
    float_bindings: &[(u32, u32)],
) -> Vec<HotkeyRegistration> {
    let mut registrations = assign_hotkey_ids(toggle_bindings);
    registrations.extend(assign_hotkey_ids_with_base(
        float_bindings,
        FLOAT_HOTKEY_ID_BASE,
    ));
    registrations
}

fn spawn_hotkey_thread_with_registrations(
    registrations: Vec<HotkeyRegistration>,
    double_modifiers: Vec<Modifier>,
    tx: mpsc::Sender<HotkeyAction>,
) -> thread::JoinHandle<()> {
    set_hotkey_sender(&tx);
    // Readiness handshake: the message window must exist before this returns,
    // otherwise a subsequent stop()+join can post WM_QUIT to a not-yet-known
    // hwnd and block forever joining a thread stuck in GetMessageW. At
    // startup the float registration restarts the loop immediately after the
    // toggle registration, which hit exactly that race and froze the app.
    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        let result = hotkey_message_loop(&registrations, &double_modifiers, ready_tx);
        clear_hotkey_state();
        if let Err(error) = result {
            crate::log_event!("[hotkey] message loop exited with error: {error}");
        }
        drop(tx);
    });
    if ready_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .is_err()
    {
        crate::log_event!("[hotkey] message loop did not signal readiness; a later stop may block");
    }
    handle
}

fn hotkey_message_loop(
    registrations: &[HotkeyRegistration],
    double_modifiers: &[Modifier],
    ready: mpsc::Sender<()>,
) -> Result<(), String> {
    if registrations.is_empty() && double_modifiers.is_empty() {
        return Err("no supported hotkey bindings were provided".to_owned());
    }

    extern "system" {
        fn GetModuleHandleW(module: *const u16) -> isize;
        fn GetLastError() -> u32;
        fn RegisterClassExW(class: *const WndClassExW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class_name: *const u16,
            window_name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: isize,
            menu: isize,
            instance: isize,
            param: *const std::ffi::c_void,
        ) -> isize;
        fn GetMessageW(msg: *mut Msg, hwnd: isize, filter_min: u32, filter_max: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn DestroyWindow(hwnd: isize) -> i32;
        fn SetWindowsHookExW(id: i32, hook_proc: usize, module: isize, thread_id: u32) -> isize;
        fn UnhookWindowsHookEx(hook: isize) -> i32;
    }
    #[repr(C)]
    struct WndClassExW {
        size: u32,
        style: u32,
        wnd_proc: usize,
        cls_extra: i32,
        wnd_extra: i32,
        instance: isize,
        icon: isize,
        cursor: isize,
        background: isize,
        menu_name: *const u16,
        class_name: *const u16,
        icon_sm: isize,
    }

    #[repr(C)]
    struct Msg {
        hwnd: isize,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt_x: i32,
        pt_y: i32,
    }

    let class_name: Vec<u16> = "ClipboardHotkeyWindow\0".encode_utf16().collect();
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };

    unsafe {
        let wc = WndClassExW {
            size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            wnd_proc: hotkey_window_proc as *const () as usize,
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: 0,
            cursor: 0,
            background: 0,
            menu_name: std::ptr::null(),
            class_name: class_name.as_ptr(),
            icon_sm: 0,
        };

        let atom = RegisterClassExW(&wc);
        if atom == 0 && GetLastError() != 1410 {
            let _ = ready.send(());
            return Err("RegisterClassExW failed".to_string());
        }

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            -3isize, // HWND_MESSAGE
            0,
            instance,
            std::ptr::null(),
        );
        if hwnd == 0 {
            let _ = ready.send(());
            return Err("CreateWindowExW failed".to_string());
        }

        set_hotkey_hwnd(hwnd);
        // The stop path posts WM_QUIT to this hwnd, so signal readiness only
        // once it is published.
        let _ = ready.send(());

        let mut registered_ids = Vec::with_capacity(registrations.len());
        for (id, modifiers, vk) in registrations {
            if let Err(error) =
                windows_clipboard::register_global_hotkey(hwnd, *id, *modifiers, *vk)
            {
                for registered_id in registered_ids {
                    let _ = windows_clipboard::unregister_global_hotkey(hwnd, registered_id);
                }
                clear_hotkey_state();
                DestroyWindow(hwnd);
                let _ = ready.send(());
                return Err(error);
            }
            registered_ids.push(*id);
        }

        // A bare double-modifier tap cannot be expressed with RegisterHotKey,
        // so it is detected with a low-level keyboard hook on this same
        // message-pump thread.
        let keyboard_hook = if double_modifiers.is_empty() {
            0
        } else {
            set_double_modifier_tracker(double_modifiers);
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                keyboard_hook_proc as *const () as usize,
                0,
                0,
            );
            if hook == 0 {
                clear_double_modifier_tracker();
                crate::log_event!(
                    "[hotkey] failed to install the double-modifier keyboard hook (Windows error {})",
                    GetLastError()
                );
            }
            hook
        };

        let mut msg: Msg = std::mem::zeroed();
        loop {
            let ret = GetMessageW(&mut msg, 0, 0, 0);
            if ret == 0 || ret == -1 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if keyboard_hook != 0 {
            UnhookWindowsHookEx(keyboard_hook);
        }
        clear_double_modifier_tracker();
        for id in registered_ids {
            let _ = windows_clipboard::unregister_global_hotkey(hwnd, id);
        }
        DestroyWindow(hwnd);
    }

    Ok(())
}

unsafe extern "system" fn hotkey_window_proc(
    hwnd: isize,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if msg == WM_HOTKEY && wparam != 0 {
        if let Some(tx) = HOTKEY_SENDER.lock().ok().and_then(|g| g.clone()) {
            let _ = tx.send(action_for_hotkey_id(wparam as i32));
        }
        return 0;
    }

    extern "system" {
        fn DefWindowProcW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    extern "system" {
        fn CallNextHookEx(hook: isize, code: i32, wparam: usize, lparam: isize) -> isize;
    }

    #[repr(C)]
    struct KeyboardHookEvent {
        virtual_key: u32,
        scan_code: u32,
        flags: u32,
        time: u32,
        extra_info: usize,
    }

    if code >= 0 && lparam != 0 {
        let message = wparam as u32;
        let is_key_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let is_key_up = message == WM_KEYUP || message == WM_SYSKEYUP;
        if is_key_down || is_key_up {
            let event = &*(lparam as *const KeyboardHookEvent);
            let fired = DOUBLE_MODIFIER_TRACKER
                .lock()
                .ok()
                .and_then(|mut tracker| {
                    tracker.as_mut().map(|tracker| {
                        tracker.on_key_event(event.virtual_key, is_key_down, u64::from(event.time))
                    })
                })
                .unwrap_or(false);
            if fired {
                // Double-tap modifiers always drive the main toggle; float
                // chords are registered as plain hotkeys only.
                if let Some(tx) = HOTKEY_SENDER.lock().ok().and_then(|g| g.clone()) {
                    let _ = tx.send(HotkeyAction::ToggleMain);
                }
            }
        }
    }

    CallNextHookEx(0, code, wparam, lparam)
}

static HOTKEY_SENDER: Mutex<Option<mpsc::Sender<HotkeyAction>>> = Mutex::new(None);
static HOTKEY_HWND: Mutex<isize> = Mutex::new(0);
static DOUBLE_MODIFIER_TRACKER: Mutex<Option<DoubleModifierTracker>> = Mutex::new(None);

fn set_double_modifier_tracker(double_modifiers: &[Modifier]) {
    if let Ok(mut guard) = DOUBLE_MODIFIER_TRACKER.lock() {
        *guard = Some(DoubleModifierTracker::new(double_modifiers.iter().copied()));
    }
}

fn clear_double_modifier_tracker() {
    if let Ok(mut guard) = DOUBLE_MODIFIER_TRACKER.lock() {
        *guard = None;
    }
}

pub fn set_hotkey_sender(tx: &mpsc::Sender<HotkeyAction>) {
    if let Ok(mut guard) = HOTKEY_SENDER.lock() {
        *guard = Some(tx.clone());
    }
}

pub fn set_hotkey_hwnd(hwnd: isize) {
    if let Ok(mut guard) = HOTKEY_HWND.lock() {
        *guard = hwnd;
    }
}

pub fn clear_hotkey_state() {
    if let Ok(mut guard) = HOTKEY_SENDER.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = HOTKEY_HWND.lock() {
        *guard = 0;
    }
    clear_double_modifier_tracker();
}

pub fn stop_hotkey_thread() {
    let hwnd = HOTKEY_HWND.lock().ok().and_then(|g| {
        let h = *g;
        if h != 0 {
            Some(h)
        } else {
            None
        }
    });
    if let Some(hwnd) = hwnd {
        extern "system" {
            fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
        }
        const WM_QUIT: u32 = 0x0012;
        unsafe {
            PostMessageW(hwnd, WM_QUIT, 0, 0);
        }
    }
    clear_hotkey_state();
}

pub struct HotkeyManager {
    handle: Option<thread::JoinHandle<()>>,
    window: Option<tauri::WebviewWindow>,
    toggle_bindings: Vec<(u32, u32)>,
    toggle_doubles: Vec<Modifier>,
    float_bindings: Vec<(u32, u32)>,
    float_app: Option<tauri::AppHandle>,
    quick_paste_target: Arc<QuickPasteTarget>,
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self {
            handle: None,
            window: None,
            toggle_bindings: Vec::new(),
            toggle_doubles: Vec::new(),
            float_bindings: Vec::new(),
            float_app: None,
            quick_paste_target: Arc::new(QuickPasteTarget::default()),
        }
    }

    pub fn start_with_window(&mut self, modifiers: u32, vk: u32, window: tauri::WebviewWindow) {
        self.start_with_bindings(vec![(modifiers, vk)], window);
    }

    pub fn start_with_bindings(&mut self, bindings: Vec<(u32, u32)>, window: tauri::WebviewWindow) {
        self.start_with_hotkeys(bindings, Vec::new(), window);
    }

    pub fn start_with_hotkeys(
        &mut self,
        bindings: Vec<(u32, u32)>,
        double_modifiers: Vec<Modifier>,
        window: tauri::WebviewWindow,
    ) {
        self.toggle_bindings = bindings;
        self.toggle_doubles = double_modifiers;
        self.window = Some(window.clone());
        self.restart_combined_thread();
    }

    /// Registers float-panel chord bindings on the same shared message loop.
    /// The app handle opens the backend-owned float window without involving
    /// any frontend state.
    pub fn set_float_hotkeys(&mut self, bindings: Vec<(u32, u32)>, app: tauri::AppHandle) {
        self.float_bindings = bindings;
        self.float_app = Some(app);
        self.restart_combined_thread();
    }

    /// (Re)builds the single hotkey thread from the stored toggle + float
    /// bindings. Stopping first tears down the previous message window, so
    /// re-registration never leaks hotkeys.
    fn restart_combined_thread(&mut self) {
        self.stop();
        let Some(window) = self.window.clone() else {
            return;
        };
        let registrations =
            combined_hotkey_registrations(&self.toggle_bindings, &self.float_bindings);
        if registrations.is_empty() && self.toggle_doubles.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel::<HotkeyAction>();
        let handle =
            spawn_hotkey_thread_with_registrations(registrations, self.toggle_doubles.clone(), tx);
        let quick_paste_target = Arc::clone(&self.quick_paste_target);
        let float_app = self.float_app.clone();

        thread::spawn(move || {
            while let Ok(action) = rx.recv() {
                match action {
                    HotkeyAction::ToggleMain => {
                        let is_visible = window.is_visible().unwrap_or(false);
                        let is_focused = window.is_focused().unwrap_or(false);
                        if !is_focused {
                            if let Some(window_handle) = foreground_window_handle() {
                                quick_paste_target.remember(window_handle);
                            }
                        }
                        if is_visible && is_focused {
                            let _ = window.hide();
                        } else {
                            if !is_visible {
                                let _ = window.show();
                            }
                            if !is_focused {
                                let _ = window.set_focus();
                            }
                        }
                    }
                    HotkeyAction::ToggleFloat => {
                        if let Some(app) = float_app.as_ref() {
                            if let Err(error) =
                                crate::commands::float::toggle_float_panel(app.clone())
                            {
                                crate::log_event!(
                                    "[hotkey] failed to toggle the float panel: {error}"
                                );
                            }
                        }
                    }
                }
            }
        });

        self.handle = Some(handle);
    }

    pub fn restart_with_hotkeys(
        &mut self,
        bindings: Vec<(u32, u32)>,
        double_modifiers: Vec<Modifier>,
    ) {
        if let Some(window) = self.window.clone() {
            self.start_with_hotkeys(bindings, double_modifiers, window);
        }
    }

    pub fn take_quick_paste_target(&self) -> Option<isize> {
        self.quick_paste_target.take()
    }

    /// Records the current foreground window as the quick-paste target so the
    /// main window can restore it when it is shown from the tray or another
    /// entry point (the toggle hotkey path records it inside its own loop).
    pub fn remember_foreground(&self) {
        if let Some(window_handle) = foreground_window_handle() {
            self.quick_paste_target.remember(window_handle);
        }
    }

    pub fn stop(&mut self) {
        stop_hotkey_thread();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.quick_paste_target.clear();
    }
}

#[cfg(target_os = "windows")]
fn foreground_window_handle() -> Option<isize> {
    extern "system" {
        fn GetForegroundWindow() -> isize;
    }

    let window_handle = unsafe { GetForegroundWindow() };
    (window_handle != 0).then_some(window_handle)
}

#[cfg(not(target_os = "windows"))]
fn foreground_window_handle() -> Option<isize> {
    None
}

#[cfg(target_os = "windows")]
pub fn restore_window_and_paste(window_handle: isize) -> Result<(), String> {
    const SW_RESTORE: i32 = 9;

    extern "system" {
        fn IsWindow(window: isize) -> i32;
        fn IsIconic(window: isize) -> i32;
        fn ShowWindow(window: isize, command: i32) -> i32;
        fn BringWindowToTop(window: isize) -> i32;
        fn SetForegroundWindow(window: isize) -> i32;
        fn GetForegroundWindow() -> isize;
    }

    if window_handle == 0 || unsafe { IsWindow(window_handle) } == 0 {
        return Err("the previous foreground window is no longer available".to_owned());
    }

    unsafe {
        let iconic = IsIconic(window_handle);
        if iconic != 0 {
            ShowWindow(window_handle, SW_RESTORE);
        }
        BringWindowToTop(window_handle);
        SetForegroundWindow(window_handle);
    }

    thread::sleep(QUICK_PASTE_FOCUS_DELAY);
    if unsafe { GetForegroundWindow() } != window_handle {
        return Err("failed to restore the previous foreground window".to_owned());
    }

    send_ctrl_v()
}

#[cfg(not(target_os = "windows"))]
pub fn restore_window_and_paste(_window_handle: isize) -> Result<(), String> {
    Err("quick paste is only implemented on Windows".to_owned())
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MouseInput {
    dx: i32,
    dy: i32,
    mouse_data: u32,
    flags: u32,
    time: u32,
    extra_info: usize,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
struct KeyboardInput {
    virtual_key: u16,
    scan_code: u16,
    flags: u32,
    time: u32,
    extra_info: usize,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
struct HardwareInput {
    message: u32,
    w_param_l: u16,
    w_param_h: u16,
}

// The Win32 `INPUT` union spans all three variants so on 64-bit Windows the
// whole struct is exactly `sizeof(INPUT)` == 40 bytes. Declaring only the
// keyboard variant shrank it to 32 bytes, which made `SendInput` reject the
// buffer with ERROR_INVALID_PARAMETER (87).
#[cfg(target_os = "windows")]
#[repr(C)]
union InputData {
    mouse: MouseInput,
    keyboard: KeyboardInput,
    hardware: HardwareInput,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct Input {
    input_type: u32,
    data: InputData,
}

#[cfg(target_os = "windows")]
fn send_ctrl_v() -> Result<(), String> {
    const INPUT_KEYBOARD: u32 = 1;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const VK_CONTROL: u16 = 0x11;
    const VK_V: u16 = 0x56;

    fn keyboard_input(virtual_key: u16, flags: u32) -> Input {
        Input {
            input_type: INPUT_KEYBOARD,
            data: InputData {
                keyboard: KeyboardInput {
                    virtual_key,
                    scan_code: 0,
                    flags,
                    time: 0,
                    extra_info: 0,
                },
            },
        }
    }

    extern "system" {
        fn SendInput(input_count: u32, inputs: *const Input, input_size: i32) -> u32;
        fn GetLastError() -> u32;
    }

    let inputs = [
        keyboard_input(VK_CONTROL, 0),
        keyboard_input(VK_V, 0),
        keyboard_input(VK_V, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<Input>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        return Err(format!(
            "failed to send Ctrl+V input (Windows error {})",
            unsafe { GetLastError() }
        ));
    }

    Ok(())
}

pub fn shortcut_to_windows_hotkey(
    binding: &crate::keyboard::ShortcutBinding,
) -> Option<(u32, u32)> {
    match binding {
        crate::keyboard::ShortcutBinding::Chord { modifiers, key } => {
            let mut mod_flags: u32 = 0;
            for m in modifiers {
                match m {
                    crate::keyboard::Modifier::Alt => mod_flags |= windows_clipboard::MOD_ALT,
                    crate::keyboard::Modifier::Control => {
                        mod_flags |= windows_clipboard::MOD_CONTROL
                    }
                    crate::keyboard::Modifier::Shift => mod_flags |= windows_clipboard::MOD_SHIFT,
                    crate::keyboard::Modifier::Meta => mod_flags |= windows_clipboard::MOD_WIN,
                }
            }
            let vk = windows_virtual_key(key)?;
            Some((mod_flags, vk))
        }
        crate::keyboard::ShortcutBinding::DoubleModifier { .. } => None,
    }
}

fn windows_virtual_key(key: &str) -> Option<u32> {
    let normalized = key.to_ascii_uppercase();
    if let Some(function_key) = normalized
        .strip_prefix('F')
        .and_then(|number| number.parse::<u32>().ok())
        .filter(|number| (1..=24).contains(number))
    {
        return Some(0x6F + function_key);
    }

    match normalized.as_str() {
        "BACKSPACE" => Some(0x08),
        "TAB" => Some(0x09),
        "ENTER" | "RETURN" => Some(0x0D),
        "ESC" | "ESCAPE" => Some(0x1B),
        "SPACE" => Some(0x20),
        "PAGEUP" => Some(0x21),
        "PAGEDOWN" => Some(0x22),
        "END" => Some(0x23),
        "HOME" => Some(0x24),
        "LEFT" | "ARROWLEFT" => Some(0x25),
        "UP" | "ARROWUP" => Some(0x26),
        "RIGHT" | "ARROWRIGHT" => Some(0x27),
        "DOWN" | "ARROWDOWN" => Some(0x28),
        "INSERT" => Some(0x2D),
        "DELETE" | "DEL" => Some(0x2E),
        other if other.len() == 1 => {
            let byte = other.as_bytes()[0];
            (byte.is_ascii_alphanumeric()).then_some(byte as u32)
        }
        _ => None,
    }
}

pub fn shortcut_bindings_to_windows_hotkeys(
    bindings: &[crate::keyboard::ShortcutBinding],
) -> Vec<(u32, u32)> {
    let converted = bindings
        .iter()
        .filter_map(shortcut_to_windows_hotkey)
        .collect::<Vec<_>>();
    deduplicate_hotkeys(&converted)
}

pub fn shortcut_bindings_to_double_modifiers(
    bindings: &[crate::keyboard::ShortcutBinding],
) -> Vec<Modifier> {
    let mut seen = BTreeSet::new();
    bindings
        .iter()
        .filter_map(|binding| match binding {
            crate::keyboard::ShortcutBinding::DoubleModifier { modifier } => {
                seen.insert(*modifier).then_some(*modifier)
            }
            crate::keyboard::ShortcutBinding::Chord { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::keyboard::{Modifier, ShortcutBinding};

    use super::{
        action_for_hotkey_id, assign_hotkey_ids, combined_hotkey_registrations,
        shortcut_bindings_to_double_modifiers, shortcut_bindings_to_windows_hotkeys,
        shortcut_to_windows_hotkey, DoubleModifierTracker, HotkeyAction, QuickPasteTarget,
        FIRST_HOTKEY_ID, FLOAT_HOTKEY_ID_BASE,
    };

    const VK_SHIFT: u32 = 0x10;
    const VK_CONTROL: u32 = 0x11;
    const VK_V: u32 = b'V' as u32;

    #[cfg(target_os = "windows")]
    #[test]
    fn input_struct_matches_win32_layout() {
        use super::{HardwareInput, Input, KeyboardInput, MouseInput};

        // sizeof(INPUT) on 64-bit Windows is 40 bytes, on 32-bit 28 bytes.
        let (expected_input, mouse, keyboard) = if cfg!(target_pointer_width = "64") {
            (40, 32, 24)
        } else {
            (28, 24, 16)
        };
        assert_eq!(size_of::<Input>(), expected_input);
        assert_eq!(size_of::<MouseInput>(), mouse);
        assert_eq!(size_of::<KeyboardInput>(), keyboard);
        assert_eq!(size_of::<HardwareInput>(), 8);
    }

    #[test]
    fn quick_paste_target_is_consumed_once() {
        let target = QuickPasteTarget::default();
        target.remember(42);

        assert_eq!(target.take(), Some(42));
        assert_eq!(target.take(), None);
    }

    #[test]
    fn quick_paste_target_ignores_invalid_window_handle() {
        let target = QuickPasteTarget::default();
        target.remember(0);

        assert_eq!(target.take(), None);
    }

    #[test]
    fn converts_supported_windows_keys() {
        let bindings = [
            ShortcutBinding::from_str("Alt+V").unwrap(),
            ShortcutBinding::from_str("Ctrl+Enter").unwrap(),
            ShortcutBinding::from_str("Shift+F5").unwrap(),
            ShortcutBinding::from_str("Meta+1").unwrap(),
        ];

        assert_eq!(
            shortcut_bindings_to_windows_hotkeys(&bindings),
            vec![(1, b'V' as u32), (2, 0x0D), (4, 0x74), (8, b'1' as u32)]
        );
    }

    #[test]
    fn batches_bindings_without_duplicate_registration_ids() {
        let bindings = [(1, b'V' as u32), (1, b'V' as u32), (2, 0x20)];

        assert_eq!(
            assign_hotkey_ids(&bindings),
            vec![
                (FIRST_HOTKEY_ID, 1, b'V' as u32),
                (FIRST_HOTKEY_ID + 1, 2, 0x20)
            ]
        );
    }

    #[test]
    fn hotkey_ids_route_to_their_action() {
        assert_eq!(
            action_for_hotkey_id(FIRST_HOTKEY_ID),
            HotkeyAction::ToggleMain
        );
        assert_eq!(
            action_for_hotkey_id(FLOAT_HOTKEY_ID_BASE),
            HotkeyAction::ToggleFloat
        );
        assert_eq!(
            action_for_hotkey_id(FLOAT_HOTKEY_ID_BASE + 7),
            HotkeyAction::ToggleFloat
        );
        // Unknown ids fail toward the main toggle, never toward float.
        assert_eq!(action_for_hotkey_id(0), HotkeyAction::ToggleMain);
        assert_eq!(action_for_hotkey_id(-3), HotkeyAction::ToggleMain);
    }

    #[test]
    fn combined_registrations_keep_disjoint_id_ranges() {
        let toggle = [(1, b'V' as u32)];
        let float = [(4, b'F' as u32), (2, 0x20)];
        let registrations = combined_hotkey_registrations(&toggle, &float);

        assert_eq!(
            registrations,
            vec![
                (FIRST_HOTKEY_ID, 1, b'V' as u32),
                (FLOAT_HOTKEY_ID_BASE, 4, b'F' as u32),
                (FLOAT_HOTKEY_ID_BASE + 1, 2, 0x20),
            ]
        );
        for (id, _, _) in &registrations {
            assert_eq!(
                action_for_hotkey_id(*id),
                if *id >= FLOAT_HOTKEY_ID_BASE {
                    HotkeyAction::ToggleFloat
                } else {
                    HotkeyAction::ToggleMain
                }
            );
        }
    }

    #[test]
    fn empty_sides_still_register_the_other_action() {
        assert_eq!(
            combined_hotkey_registrations(&[], &[(4, b'F' as u32)]).len(),
            1
        );
        assert!(combined_hotkey_registrations(&[], &[]).is_empty());
    }

    #[test]
    fn double_modifier_bindings_are_not_registered_as_native_chords() {
        let binding = ShortcutBinding::from_str("Shift+Shift").unwrap();
        assert_eq!(shortcut_to_windows_hotkey(&binding), None);
    }

    #[test]
    fn collects_double_modifier_bindings_for_the_keyboard_hook() {
        let bindings = [
            ShortcutBinding::from_str("Shift+Shift").unwrap(),
            ShortcutBinding::from_str("Alt+V").unwrap(),
            ShortcutBinding::from_str("Ctrl+Ctrl").unwrap(),
            ShortcutBinding::from_str("Shift+Shift").unwrap(),
        ];

        assert_eq!(
            shortcut_bindings_to_double_modifiers(&bindings),
            vec![Modifier::Shift, Modifier::Control]
        );
    }

    #[test]
    fn tracker_fires_on_a_clean_double_tap() {
        let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

        assert!(!tracker.on_key_event(VK_SHIFT, true, 1_000));
        assert!(!tracker.on_key_event(VK_SHIFT, false, 1_050));
        assert!(!tracker.on_key_event(VK_SHIFT, true, 1_200));
        assert!(tracker.on_key_event(VK_SHIFT, false, 1_250));
    }

    #[test]
    fn tracker_ignores_taps_outside_the_double_tap_interval() {
        let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

        tracker.on_key_event(VK_SHIFT, true, 1_000);
        tracker.on_key_event(VK_SHIFT, false, 1_050);
        tracker.on_key_event(VK_SHIFT, true, 1_500);
        assert!(!tracker.on_key_event(VK_SHIFT, false, 1_550));
        tracker.on_key_event(VK_SHIFT, true, 1_700);
        assert!(tracker.on_key_event(VK_SHIFT, false, 1_750));
    }

    #[test]
    fn tracker_treats_chords_and_other_keys_as_interruptions() {
        let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

        // Shift+V is a chord, not a bare tap.
        tracker.on_key_event(VK_SHIFT, true, 1_000);
        tracker.on_key_event(VK_V, true, 1_020);
        tracker.on_key_event(VK_V, false, 1_040);
        assert!(!tracker.on_key_event(VK_SHIFT, false, 1_060));

        // A clean double tap afterwards still works.
        tracker.on_key_event(VK_SHIFT, true, 1_200);
        tracker.on_key_event(VK_SHIFT, false, 1_220);
        tracker.on_key_event(VK_SHIFT, true, 1_320);
        assert!(tracker.on_key_event(VK_SHIFT, false, 1_340));
    }

    #[test]
    fn tracker_resets_when_a_different_modifier_is_tapped_in_between() {
        let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

        tracker.on_key_event(VK_SHIFT, true, 1_000);
        tracker.on_key_event(VK_SHIFT, false, 1_020);
        tracker.on_key_event(VK_CONTROL, true, 1_060);
        tracker.on_key_event(VK_CONTROL, false, 1_080);
        tracker.on_key_event(VK_SHIFT, true, 1_120);
        assert!(!tracker.on_key_event(VK_SHIFT, false, 1_140));
    }

    #[test]
    fn tracker_only_fires_for_registered_modifiers() {
        let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

        tracker.on_key_event(VK_CONTROL, true, 1_000);
        tracker.on_key_event(VK_CONTROL, false, 1_020);
        tracker.on_key_event(VK_CONTROL, true, 1_100);
        assert!(!tracker.on_key_event(VK_CONTROL, false, 1_120));
    }

    #[test]
    fn tracker_ignores_key_auto_repeat_while_a_modifier_is_held() {
        let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

        tracker.on_key_event(VK_SHIFT, true, 1_000);
        tracker.on_key_event(VK_SHIFT, true, 1_050);
        tracker.on_key_event(VK_SHIFT, false, 1_100);
        tracker.on_key_event(VK_SHIFT, true, 1_200);
        assert!(tracker.on_key_event(VK_SHIFT, false, 1_250));
    }
}
