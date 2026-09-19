use std::collections::BTreeSet;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
#[cfg(target_os = "windows")]
use std::time::Duration;

use tauri::Emitter as _;
use tauri::Manager as _;

use super::hotkey_common::{action_index_for_hotkey_id, plan_registrations};
pub use super::hotkey_common::{
    assign_hotkey_ids, combined_hotkey_registrations, shortcut_bindings_to_double_modifiers,
    shortcut_bindings_to_windows_hotkeys, HotkeyRegistration, FIRST_HOTKEY_ID,
    FLOAT_HOTKEY_ID_BASE,
};
use super::windows_clipboard;
use crate::keyboard::{global_action_ids, Modifier, DEFAULT_DOUBLE_TAP_INTERVAL_MS};

const WM_HOTKEY: u32 = 0x0312;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
const WH_KEYBOARD_LL: i32 = 13;
#[cfg(target_os = "windows")]
const QUICK_PASTE_FOCUS_DELAY: Duration = Duration::from_millis(60);
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const OBJID_WINDOW: i32 = 0;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

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

/// OS hotkey action selected by the fired registration id.
///
/// `Forward` carries a registry-action position for actions without a native
/// handler; the dispatch loop emits them as `global-hotkey` events, so a new
/// global shortcut needs no manager changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleMain,
    ToggleFloat,
    Forward(usize),
}

