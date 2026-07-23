use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub const CF_UNICODETEXT: u32 = 13;

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

pub const VK_V: u32 = 0x56;

pub struct WindowsClipboardMonitor {
    running: bool,
    ignored_apps: Vec<String>,
    last_sequence: u32,
    sender: Option<mpsc::Sender<ClipboardChange>>,
}

#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub sequence: u32,
    pub formats: Vec<String>,
}

impl WindowsClipboardMonitor {
    pub fn new() -> Self {
        Self {
            running: false,
            ignored_apps: vec![],
            last_sequence: 0,
            sender: None,
        }
    }

    pub fn start(&mut self) -> Result<mpsc::Receiver<ClipboardChange>, String> {
        if self.running {
            return Err("clipboard monitor is already running".to_string());
        }

        let (sender, receiver) = mpsc::channel();
        self.sender = Some(sender.clone());
        self.running = true;

        let _ignored = self.ignored_apps.clone();

        thread::spawn(move || {
            let mut sequence = 0u32;
            loop {
                thread::sleep(Duration::from_millis(300));

                let current_sequence = match read_clipboard_sequence() {
                    Some(seq) => seq,
                    None => continue,
                };

                if current_sequence == sequence {
                    continue;
                }
                sequence = current_sequence;

                let formats = list_clipboard_formats();

                if sender
                    .send(ClipboardChange {
                        sequence,
                        formats,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        Ok(receiver)
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.sender = None;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        self.ignored_apps = apps;
    }
}

#[cfg(target_os = "windows")]
fn read_clipboard_sequence() -> Option<u32> {
    extern "system" {
        fn GetClipboardSequenceNumber() -> u32;
    }

    unsafe {
        let seq = GetClipboardSequenceNumber();
        if seq == 0 {
            None
        } else {
            Some(seq)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn read_clipboard_sequence() -> Option<u32> {
    None
}

#[cfg(target_os = "windows")]
fn list_clipboard_formats() -> Vec<String> {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn EnumClipboardFormats(format: u32) -> u32;
        fn GetClipboardFormatNameW(format: u32, name: *mut u16, max_count: i32) -> i32;
    }

    let mut formats = Vec::new();

    unsafe {
        if OpenClipboard(0) == 0 {
            eprintln!("[clipboard] OpenClipboard failed");
            return formats;
        }

        let mut format = 0u32;
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 {
                break;
            }

            let name = format_id_to_name(format);
            formats.push(name);
        }

        CloseClipboard();
    }

    formats
}

#[cfg(not(target_os = "windows"))]
fn list_clipboard_formats() -> Vec<String> {
    vec![]
}

#[cfg(target_os = "windows")]
fn format_id_to_name(format_id: u32) -> String {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let predefined: &[(u32, &str)] = &[
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
    ];

    if let Some(&(_, name)) = predefined.iter().find(|&&(id, _)| id == format_id) {
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
fn format_id_to_name(_format_id: u32) -> String {
    String::new()
}

#[cfg(target_os = "windows")]
pub fn read_clipboard_text() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn GlobalLock(handle: isize) -> *const u16;
        fn GlobalUnlock(handle: isize) -> i32;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }

        if OpenClipboard(0) == 0 {
            return None;
        }

        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle == 0 {
            CloseClipboard();
            return None;
        }

        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }

        let len = (0..).take_while(|&i| *ptr.add(i) != 0).count();
        let wide: Vec<u16> = std::slice::from_raw_parts(ptr, len).to_vec();
        GlobalUnlock(handle);
        CloseClipboard();

        Some(OsString::from_wide(&wide).to_string_lossy().to_string())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_text() -> Option<String> {
    None
}

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
pub fn register_global_hotkey(_hwnd: isize, _id: i32, _modifiers: u32, _vk: u32) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_starts_and_stops() {
        let mut monitor = WindowsClipboardMonitor::new();
        assert!(!monitor.is_running());

        let result = monitor.start();
        assert!(result.is_ok());
        assert!(monitor.is_running());

        monitor.stop();
        assert!(!monitor.is_running());
    }

    #[test]
    fn double_start_returns_error() {
        let mut monitor = WindowsClipboardMonitor::new();
        let _ = monitor.start().ok();
        let result = monitor.start();
        assert!(result.is_err());
        monitor.stop();
    }

    #[test]
    fn format_name_lookup() {
        assert_eq!(format_id_to_name(1), "CF_TEXT");
        assert_eq!(format_id_to_name(13), "CF_UNICODETEXT");
        assert_eq!(format_id_to_name(15), "CF_HDROP");
    }

    #[test]
    fn unknown_format_gets_fallback_name() {
        assert!(format_id_to_name(99999).starts_with("format_"));
    }
}
