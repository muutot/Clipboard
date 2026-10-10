//! CF_UNICODETEXT reads plus the registered-format diagnostic helper.

use super::*;
#[cfg(target_os = "windows")]
pub(super) fn format_id_to_name(format_id: u32) -> String {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::sync::LazyLock;

    static PREDEFINED: LazyLock<BTreeMap<u32, &'static str>> = LazyLock::new(|| {
        BTreeMap::from([
            (1, "CF_TEXT"),
            (2, "CF_BITMAP"),
            (3, "CF_METAFILEPICT"),
            (4, "CF_SYLK"),
            (5, "CF_DIF"),
            (6, "CF_TIFF"),
            (7, "CF_OEMTEXT"),
            (8, "CF_DIB"),
            (9, "CF_PALETTE"),
            (10, "CF_PENDATA"),
            (11, "CF_RIFF"),
            (12, "CF_WAVE"),
            (13, "CF_UNICODETEXT"),
            (14, "CF_ENHMETAFILE"),
            (15, "CF_HDROP"),
            (16, "CF_LOCALE"),
            (17, "CF_DIBV5"),
            (128, "CF_OWNERDISPLAY"),
            (129, "CF_DSPTEXT"),
            (130, "CF_DSPBITMAP"),
            (131, "CF_DSPMETAFILEPICT"),
            (132, "CF_DSPENHMETAFILE"),
        ])
    });

    if let Some(&name) = PREDEFINED.get(&format_id) {
        return name.to_string();
    }

    extern "system" {
        fn GetClipboardFormatNameW(format: u32, name: *mut u16, max_count: i32) -> i32;
    }

    unsafe {
        let mut buffer = [0u16; 256];
        let len = GetClipboardFormatNameW(format_id, buffer.as_mut_ptr(), 256);
        if len > 0 {
            let wide: Vec<u16> = buffer[..len as usize].to_vec();
            return OsString::from_wide(&wide).to_string_lossy().to_string();
        }
    }

    format!("format_{}", format_id)
}

#[cfg(not(target_os = "windows"))]
pub(super) fn format_id_to_name(_format_id: u32) -> String {
    String::new()
}

#[cfg(target_os = "windows")]
pub fn read_clipboard_text() -> Option<String> {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn GlobalLock(handle: isize) -> *const u8;
        fn GlobalSize(handle: isize) -> usize;
        fn GlobalUnlock(handle: isize) -> i32;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }

        if !open_clipboard_with_retry() {
            return None;
        }

        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle == 0 {
            CloseClipboard();
            return None;
        }

        let size = GlobalSize(handle);
        if !native_payload_size_allowed(size, MAX_NATIVE_TEXT_BYTES) || !size.is_multiple_of(2) {
            CloseClipboard();
            return None;
        }
        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }

        // Bound the NUL-terminator scan to the real allocation size so a
        // producer that omits the terminator cannot cause an out-of-bounds
        // read past the locked global memory block.
        let cap_u16 = size / size_of::<u16>();
        let len = (0..cap_u16).take_while(|&i| *ptr.add(i) != 0).count();
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        GlobalUnlock(handle);
        CloseClipboard();

        Some(text)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_text() -> Option<String> {
    None
}