/// Maps a fired `WM_HOTKEY` id onto its action. Pure so the routing is
/// unit-testable without a message loop. Unknown ids fail toward the main
/// toggle, never toward float.
pub fn action_for_hotkey_id(id: i32) -> HotkeyAction {
    match action_index_for_hotkey_id(id) {
        None | Some(0) => HotkeyAction::ToggleMain,
        Some(1) => HotkeyAction::ToggleFloat,
        Some(index) => HotkeyAction::Forward(index),
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
    // Timestamps are raw GetTickCount() ticks (u32 milliseconds, wraps every
    // ~49.7 days); wrapping subtraction keeps intervals across the wrap.
    last_tap: Option<(Modifier, u32)>,
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

    fn on_key_event(&mut self, virtual_key: u32, is_key_down: bool, timestamp_ms: u32) -> bool {
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
                    && u64::from(timestamp_ms.wrapping_sub(previous_timestamp))
                        <= self.double_tap_interval_ms
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

/// Chord registration failure forwarded to the frontend so the keyboard
/// settings panel can surface a conflict (another app already owns the
/// shortcut) instead of silently showing the binding as active.
#[derive(serde::Serialize, Clone)]
pub struct HotkeyRegistrationFailure<'a> {
    pub action: &'a str,
    pub error: String,
}

fn spawn_hotkey_thread_with_registrations(
    registrations: Vec<HotkeyRegistration>,
    double_modifiers: Vec<Modifier>,
    tx: mpsc::Sender<HotkeyAction>,
    app: Option<tauri::AppHandle>,
    paste_target: Arc<QuickPasteTarget>,
) -> thread::JoinHandle<()> {
    set_hotkey_sender(&tx);
    // Readiness handshake: the message window must exist before this returns,
    // otherwise a subsequent stop()+join can post WM_QUIT to a not-yet-known
    // hwnd and block forever joining a thread stuck in GetMessageW. At
    // startup the float registration restarts the loop immediately after the
    // toggle registration, which hit exactly that race and froze the app.
    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        let result = hotkey_message_loop(
            &registrations,
            &double_modifiers,
            ready_tx,
            app,
            paste_target,
        );
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
    app: Option<tauri::AppHandle>,
    paste_target: Arc<QuickPasteTarget>,
) -> Result<(), String> {
    if registrations.is_empty() && double_modifiers.is_empty() {
        return Err("no supported hotkey bindings were provided".to_owned());
    }
    set_foreground_paste_target(&paste_target);

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
        fn SetWinEventHook(
            event_min: u32,
            event_max: u32,
            module: isize,
            proc: usize,
            process_id: u32,
            thread_id: u32,
            flags: u32,
        ) -> isize;
        fn UnhookWinEvent(hook: isize) -> i32;
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
                // A single occupied chord (another app already owns that
                // shortcut) must not take down every other action sharing
                // this loop: skip that chord and keep serving the rest.
                let action = action_index_for_hotkey_id(*id)
                    .and_then(|index| global_action_ids().nth(index))
                    .unwrap_or("unknown");
                crate::log_event!(
                    "[hotkey] failed to register {action} chord (hotkey id {id}): {error}"
                );
                // Tell the settings UI the chord is not actually live —
                // without this the panel keeps showing it as an active
                // binding while the OS silently drops every press.
                if let Some(app) = app.as_ref() {
                    let _ = app.emit(
                        "hotkey-registration-failed",
                        HotkeyRegistrationFailure {
                            action,
                            error: error.clone(),
                        },
                    );
                }
                continue;
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

        // Continuous foreground tracking so "copy and paste" also works
        // when no hotkey or tray toggle recorded a target first: a resident
        // float panel clicked with the mouse already owns the focus by the
        // time the click lands, so only a watcher can know where the user
        // came from. SKIPOWNPROCESS keeps our own windows out, and the
        // target stays take-once, so a stale entry degrades to the
        // historical copy-only toast instead of a bogus paste.
        let foreground_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            0,
            foreground_hook_proc as *const () as usize,
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if foreground_hook == 0 {
            crate::log_event!(
                "[hotkey] failed to install the foreground watcher (Windows error {})",
                GetLastError()
            );
        }

        let mut msg: Msg = std::mem::zeroed();
        loop {
            let ret = GetMessageW(&mut msg, 0, 0, 0);
            if ret == 0 || ret == -1 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if foreground_hook != 0 {
            UnhookWinEvent(foreground_hook);
        }
        clear_foreground_paste_target();
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
                        tracker.on_key_event(event.virtual_key, is_key_down, event.time)
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
static FOREGROUND_TARGET: Mutex<Option<Arc<QuickPasteTarget>>> = Mutex::new(None);

/// WinEvent callback feeding the continuous foreground watcher. Runs on the
/// hotkey message-loop thread (OUTOFCONTEXT delivery), so it only locks the
/// shared target and never touches UI state.
unsafe extern "system" fn foreground_hook_proc(
    _hook: isize,
    _event: u32,
    hwnd: isize,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object != OBJID_WINDOW || hwnd == 0 {
        return;
    }
    if let Ok(guard) = FOREGROUND_TARGET.lock() {
        if let Some(target) = guard.as_ref() {
            target.remember(hwnd);
        }
    }
}

fn set_foreground_paste_target(target: &Arc<QuickPasteTarget>) {
    if let Ok(mut guard) = FOREGROUND_TARGET.lock() {
        *guard = Some(Arc::clone(target));
    }
}

fn clear_foreground_paste_target() {
    if let Ok(mut guard) = FOREGROUND_TARGET.lock() {
        *guard = None;
    }
}

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

/// Bounded wait covering the gap after a readiness-timeout spawn: the loop
/// thread may publish its hwnd after the spawn-side handshake gave up, and
/// until it does (or the thread exits on its error path) there is no hwnd to
/// post WM_QUIT to, while `join` would block forever on a thread stuck in
/// `GetMessageW`. Returns `false` when the wait expired — the caller must
/// then leak the thread instead of joining it.
fn wait_for_hotkey_hwnd_or_exit(handle: &thread::JoinHandle<()>) -> bool {
    const WAIT: std::time::Duration = std::time::Duration::from_secs(5);
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        if handle.is_finished() {
            return true;
        }
        let hwnd = HOTKEY_HWND.lock().map(|g| *g).unwrap_or(0);
        if hwnd != 0 {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        thread::sleep(std::time::Duration::from_millis(25));
    }
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
    /// Chord bindings per global action in `global_action_ids()` order
    /// (index 0 = `toggleWindow`, 1 = `toggleFloatPanel`). A new registry row
    /// automatically extends this vector, so new global shortcuts need no
    /// manager edits — only the plan builder in `lib.rs` reads the registry.
    global_chords: Vec<Vec<(u32, u32)>>,
    toggle_doubles: Vec<Modifier>,
    app: Option<tauri::AppHandle>,
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
            global_chords: Vec::new(),
            toggle_doubles: Vec::new(),
            app: None,
            quick_paste_target: Arc::new(QuickPasteTarget::default()),
        }
    }

    fn ensure_chord_slots(&mut self) {
        let want = global_action_ids().count();
        if self.global_chords.len() < want {
            self.global_chords.resize(want, Vec::new());
        }
    }

    /// Sets chord bindings for any registry action by id and rebuilds the
    /// shared loop. This is the only method new global actions need.
    pub fn set_action_chords(&mut self, action: &str, bindings: Vec<(u32, u32)>) {
        self.ensure_chord_slots();
        if let Some(index) = global_action_ids().position(|id| id == action) {
            self.global_chords[index] = bindings;
        }
        self.restart_combined_thread();
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
        self.ensure_chord_slots();
        if let Some(slot) = self.global_chords.first_mut() {
            *slot = bindings;
        }
        self.toggle_doubles = double_modifiers;
        self.window = Some(window.clone());
        self.restart_combined_thread();
    }

    /// Registers float-panel chord bindings on the same shared message loop.
    /// The app handle opens the backend-owned float window without involving
    /// any frontend state; it also forwards future `Forward` actions as
    /// `global-hotkey` events.
    pub fn set_float_hotkeys(&mut self, bindings: Vec<(u32, u32)>, app: tauri::AppHandle) {
        self.ensure_chord_slots();
        if let Some(slot) = self.global_chords.get_mut(1) {
            *slot = bindings;
        }
        self.app = Some(app);
        self.restart_combined_thread();
    }

    /// Starts the shared loop from a full registry-ordered chord plan in one
    /// rebuild. Used once at startup so every global action registers
    /// together instead of restarting the loop per action.
    pub fn start_with_plan(
        &mut self,
        chords: Vec<Vec<(u32, u32)>>,
        double_modifiers: Vec<Modifier>,
        window: tauri::WebviewWindow,
        app: tauri::AppHandle,
    ) {
        self.global_chords = chords;
        self.ensure_chord_slots();
        self.toggle_doubles = double_modifiers;
        self.window = Some(window);
        self.app = Some(app);
        self.restart_combined_thread();
    }

    /// Applies a full registry-ordered chord plan plus the app handle in one
    /// rebuild. Used by `refresh_hotkey_registrations` after any keyboard
    /// config change; without a window the loop stays stopped.
    pub fn apply_global_plan(
        &mut self,
        chords: Vec<Vec<(u32, u32)>>,
        double_modifiers: Vec<Modifier>,
        app: tauri::AppHandle,
    ) {
        self.global_chords = chords;
        self.ensure_chord_slots();
        self.toggle_doubles = double_modifiers;
        self.app = Some(app);
        self.restart_combined_thread();
    }

    /// (Re)builds the single hotkey thread from every registry action's
    /// stored chords. Stopping first tears down the previous message window,
    /// so re-registration never leaks hotkeys.
    fn restart_combined_thread(&mut self) {
        self.stop();
        let Some(window) = self.window.clone() else {
            return;
        };
        self.ensure_chord_slots();
        let slices: Vec<&[(u32, u32)]> = self.global_chords.iter().map(Vec::as_slice).collect();
        let plan = plan_registrations(&slices);
        if plan.is_empty() && self.toggle_doubles.is_empty() {
            return;
        }
        let registrations: Vec<HotkeyRegistration> = plan
            .iter()
            .map(|registration| {
                (
                    registration.id,
                    registration.modifiers,
                    registration.virtual_key,
                )
            })
            .collect();
        let (tx, rx) = mpsc::channel::<HotkeyAction>();
        let handle = spawn_hotkey_thread_with_registrations(
            registrations,
            self.toggle_doubles.clone(),
            tx,
            self.app.clone(),
            Arc::clone(&self.quick_paste_target),
        );
        let quick_paste_target = Arc::clone(&self.quick_paste_target);
        let app = self.app.clone();

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
                        if let Some(app) = app.as_ref() {
                            // Mirror ToggleMain: remember the foreground window
                            // as the quick-paste target so "copy and paste"
                            // from the float panel has a window to restore.
                            // Skip when one of our own windows is focused
                            // (the user is toggling the panel away, and its
                            // handle must not become the paste target).
                            let own_focused = window.is_focused().unwrap_or(false)
                                || app
                                    .get_webview_window(crate::commands::float::FLOAT_WINDOW_LABEL)
                                    .and_then(|panel| panel.is_focused().ok())
                                    .unwrap_or(false);
                            if !own_focused {
                                if let Some(window_handle) = foreground_window_handle() {
                                    quick_paste_target.remember(window_handle);
                                }
                            }
                            if let Err(error) =
                                crate::commands::float::toggle_float_panel(app.clone())
                            {
                                crate::log_event!(
                                    "[hotkey] failed to toggle the float panel: {error}"
                                );
                            }
                        }
                    }
                    HotkeyAction::Forward(index) => {
                        // Future global actions without a native handler are
                        // forwarded as events; listeners need no manager code.
                        let action_id = global_action_ids().nth(index).unwrap_or("unknown");
                        if let Some(app) = app.as_ref() {
                            let _ = app.emit("global-hotkey", action_id);
                        } else {
                            crate::log_event!(
                                "[hotkey] no app handle to forward global action {action_id}"
                            );
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
        let handle = self.handle.take();
        // The spawn-side readiness wait may time out before the loop thread
        // publishes its hwnd (pathologically slow window creation). Then
        // `stop_hotkey_thread` has no hwnd to post WM_QUIT to and the join
        // below would block forever on a thread stuck in GetMessageW. Wait
        // bounded for the hwnd to appear — or the thread to exit on its own
        // error path — before asking the stop to run; if even that expires,
        // leak the thread so shutdown still proceeds.
        let joinable = handle
            .as_ref()
            .map(wait_for_hotkey_hwnd_or_exit)
            .unwrap_or(true);
        stop_hotkey_thread();
        if let Some(handle) = handle {
            if joinable {
                if let Err(panic) = handle.join() {
                    crate::log_event!(
                        "[hotkey] message loop thread terminated with a panic: {panic:?}"
                    );
                }
            } else {
                crate::log_event!(
                    "[hotkey] message loop thread never became stoppable; leaking it so shutdown can proceed"
                );
            }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::keyboard::Modifier;
    use crate::platform::hotkey_common::action_id_base;

    use super::{
        action_for_hotkey_id, clear_foreground_paste_target, foreground_hook_proc,
        set_foreground_paste_target, DoubleModifierTracker, HotkeyAction, QuickPasteTarget,
        EVENT_SYSTEM_FOREGROUND, FIRST_HOTKEY_ID, FLOAT_HOTKEY_ID_BASE, OBJID_WINDOW,
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
    fn foreground_hook_records_window_objects_only() {
        let shared = Arc::new(QuickPasteTarget::default());
        set_foreground_paste_target(&shared);
        unsafe {
            foreground_hook_proc(0, EVENT_SYSTEM_FOREGROUND, 77, OBJID_WINDOW, 0, 0, 0);
        }
        assert_eq!(shared.take(), Some(77));
        unsafe {
            // Non-window objects and null handles must not become targets.
            foreground_hook_proc(0, EVENT_SYSTEM_FOREGROUND, 78, 1, 0, 0, 0);
            foreground_hook_proc(0, EVENT_SYSTEM_FOREGROUND, 0, OBJID_WINDOW, 0, 0, 0);
        }
        assert_eq!(shared.take(), None);
        clear_foreground_paste_target();
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
        // A third registry action forwards without manager changes.
        assert_eq!(
            action_for_hotkey_id(action_id_base(2)),
            HotkeyAction::Forward(2)
        );
        // Unknown ids fail toward the main toggle, never toward float.
        assert_eq!(action_for_hotkey_id(0), HotkeyAction::ToggleMain);
        assert_eq!(action_for_hotkey_id(-3), HotkeyAction::ToggleMain);
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
    fn tracker_fires_when_the_tick_counter_wraps() {
        let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

        assert!(!tracker.on_key_event(VK_SHIFT, true, u32::MAX - 250));
        assert!(!tracker.on_key_event(VK_SHIFT, false, u32::MAX - 200));
        assert!(!tracker.on_key_event(VK_SHIFT, true, u32::MAX - 50));
        assert!(tracker.on_key_event(VK_SHIFT, false, 0));
    }

    #[test]
    fn tracker_ignores_taps_outside_the_interval_across_the_wrap() {
        let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

        tracker.on_key_event(VK_SHIFT, true, u32::MAX - 250);
        tracker.on_key_event(VK_SHIFT, false, u32::MAX - 200);
        // One second after the wrapped first tap: outside the 300 ms window.
        tracker.on_key_event(VK_SHIFT, true, 800);
        assert!(!tracker.on_key_event(VK_SHIFT, false, 850));
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
