use std::collections::HashSet;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
#[cfg(target_os = "windows")]
use std::time::Duration;

use super::windows_clipboard;

const WM_HOTKEY: u32 = 0x0312;
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

fn deduplicate_hotkeys(bindings: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut seen = HashSet::new();
    bindings
        .iter()
        .copied()
        .filter(|binding| seen.insert(*binding))
        .collect()
}

fn assign_hotkey_ids(bindings: &[(u32, u32)]) -> Vec<HotkeyRegistration> {
    deduplicate_hotkeys(bindings)
        .into_iter()
        .enumerate()
        .map(|(index, (modifiers, vk))| (FIRST_HOTKEY_ID + index as i32, modifiers, vk))
        .collect()
}

pub fn spawn_hotkey_thread(
    hotkey_id: i32,
    modifiers: u32,
    vk: u32,
    tx: mpsc::Sender<()>,
) -> thread::JoinHandle<()> {
    spawn_hotkey_thread_with_registrations(vec![(hotkey_id, modifiers, vk)], tx)
}

pub fn spawn_hotkey_thread_with_bindings(
    bindings: Vec<(u32, u32)>,
    tx: mpsc::Sender<()>,
) -> thread::JoinHandle<()> {
    spawn_hotkey_thread_with_registrations(assign_hotkey_ids(&bindings), tx)
}

fn spawn_hotkey_thread_with_registrations(
    registrations: Vec<HotkeyRegistration>,
    tx: mpsc::Sender<()>,
) -> thread::JoinHandle<()> {
    set_hotkey_sender(&tx);
    thread::spawn(move || {
        let result = hotkey_message_loop(&registrations);
        clear_hotkey_state();
        if let Err(error) = result {
            eprintln!("[hotkey] message loop exited with error: {error}");
        }
        drop(tx);
    })
}

fn hotkey_message_loop(registrations: &[HotkeyRegistration]) -> Result<(), String> {
    if registrations.is_empty() {
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
            return Err("CreateWindowExW failed".to_string());
        }

        set_hotkey_hwnd(hwnd);

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
                return Err(error);
            }
            registered_ids.push(*id);
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
            let _ = tx.send(());
        }
        return 0;
    }

    extern "system" {
        fn DefWindowProcW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

static HOTKEY_SENDER: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);
static HOTKEY_HWND: Mutex<isize> = Mutex::new(0);

pub fn set_hotkey_sender(tx: &mpsc::Sender<()>) {
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
            quick_paste_target: Arc::new(QuickPasteTarget::default()),
        }
    }

    pub fn start_with_window(&mut self, modifiers: u32, vk: u32, window: tauri::WebviewWindow) {
        self.start_with_bindings(vec![(modifiers, vk)], window);
    }

    pub fn start_with_bindings(&mut self, bindings: Vec<(u32, u32)>, window: tauri::WebviewWindow) {
        self.stop();
        self.window = Some(window.clone());
        let (tx, rx) = mpsc::channel::<()>();
        let handle = spawn_hotkey_thread_with_bindings(bindings, tx);
        let quick_paste_target = Arc::clone(&self.quick_paste_target);

        thread::spawn(move || {
            while let Ok(()) = rx.recv() {
                let is_visible = window.is_visible().unwrap_or(false);
                let is_focused = window.is_focused().unwrap_or(false);
                if !is_focused {
                    if let Some(window_handle) = foreground_window_handle() {
                        quick_paste_target.remember(window_handle);
                    }
                }
                if !is_visible {
                    let _ = window.show();
                }
                if !is_focused {
                    let _ = window.set_focus();
                }
            }
        });

        self.handle = Some(handle);
    }

    pub fn restart(&mut self, modifiers: u32, vk: u32) {
        self.restart_with_bindings(vec![(modifiers, vk)]);
    }

    pub fn restart_with_bindings(&mut self, bindings: Vec<(u32, u32)>) {
        if let Some(window) = self.window.clone() {
            self.start_with_bindings(bindings, window);
        }
    }

    pub fn take_quick_paste_target(&self) -> Option<isize> {
        self.quick_paste_target.take()
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
        if IsIconic(window_handle) != 0 {
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
fn send_ctrl_v() -> Result<(), String> {
    const INPUT_KEYBOARD: u32 = 1;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const VK_CONTROL: u16 = 0x11;
    const VK_V: u16 = 0x56;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeyboardInput {
        virtual_key: u16,
        scan_code: u16,
        flags: u32,
        time: u32,
        extra_info: usize,
    }

    #[repr(C)]
    union InputData {
        keyboard: KeyboardInput,
    }

    #[repr(C)]
    struct Input {
        input_type: u32,
        data: InputData,
    }

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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::keyboard::ShortcutBinding;

    use super::{
        assign_hotkey_ids, shortcut_bindings_to_windows_hotkeys, shortcut_to_windows_hotkey,
        QuickPasteTarget, FIRST_HOTKEY_ID,
    };

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
    fn double_modifier_bindings_are_left_for_the_software_matcher() {
        let binding = ShortcutBinding::from_str("Shift+Shift").unwrap();
        assert_eq!(shortcut_to_windows_hotkey(&binding), None);
    }
}
