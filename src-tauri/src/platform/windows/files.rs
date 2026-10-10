//! CF_HDROP file-path reads.

use super::*;
#[cfg(target_os = "windows")]
pub fn read_clipboard_file_paths() -> Vec<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn DragQueryFileW(hdrop: isize, index: u32, buffer: *mut u16, max_count: u32) -> u32;
    }

    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP) == 0 {
            return vec![];
        }

        if !open_clipboard_with_retry() {
            return vec![];
        }

        // `GetClipboardData(CF_HDROP)` already returns the HDROP handle that
        // `DragQueryFileW` expects. It must not be GlobalLock'd: that yields a
        // pointer to the DROPFILES structure, not the handle, and would also
        // make the subsequent GlobalUnlock operate on the wrong value.
        let handle = GetClipboardData(CF_HDROP);
        if handle == 0 {
            CloseClipboard();
            return vec![];
        }

        let file_count = DragQueryFileW(handle, 0xFFFFFFFF, std::ptr::null_mut(), 0);
        let mut paths = Vec::new();

        for i in 0..file_count {
            // Query the required length first. `DragQueryFileW` returns the
            // required buffer size (excluding the NUL) when the buffer is too
            // small, so a fixed 520-unit buffer would index out of bounds for
            // a long (`\\?\`) path and abort the process under `panic=abort`.
            let len = DragQueryFileW(handle, i, std::ptr::null_mut(), 0);
            if len == 0 {
                continue;
            }
            let mut buffer = vec![0u16; len as usize + 1];
            let copied = DragQueryFileW(handle, i, buffer.as_mut_ptr(), buffer.len() as u32);
            if copied > 0 {
                let wide: Vec<u16> = buffer[..copied as usize].to_vec();
                paths.push(OsString::from_wide(&wide).to_string_lossy().to_string());
            }
        }

        CloseClipboard();
        paths
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_file_paths() -> Vec<String> {
    vec![]
}
