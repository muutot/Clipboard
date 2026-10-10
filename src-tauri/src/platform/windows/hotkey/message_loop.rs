//! The `RegisterHotKey` message window, its loop thread, and the stop handshake.

use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use tauri::Emitter as _;

use crate::keyboard::{global_action_ids, Modifier};
use crate::platform::hotkey_common::action_index_for_hotkey_id;
use crate::platform::windows::{register_global_hotkey, unregister_global_hotkey};

use super::double_tap::{
    clear_double_modifier_tracker, keyboard_hook_proc, set_double_modifier_tracker, WH_KEYBOARD_LL,
};
use super::paste_target::{
    clear_foreground_paste_target, foreground_hook_proc, set_foreground_paste_target,
    QuickPasteTarget, EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};
use super::{action_for_hotkey_id, HotkeyAction, HotkeyRegistration, HotkeyRegistrationFailure};

const WM_HOTKEY: u32 = 0x0312;

pub(super) fn spawn_hotkey_thread_with_registrations(
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
            crate::log_error!("[hotkey] message loop exited with error: {error}");
        }
        drop(tx);
    });
    if ready_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .is_err()
    {
        crate::log_warn!("[hotkey] message loop did not signal readiness; a later stop may block");
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
            if let Err(error) = register_global_hotkey(hwnd, *id, *modifiers, *vk) {
                // A single occupied chord (another app already owns that
                // shortcut) must not take down every other action sharing
                // this loop: skip that chord and keep serving the rest.
                let action = action_index_for_hotkey_id(*id)
                    .and_then(|index| global_action_ids().nth(index))
                    .unwrap_or("unknown");
                crate::log_error!(
                    "[hotkey] failed to register {action} chord (hotkey id {id}): {error}"
                );
                // Tell the settings UI the chord is not actually live —
                // without this the panel keeps showing it as an active
                // binding while the OS silently drops every press.
                if let Some(app) = app.as_ref() {
                    if let Err(error) = app.emit(
                        "hotkey-registration-failed",
                        HotkeyRegistrationFailure {
                            action,
                            error: error.clone(),
                        },
                    ) {
                        crate::log_error!("[hotkey] failed to emit registration-failed: {error}");
                    }
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
                crate::log_error!(
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
            crate::log_error!(
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
            let _ = unregister_global_hotkey(hwnd, id);
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

pub(super) static HOTKEY_SENDER: Mutex<Option<mpsc::Sender<HotkeyAction>>> = Mutex::new(None);
static HOTKEY_HWND: Mutex<isize> = Mutex::new(0);

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
pub(super) fn wait_for_hotkey_hwnd_or_exit(handle: &thread::JoinHandle<()>) -> bool {
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
