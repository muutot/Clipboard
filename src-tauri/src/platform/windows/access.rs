//! Open/close plumbing, self-trigger format registration and
//! global-memory allocation helpers shared by the read/write paths.

use super::*;

#[cfg(target_os = "windows")]
pub(super) fn clipboard_format_available(format: u32) -> bool {
    extern "system" {
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }
    unsafe { IsClipboardFormatAvailable(format) != 0 }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn clipboard_format_available(_format: u32) -> bool {
    false
}

pub(super) fn has_self_trigger_format() -> bool {
    if let Some(format) = self_trigger_format_id() {
        return clipboard_format_available(format);
    }
    false
}

#[cfg(target_os = "windows")]
pub(super) fn self_trigger_format_id() -> Option<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }

    static FORMAT_ID: OnceLock<Option<u32>> = OnceLock::new();
    *FORMAT_ID.get_or_init(|| {
        let name = OsStr::new(SELF_TRIGGER_FORMAT_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        (format != 0).then_some(format)
    })
}

#[cfg(not(target_os = "windows"))]
pub(super) fn self_trigger_format_id() -> Option<u32> {
    None
}

#[cfg(target_os = "windows")]
pub(super) fn read_self_trigger_marker() -> Option<Vec<u8>> {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn GlobalLock(handle: isize) -> *const u8;
        fn GlobalUnlock(handle: isize) -> i32;
        fn GlobalSize(handle: isize) -> usize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    let format = self_trigger_format_id()?;
    unsafe {
        if IsClipboardFormatAvailable(format) == 0 || OpenClipboard(0) == 0 {
            return None;
        }

        let handle = GetClipboardData(format);
        if handle == 0 {
            CloseClipboard();
            return None;
        }

        let size = GlobalSize(handle);
        if !native_payload_size_allowed(size, MAX_SELF_TRIGGER_BYTES) {
            CloseClipboard();
            return None;
        }

        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }

        let marker = std::slice::from_raw_parts(ptr, size).to_vec();
        GlobalUnlock(handle);
        CloseClipboard();
        Some(marker)
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn read_self_trigger_marker() -> Option<Vec<u8>> {
    None
}

// Owns nothing itself; `SetClipboardData` transfers ownership of successful
// allocations to the system and the guard only closes the clipboard so the
// next `EmptyClipboard` releases them.
#[cfg(target_os = "windows")]
#[link(name = "User32")]
extern "system" {
    pub(super) fn OpenClipboard(window: isize) -> i32;
    pub(super) fn CloseClipboard() -> i32;
    pub(super) fn EmptyClipboard() -> i32;
    pub(super) fn SetClipboardData(format: u32, memory: isize) -> isize;
}

#[cfg(target_os = "windows")]
#[link(name = "Kernel32")]
extern "system" {
    pub(super) fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    pub(super) fn GlobalFree(memory: isize) -> isize;
    pub(super) fn GlobalLock(memory: isize) -> *const u8;
    pub(super) fn GlobalUnlock(memory: isize) -> i32;
}

#[cfg(target_os = "windows")]
pub(super) struct ClipboardGuard;

#[cfg(target_os = "windows")]
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

/// Opens the clipboard with a bounded retry. Other applications can hold the
/// clipboard open for short windows (MSDN explicitly recommends retrying):
/// a single failed `OpenClipboard` would permanently miss that content on
/// the capture path and fail pastes spuriously.
#[cfg(target_os = "windows")]
pub(super) fn open_clipboard_with_retry() -> bool {
    for _ in 0..20 {
        if unsafe { OpenClipboard(0) } != 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

#[cfg(target_os = "windows")]
pub(super) fn allocate_global_bytes(bytes: &[u8]) -> Result<isize, String> {
    const GMEM_MOVEABLE: u32 = 0x0002;
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) };
    if memory == 0 {
        return Err("failed to allocate clipboard memory".to_owned());
    }
    let target = unsafe { GlobalLock(memory) }.cast_mut();
    if target.is_null() {
        unsafe { GlobalFree(memory) };
        return Err("failed to lock clipboard memory".to_owned());
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len()) };
    unsafe { GlobalUnlock(memory) };
    Ok(memory)
}
