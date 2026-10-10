//! Quick-paste foreground target tracking and Win32 `Ctrl+V` synthesis.

use std::sync::{Arc, Mutex};
use std::thread;
#[cfg(target_os = "windows")]
use std::time::Duration;

#[cfg(target_os = "windows")]
const QUICK_PASTE_FOCUS_DELAY: Duration = Duration::from_millis(60);
pub(super) const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
pub(super) const OBJID_WINDOW: i32 = 0;
pub(super) const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
pub(super) const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

#[derive(Default)]
pub(super) struct QuickPasteTarget {
    window_handle: Mutex<Option<isize>>,
}

impl QuickPasteTarget {
    pub(super) fn remember(&self, window_handle: isize) {
        if window_handle == 0 {
            return;
        }
        if let Ok(mut target) = self.window_handle.lock() {
            *target = Some(window_handle);
        }
    }

    pub(super) fn take(&self) -> Option<isize> {
        self.window_handle
            .lock()
            .ok()
            .and_then(|mut target| target.take())
    }

    pub(super) fn clear(&self) {
        if let Ok(mut target) = self.window_handle.lock() {
            *target = None;
        }
    }
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
