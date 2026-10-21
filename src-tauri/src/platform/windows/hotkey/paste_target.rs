//! Quick-paste foreground target tracking and Win32 `Ctrl+V` synthesis.

use std::sync::{Arc, Mutex};
use std::thread;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

/// Opaque quick-paste target handed from the hotkey watcher to the restore
/// routine. Carries the owning process id captured at remember time so the
/// restore can prove the handle was not recycled to another process while the
/// main window was hidden (mirrors `platform::quick_paste::Target`, which is
/// not compiled into Windows release builds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuickPasteHandle {
    pub window_handle: isize,
    pub process_id: u32,
}

impl std::fmt::UpperHex for QuickPasteHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::UpperHex::fmt(&self.window_handle, f)
    }
}

#[cfg(target_os = "windows")]
const QUICK_PASTE_FOCUS_TIMEOUT: Duration = Duration::from_millis(400);
const QUICK_PASTE_FOCUS_POLL: Duration = Duration::from_millis(15);
const QUICK_PASTE_MODIFIER_TIMEOUT: Duration = Duration::from_millis(400);
pub(super) const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
pub(super) const OBJID_WINDOW: i32 = 0;
pub(super) const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
pub(super) const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

#[derive(Default)]
pub(super) struct QuickPasteTarget {
    target: Mutex<Option<QuickPasteHandle>>,
}

impl QuickPasteTarget {
    pub(super) fn remember(&self, window_handle: isize) {
        if window_handle == 0 {
            return;
        }
        if let Ok(mut target) = self.target.lock() {
            *target = Some(QuickPasteHandle {
                window_handle,
                process_id: window_process_id(window_handle),
            });
        }
    }

    pub(super) fn take(&self) -> Option<QuickPasteHandle> {
        self.target.lock().ok().and_then(|mut target| target.take())
    }

    pub(super) fn clear(&self) {
        if let Ok(mut target) = self.target.lock() {
            *target = None;
        }
    }
}

#[cfg(target_os = "windows")]
fn window_process_id(window_handle: isize) -> u32 {
    extern "system" {
        fn GetWindowThreadProcessId(window: isize, process_id: *mut u32) -> u32;
    }
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(window_handle, &mut process_id);
    }
    process_id
}

#[cfg(not(target_os = "windows"))]
fn window_process_id(_window_handle: isize) -> u32 {
    0
}

static FOREGROUND_TARGET: Mutex<Option<Arc<QuickPasteTarget>>> = Mutex::new(None);

/// WinEvent callback feeding the continuous foreground watcher. Runs on the
/// hotkey message-loop thread (OUTOFCONTEXT delivery), so it only locks the
/// shared target and never touches UI state.
pub(super) unsafe extern "system" fn foreground_hook_proc(
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
    // A mouse-only window switch never reaches the keyboard hook, so drop any
    // pending double-modifier tap here; otherwise the two taps could still
    // satisfy the double-tap interval across the switch and misfire.
    super::double_tap::cancel_pending_double_tap();
    if let Ok(guard) = FOREGROUND_TARGET.lock() {
        if let Some(target) = guard.as_ref() {
            target.remember(hwnd);
        }
    }
}

pub(super) fn set_foreground_paste_target(target: &Arc<QuickPasteTarget>) {
    if let Ok(mut guard) = FOREGROUND_TARGET.lock() {
        *guard = Some(Arc::clone(target));
    }
}

pub(super) fn clear_foreground_paste_target() {
    if let Ok(mut guard) = FOREGROUND_TARGET.lock() {
        *guard = None;
    }
}

#[cfg(target_os = "windows")]
pub(super) fn foreground_window_handle() -> Option<isize> {
    extern "system" {
        fn GetForegroundWindow() -> isize;
    }

    let window_handle = unsafe { GetForegroundWindow() };
    (window_handle != 0).then_some(window_handle)
}

#[cfg(not(target_os = "windows"))]
pub(super) fn foreground_window_handle() -> Option<isize> {
    None
}

#[cfg(target_os = "windows")]
pub fn restore_window_and_paste(target: QuickPasteHandle) -> Result<(), String> {
    const SW_RESTORE: i32 = 9;

    extern "system" {
        fn IsWindow(window: isize) -> i32;
        fn IsIconic(window: isize) -> i32;
        fn ShowWindow(window: isize, command: i32) -> i32;
        fn BringWindowToTop(window: isize) -> i32;
        fn SetForegroundWindow(window: isize) -> i32;
        fn GetForegroundWindow() -> isize;
    }

    if target.window_handle == 0 || unsafe { IsWindow(target.window_handle) } == 0 {
        return Err("the previous foreground window is no longer available".to_owned());
    }
    // HWNDs are recycled by the OS: a window closed while the main panel was
    // hidden can be reused by an unrelated process. The owner pid captured at
    // remember time proves the handle still names the same window.
    if target.process_id != 0 && window_process_id(target.window_handle) != target.process_id {
        return Err("the previous window's owner changed".to_owned());
    }

    unsafe {
        let iconic = IsIconic(target.window_handle);
        if iconic != 0 {
            ShowWindow(target.window_handle, SW_RESTORE);
        }
        BringWindowToTop(target.window_handle);
        SetForegroundWindow(target.window_handle);
    }

    // Focus changes are asynchronous: poll until the target actually owns the
    // foreground instead of a single fixed sleep, which both slowed the common
    // case and still raced on slow focus transitions.
    let deadline = Instant::now() + QUICK_PASTE_FOCUS_TIMEOUT;
    while unsafe { GetForegroundWindow() } != target.window_handle {
        if Instant::now() >= deadline {
            return Err("failed to restore the previous foreground window".to_owned());
        }
        thread::sleep(QUICK_PASTE_FOCUS_POLL);
    }

    send_ctrl_v()
}

#[cfg(not(target_os = "windows"))]
pub fn restore_window_and_paste(_target: QuickPasteHandle) -> Result<(), String> {
    Err("quick paste is only implemented on Windows".to_owned())
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct MouseInput {
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
pub(super) struct KeyboardInput {
    virtual_key: u16,
    scan_code: u16,
    flags: u32,
    time: u32,
    extra_info: usize,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct HardwareInput {
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
pub(super) union InputData {
    mouse: MouseInput,
    keyboard: KeyboardInput,
    hardware: HardwareInput,
}

#[cfg(target_os = "windows")]
#[repr(C)]
pub(super) struct Input {
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
        fn GetAsyncKeyState(virtual_key: i32) -> i16;
        fn GetLastError() -> u32;
    }

    const VK_SHIFT: i32 = 0x10;
    const VK_CONTROL_MOD: i32 = 0x11;
    const VK_MENU: i32 = 0x12;
    const VK_LWIN: i32 = 0x5B;
    const VK_RWIN: i32 = 0x5C;
    let deadline = Instant::now() + QUICK_PASTE_MODIFIER_TIMEOUT;
    loop {
        let held = [VK_SHIFT, VK_CONTROL_MOD, VK_MENU, VK_LWIN, VK_RWIN]
            .into_iter()
            .any(|key| unsafe { GetAsyncKeyState(key) as u16 & 0x8000 != 0 });
        if !held {
            break;
        }
        if Instant::now() >= deadline {
            return Err("release keys before pasting".to_string());
        }
        thread::sleep(Duration::from_millis(10));
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
