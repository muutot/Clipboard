//! Global hotkey registration against a message-window handle.

#[cfg(target_os = "windows")]
pub fn register_global_hotkey(hwnd: isize, id: i32, modifiers: u32, vk: u32) -> Result<(), String> {
    extern "system" {
        fn RegisterHotKey(hwnd: isize, id: i32, modifiers: u32, vk: u32) -> i32;
    }

    let result = unsafe { RegisterHotKey(hwnd, id, modifiers, vk) };
    if result == 0 {
        Err(format!("RegisterHotKey failed for id={}", id))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn register_global_hotkey(
    _hwnd: isize,
    _id: i32,
    _modifiers: u32,
    _vk: u32,
) -> Result<(), String> {
    Err("global hotkey registration is not supported on this platform".to_string())
}

#[cfg(target_os = "windows")]
pub fn unregister_global_hotkey(hwnd: isize, id: i32) -> Result<(), String> {
    extern "system" {
        fn UnregisterHotKey(hwnd: isize, id: i32) -> i32;
    }

    let result = unsafe { UnregisterHotKey(hwnd, id) };
    if result == 0 {
        Err(format!("UnregisterHotKey failed for id={}", id))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn unregister_global_hotkey(_hwnd: isize, _id: i32) -> Result<(), String> {
    Err("global hotkey unregistration is not supported on this platform".to_string())
}
