#![allow(non_snake_case, dead_code)]

use std::sync::mpsc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(target_os = "windows")]
use crate::content::icon_key;

pub struct WindowsPlatform;

#[cfg(target_os = "windows")]
impl crate::platform::PlatformClipboard for WindowsPlatform {
    fn get_foreground_app(&self) -> crate::platform::ForegroundApp {
        get_foreground_app()
    }

    fn read_clipboard_text(&self) -> Option<String> {
        read_clipboard_text()
    }

    fn read_clipboard_html(&self) -> Option<String> {
        read_clipboard_html()
    }

    fn read_clipboard_rtf(&self) -> Option<String> {
        read_clipboard_rtf()
    }

    fn read_clipboard_sequence(&self) -> Option<u32> {
        read_clipboard_sequence()
    }

    fn read_clipboard_image(&self) -> Option<(Vec<u8>, u32, u32)> {
        read_clipboard_image()
    }

    fn read_clipboard_file_paths(&self) -> Vec<String> {
        read_clipboard_file_paths()
    }

    fn write_clipboard_text_with_self_trigger(&self, text: &str) -> Result<(), String> {
        write_clipboard_text_with_self_trigger(text)
    }

    fn write_clipboard_files_with_self_trigger(&self, paths: &[String]) -> Result<(), String> {
        write_clipboard_files_with_self_trigger(paths)
    }

    fn extract_app_icon(
        &self,
        icon_dir: &std::path::Path,
        app_name: &str,
        exe_path: &str,
    ) -> Option<String> {
        extract_app_icon(icon_dir, app_name, exe_path)
    }
}

pub const CF_UNICODETEXT: u32 = 13;
pub const CF_DIB: u32 = 8;
pub const CF_DIBV5: u32 = 17;
pub const CF_HDROP: u32 = 15;
pub const CF_BITMAP: u32 = 2;

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

const APP_ICON_SIZE: u32 = 32;
pub const SELF_TRIGGER_FORMAT_NAME: &str = "ClipboardDesktop.SelfTrigger.v1";

/// Encodes all hashes that the capture pipeline may derive from a text write.
/// Keeping the marker as a small private clipboard format lets a separate CLI
/// process tell the running monitor that the next change originated here.
pub fn self_trigger_marker_for_text(text: &str) -> Vec<u8> {
    crate::content::hash::compute_clipboard_write_hashes(text)
        .join("\n")
        .into_bytes()
}

pub fn clipboard_change_is_self_write(marker: &[u8], observed_text: &str) -> bool {
    let Ok(marker_text) = std::str::from_utf8(marker) else {
        return false;
    };
    let marker_hashes = marker_text.trim_matches('\0').split('\n');
    let expected_hashes = crate::content::hash::compute_clipboard_write_hashes(observed_text);
    expected_hashes
        .iter()
        .any(|expected| marker_hashes.clone().any(|marked| marked == expected))
}

pub(crate) fn normalize_app_icon(image: image::RgbaImage) -> image::RgbaImage {
    image::imageops::resize(
        &image,
        APP_ICON_SIZE,
        APP_ICON_SIZE,
        image::imageops::FilterType::Lanczos3,
    )
}

fn is_normalized_app_icon(path: &std::path::Path) -> bool {
    image::image_dimensions(path)
        .map(|(width, height)| width == APP_ICON_SIZE && height == APP_ICON_SIZE)
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
pub struct WindowsClipboardMonitor {
    running: bool,
    ignored_apps: Vec<String>,
    last_sequence: u32,
    sender: Option<mpsc::Sender<ClipboardChange>>,
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub sequence: u32,
}

#[cfg(target_os = "windows")]
impl Default for WindowsClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
impl WindowsClipboardMonitor {
    pub fn new() -> Self {
        Self {
            running: false,
            ignored_apps: vec![],
            last_sequence: 0,
            sender: None,
            stop_sender: None,
            handle: None,
        }
    }

    pub fn start(&mut self) -> Result<mpsc::Receiver<ClipboardChange>, String> {
        if self.running {
            // The monitor thread exits silently when the capture worker's
            // receiver is dropped (worker panic or spawn failure) — nothing
            // resets this flag. Detect the dead thread so a restart is not
            // permanently blocked by "already running".
            let thread_dead = self
                .handle
                .as_ref()
                .is_some_and(|handle| handle.is_finished());
            if !thread_dead {
                return Err("clipboard monitor is already running".to_string());
            }
            self.running = false;
            self.handle = None;
        }

        let (sender, receiver) = mpsc::channel();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let sender_for_thread = sender.clone();

        let handle = thread::Builder::new()
            .name("clipboard-monitor".to_owned())
            .spawn(move || {
                let mut sequence = 0u32;
                loop {
                    match stop_receiver.recv_timeout(Duration::from_millis(300)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }

                    let current_sequence = match read_clipboard_sequence() {
                        Some(seq) => seq,
                        None => continue,
                    };

                    if current_sequence == sequence {
                        continue;
                    }
                    sequence = current_sequence;

                    if has_self_trigger_format() {
                        if let (Some(marker), Some(text)) =
                            (read_self_trigger_marker(), read_clipboard_text())
                        {
                            if clipboard_change_is_self_write(&marker, &text) {
                                continue;
                            }
                        }
                    }

                    if sender_for_thread
                        .send(ClipboardChange { sequence })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|error| format!("failed to spawn clipboard monitor: {error}"))?;
        self.sender = Some(sender);
        self.stop_sender = Some(stop_sender);
        self.handle = Some(handle);
        self.running = true;

        Ok(receiver)
    }

    pub fn stop(&mut self) {
        self.running = false;
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        self.sender = None;
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                // A panicked monitor explains why captures silently stopped;
                // swallowing the payload hid that from every log.
                if let Err(panic) = handle.join() {
                    crate::log_event!(
                        "[clipboard-monitor] monitor thread terminated with a panic: {panic:?}"
                    );
                }
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        self.ignored_apps = apps;
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsClipboardMonitor {
    fn drop(&mut self) {
        self.stop();
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
fn clipboard_format_available(format: u32) -> bool {
    extern "system" {
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }
    unsafe { IsClipboardFormatAvailable(format) != 0 }
}

#[cfg(not(target_os = "windows"))]
fn clipboard_format_available(_format: u32) -> bool {
    false
}

fn has_self_trigger_format() -> bool {
    if let Some(format) = self_trigger_format_id() {
        return clipboard_format_available(format);
    }
    false
}

#[cfg(target_os = "windows")]
fn self_trigger_format_id() -> Option<u32> {
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
fn self_trigger_format_id() -> Option<u32> {
    None
}

#[cfg(target_os = "windows")]
fn read_self_trigger_marker() -> Option<Vec<u8>> {
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
        if size == 0 {
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
fn read_self_trigger_marker() -> Option<Vec<u8>> {
    None
}

// Owns nothing itself; `SetClipboardData` transfers ownership of successful
// allocations to the system and the guard only closes the clipboard so the
// next `EmptyClipboard` releases them.
#[cfg(target_os = "windows")]
#[link(name = "User32")]
extern "system" {
    fn OpenClipboard(window: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: isize) -> isize;
}

#[cfg(target_os = "windows")]
#[link(name = "Kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalFree(memory: isize) -> isize;
    fn GlobalLock(memory: isize) -> *const u8;
    fn GlobalUnlock(memory: isize) -> i32;
}

#[cfg(target_os = "windows")]
struct ClipboardGuard;

#[cfg(target_os = "windows")]
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

#[cfg(target_os = "windows")]
fn allocate_global_bytes(bytes: &[u8]) -> Result<isize, String> {
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

/// Writes CF_UNICODETEXT plus a private hash marker. Marker failures are
/// intentionally best-effort: the text write must retain its original
/// behavior even when a clipboard implementation rejects custom formats.
#[cfg(target_os = "windows")]
pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;

    let wide = OsStr::new(text)
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let wide_byte_len = wide
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| "clipboard text is too large".to_owned())?;
    let marker = self_trigger_marker_for_text(text);

    unsafe {
        if OpenClipboard(0) == 0 {
            return Err("failed to open the system clipboard".to_owned());
        }
        let _clipboard_guard = ClipboardGuard;

        if EmptyClipboard() == 0 {
            return Err("failed to clear the system clipboard".to_owned());
        }

        let wide_bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide_byte_len);
        let text_memory = allocate_global_bytes(wide_bytes)?;
        if SetClipboardData(CF_UNICODETEXT, text_memory) == 0 {
            GlobalFree(text_memory);
            return Err("failed to write text to the system clipboard".to_owned());
        }

        if let Some(format) = self_trigger_format_id() {
            if let Ok(marker_memory) = allocate_global_bytes(&marker) {
                if SetClipboardData(format, marker_memory) == 0 {
                    GlobalFree(marker_memory);
                }
            }
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn write_clipboard_text_with_self_trigger(_text: &str) -> Result<(), String> {
    Err("Windows clipboard text writing is not supported on this platform".to_owned())
}

/// Builds a `CF_HDROP` payload: a DROPFILES header (`pFiles`/`pt`/`fNC`/
/// `fWide`) followed by a double-NUL-terminated list of UTF-16LE paths.
#[cfg(target_os = "windows")]
fn build_drop_files_bytes(paths: &[String]) -> Result<Vec<u8>, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let mut utf16 = Vec::new();
    for path in paths {
        if path.is_empty() {
            return Err("no file paths to copy".to_owned());
        }
        utf16.extend(OsStr::new(path).encode_wide().flat_map(u16::to_le_bytes));
        utf16.extend_from_slice(&[0, 0]);
    }
    utf16.extend_from_slice(&[0, 0]);
    let mut header = Vec::with_capacity(20 + utf16.len());
    header.extend_from_slice(&20u32.to_le_bytes());
    header.extend_from_slice(&0i32.to_le_bytes());
    header.extend_from_slice(&0i32.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&utf16);
    Ok(header)
}

/// Writes `CF_HDROP` (dropped file references) plus the joined paths as
/// `CF_UNICODETEXT` and the private hash marker. Pasting into a file manager
/// copies the referenced files; text consumers still receive the path list.
#[cfg(target_os = "windows")]
pub fn write_clipboard_files_with_self_trigger(paths: &[String]) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    if paths.is_empty() {
        return Err("no file paths to copy".to_owned());
    }

    let buffer = build_drop_files_bytes(paths)?;

    let text = paths.join("\n");
    let marker = self_trigger_marker_for_text(&text);

    unsafe {
        if OpenClipboard(0) == 0 {
            return Err("failed to open the system clipboard".to_owned());
        }
        let _clipboard_guard = ClipboardGuard;

        if EmptyClipboard() == 0 {
            return Err("failed to clear the system clipboard".to_owned());
        }

        let drop_memory = allocate_global_bytes(&buffer)?;
        if SetClipboardData(CF_HDROP, drop_memory) == 0 {
            GlobalFree(drop_memory);
            return Err("failed to write files to the system clipboard".to_owned());
        }

        let wide = OsStr::new(&text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // The file list is the primary payload. Once CF_HDROP is in place
        // the user's previous clipboard contents are already gone, so a
        // failure in the text form must not abort with an error: the files
        // would still be pasteable while the caller reports failure. Treat
        // the text form as best-effort instead.
        if let Some(wide_byte_len) = wide.len().checked_mul(std::mem::size_of::<u16>()) {
            let wide_bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide_byte_len);
            if let Ok(text_memory) = allocate_global_bytes(wide_bytes) {
                if SetClipboardData(CF_UNICODETEXT, text_memory) == 0 {
                    GlobalFree(text_memory);
                }
            }
        }

        if let Some(format) = self_trigger_format_id() {
            if let Ok(marker_memory) = allocate_global_bytes(&marker) {
                if SetClipboardData(format, marker_memory) == 0 {
                    GlobalFree(marker_memory);
                }
            }
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn write_clipboard_files_with_self_trigger(_paths: &[String]) -> Result<(), String> {
    Err("Windows clipboard file writing is not supported on this platform".to_owned())
}

#[cfg(target_os = "windows")]
fn format_id_to_name(format_id: u32) -> String {
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
fn format_id_to_name(_format_id: u32) -> String {
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

        if OpenClipboard(0) == 0 {
            return None;
        }

        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle == 0 {
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
        let cap_u16 = GlobalSize(handle) / size_of::<u16>();
        let len = (0..cap_u16).take_while(|&i| *ptr.add(i) != 0).count();
        let wide: Vec<u16> = std::slice::from_raw_parts(ptr, len).to_vec();
        GlobalUnlock(handle);
        CloseClipboard();

        Some(String::from_utf16_lossy(&wide))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_text() -> Option<String> {
    None
}

/// The registered clipboard format name used by browsers and office apps to
/// expose the HTML fragment of a rich-text copy.
const CF_HTML_REGISTERED_NAME: &str = "HTML Format";

#[cfg(target_os = "windows")]
fn html_format_id() -> Option<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }

    static FORMAT_ID: OnceLock<Option<u32>> = OnceLock::new();
    *FORMAT_ID.get_or_init(|| {
        let name = OsStr::new(CF_HTML_REGISTERED_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        (format != 0).then_some(format)
    })
}

/// Extracts the HTML fragment from a CF_HTML payload.
///
/// CF_HTML is a text header followed by the markup; `StartFragment:` /
/// `EndFragment:` give byte offsets into the whole buffer. Headers use the
/// `StartFragment:0000000141` shape, so the label search includes the colon.
/// Falls back to `StartHTML:` / `EndHTML:` when the fragment offsets are
/// missing, and rejects empty results.
fn parse_cf_html(data: &[u8]) -> Option<String> {
    fn field_value(data: &[u8], label: &str) -> Option<usize> {
        let needle = label.as_bytes();
        let pos = data
            .windows(needle.len())
            .position(|window| window == needle)?;
        let rest = &data[pos + needle.len()..];
        let line_end = rest
            .iter()
            .position(|&byte| byte == b'\r' || byte == b'\n')
            .unwrap_or(rest.len());
        let value = rest[..line_end]
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .collect::<Vec<_>>();
        std::str::from_utf8(&value).ok()?.trim().parse().ok()
    }

    let (start, end) = match (
        field_value(data, "StartFragment:"),
        field_value(data, "EndFragment:"),
    ) {
        (Some(start), Some(end)) if end > start && end <= data.len() => (start, end),
        _ => {
            let start = field_value(data, "StartHTML:")?;
            let end = field_value(data, "EndHTML:")?;
            if end <= start || end > data.len() {
                return None;
            }
            (start, end)
        }
    };

    let fragment = String::from_utf8_lossy(&data[start..end]);
    let trimmed = fragment.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Reads the HTML fragment of a rich-text clipboard copy, if present.
#[cfg(target_os = "windows")]
pub fn read_clipboard_html() -> Option<String> {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn GlobalLock(handle: isize) -> *const u8;
        fn GlobalUnlock(handle: isize) -> i32;
        fn GlobalSize(handle: isize) -> usize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    let format = html_format_id()?;
    unsafe {
        if IsClipboardFormatAvailable(format) == 0 {
            return None;
        }

        if OpenClipboard(0) == 0 {
            return None;
        }

        let handle = GetClipboardData(format);
        if handle == 0 {
            CloseClipboard();
            return None;
        }

        let size = GlobalSize(handle);
        if size == 0 {
            CloseClipboard();
            return None;
        }

        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }

        let data = std::slice::from_raw_parts(ptr, size).to_vec();
        GlobalUnlock(handle);
        CloseClipboard();

        parse_cf_html(&data)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_html() -> Option<String> {
    None
}

/// The registered clipboard format name used by rich-text-capable apps
/// (Word, Outlook, browsers) to expose the RTF payload of a rich-text copy.
const CF_RTF_REGISTERED_NAME: &str = "Rich Text Format";

#[cfg(target_os = "windows")]
fn rtf_format_id() -> Option<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }

    static FORMAT_ID: OnceLock<Option<u32>> = OnceLock::new();
    *FORMAT_ID.get_or_init(|| {
        let name = OsStr::new(CF_RTF_REGISTERED_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        (format != 0).then_some(format)
    })
}

/// Reads the RTF payload of a rich-text clipboard copy, if present. RTF is
/// ASCII-superset text (`{\rtf1 ...}`) so the raw bytes decode lossily
/// without the fragment-header parsing CF_HTML requires.
#[cfg(target_os = "windows")]
pub fn read_clipboard_rtf() -> Option<String> {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn GlobalLock(handle: isize) -> *const u8;
        fn GlobalUnlock(handle: isize) -> i32;
        fn GlobalSize(handle: isize) -> usize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    let format = rtf_format_id()?;
    unsafe {
        if IsClipboardFormatAvailable(format) == 0 {
            return None;
        }

        if OpenClipboard(0) == 0 {
            return None;
        }

        let handle = GetClipboardData(format);
        if handle == 0 {
            CloseClipboard();
            return None;
        }

        let size = GlobalSize(handle);
        if size == 0 {
            CloseClipboard();
            return None;
        }

        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }

        let data = std::slice::from_raw_parts(ptr, size).to_vec();
        GlobalUnlock(handle);
        CloseClipboard();

        let rtf = String::from_utf8_lossy(&data);
        let trimmed = rtf.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_rtf() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn GlobalLock(handle: isize) -> *const u8;
        fn GlobalUnlock(handle: isize) -> i32;
        fn GlobalSize(handle: isize) -> usize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    unsafe {
        let has_dib = IsClipboardFormatAvailable(CF_DIB) != 0;
        let has_dibv5 = IsClipboardFormatAvailable(CF_DIBV5) != 0;
        let has_bitmap = IsClipboardFormatAvailable(CF_BITMAP) != 0;

        if !has_dib && !has_dibv5 && !has_bitmap {
            return None;
        }

        let format = if has_dibv5 {
            CF_DIBV5
        } else if has_dib {
            CF_DIB
        } else {
            CF_BITMAP
        };

        if OpenClipboard(0) == 0 {
            return None;
        }

        let handle = GetClipboardData(format);
        if handle == 0 {
            CloseClipboard();
            return None;
        }

        // CF_BITMAP yields an HBITMAP, not an HGLOBAL, so GlobalSize/GlobalLock
        // cannot read it. Convert it to a DIB with GetDIBits and reuse the DIB
        // decoder; a plain DIB/DIBV5 still takes the shared-memory path.
        let result = if format == CF_BITMAP {
            hbitmap_to_dib_bytes(handle).and_then(|dib| dib_to_png(&dib))
        } else {
            let size = GlobalSize(handle);
            if size == 0 {
                CloseClipboard();
                return None;
            }
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                CloseClipboard();
                return None;
            }
            let data = std::slice::from_raw_parts(ptr, size).to_vec();
            GlobalUnlock(handle);
            dib_to_png(&data)
        };
        CloseClipboard();
        result
    }
}

/// Converts an `HBITMAP` into a bottom-up 32-bpp DIB buffer (BITMAPINFOHEADER
/// followed by BGRA pixels) so it can be decoded by [`dib_to_png`]. The alpha
/// byte is forced opaque: an `HBITMAP` carries no defined alpha channel, and a
/// zero high byte would otherwise produce a fully transparent PNG.
#[cfg(target_os = "windows")]
unsafe fn hbitmap_to_dib_bytes(hbitmap: isize) -> Option<Vec<u8>> {
    extern "system" {
        fn GetObjectW(obj: isize, size: i32, buf: *mut u8) -> i32;
        fn GetDC(hwnd: isize) -> isize;
        fn ReleaseDC(hwnd: isize, dc: isize) -> i32;
        fn GetDIBits(
            dc: isize,
            bitmap: isize,
            start: u32,
            lines: u32,
            bits: *mut u8,
            info: *mut BITMAPINFOHEADER,
            usage: u32,
        ) -> i32;
    }

    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct BITMAPINFOHEADER {
        biSize: u32,
        biWidth: i32,
        biHeight: i32,
        biPlanes: u16,
        biBitCount: u16,
        biCompression: u32,
        biSizeImage: u32,
        biXPelsPerMeter: i32,
        biYPelsPerMeter: i32,
        biClrUsed: u32,
        biClrImportant: u32,
    }

    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct BITMAP {
        bmType: i32,
        bmWidth: i32,
        bmHeight: i32,
        bmWidthBytes: i32,
        bmPlanes: u16,
        bmBitsPixel: u16,
        bmBits: isize,
    }

    const DIB_RGB_COLORS: u32 = 0;
    const BI_RGB: u32 = 0;

    let mut bmp = BITMAP {
        bmType: 0,
        bmWidth: 0,
        bmHeight: 0,
        bmWidthBytes: 0,
        bmPlanes: 0,
        bmBitsPixel: 0,
        bmBits: 0,
    };
    if GetObjectW(
        hbitmap,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bmp as *mut _ as *mut u8,
    ) == 0
    {
        return None;
    }
    if bmp.bmWidth <= 0 || bmp.bmHeight == 0 {
        return None;
    }
    let width = bmp.bmWidth.unsigned_abs();
    let height = bmp.bmHeight.unsigned_abs();
    let image_size = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;

    let mut header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        // Positive height requests bottom-up rows, matching `dib_to_png`.
        biHeight: height as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: image_size as u32,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };

    let dc = GetDC(0);
    if dc == 0 {
        return None;
    }
    let mut pixels = vec![0u8; image_size];
    let copied = GetDIBits(
        dc,
        hbitmap,
        0,
        height,
        pixels.as_mut_ptr(),
        &mut header,
        DIB_RGB_COLORS,
    );
    ReleaseDC(0, dc);
    if copied == 0 {
        return None;
    }

    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }

    let mut dib = Vec::with_capacity(std::mem::size_of::<BITMAPINFOHEADER>() + pixels.len());
    dib.extend_from_slice(std::slice::from_raw_parts(
        (&header as *const BITMAPINFOHEADER) as *const u8,
        std::mem::size_of::<BITMAPINFOHEADER>(),
    ));
    dib.extend_from_slice(&pixels);
    Some(dib)
}

#[cfg(target_os = "windows")]
fn dib_to_png(dib: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    if dib.len() < 40 {
        return None;
    }

    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]);
    if header_size < 40 {
        return None;
    }

    let width = i32::from_le_bytes([dib[4], dib[5], dib[6], dib[7]]);
    // DIB width must be positive; a negative value would wrap when cast to
    // u32 and overflow the per-row math below.
    let width = u32::try_from(width).ok()?;
    // A negative biHeight declares top-down rows; positive declares the
    // classic bottom-up layout. Dropping the sign would flip every image
    // produced by top-down sources.
    let raw_height = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]);
    let top_down = raw_height < 0;
    let height_abs = raw_height.unsigned_abs();
    let bit_count = u16::from_le_bytes([dib[14], dib[15]]);

    let header_size = header_size as usize;
    if header_size > dib.len() {
        return None;
    }
    let pixel_data = &dib[header_size..];

    // A clipboard producer may declare a header whose claimed pixel payload is
    // larger than the actual allocation. Validate the exact byte count up front
    // so the per-row slices below can never read out of bounds.
    let required_bytes: u128 = match bit_count {
        32 => u128::from(width) * u128::from(u64::from(height_abs)) * 4,
        24 => {
            let row = (u128::from(width) * 3).div_ceil(4) * 4;
            row * u128::from(u64::from(height_abs))
        }
        _ => return None,
    };
    if (pixel_data.len() as u128) < required_bytes {
        return None;
    }

    let img = match bit_count {
        32 => {
            let rgba = bgra_to_rgba(pixel_data, width, height_abs, top_down);
            let rgba = normalize_zero_alpha(rgba);
            image::RgbaImage::from_raw(width, height_abs, rgba)?
        }
        24 => {
            let rgb = bgr_to_rgb(pixel_data, width, height_abs, top_down);
            let mut buf = Vec::with_capacity(rgb.len());
            for chunk in rgb.as_chunks::<3>().0 {
                buf.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            image::RgbaImage::from_raw(width, height_abs, buf)?
        }
        _ => return None,
    };

    let mut png_bytes = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png_bytes, image::ImageFormat::Png).ok()?;
    Some((png_bytes.into_inner(), width, height_abs))
}

/// Treats an all-zero alpha channel as "no alpha": many BI_RGB 32-bpp DIB
/// producers leave the high byte at 0, which would otherwise decode to a fully
/// transparent PNG. A DIB with any meaningful alpha is left untouched.
#[cfg(target_os = "windows")]
fn normalize_zero_alpha(mut rgba: Vec<u8>) -> Vec<u8> {
    let all_zero = rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0);
    if all_zero {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
    }
    rgba
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    None
}

#[cfg(target_os = "windows")]
fn bgra_to_rgba(data: &[u8], width: u32, height: u32, top_down: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let row_size = (width * 4) as usize;
    for index in 0..height {
        // Bottom-up DIBs store rows reversed in memory; top-down DIBs do not.
        let y = if top_down { index } else { height - 1 - index };
        let start = (y as usize) * row_size;
        let row = &data[start..start + row_size];
        for chunk in row.as_chunks::<4>().0 {
            out.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]);
        }
    }
    out
}

#[cfg(target_os = "windows")]
fn bgr_to_rgb(data: &[u8], width: u32, height: u32, top_down: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let row_padded = (width * 3).div_ceil(4) * 4;
    for index in 0..height {
        // Mirror `bgra_to_rgba`: only bottom-up DIBs read rows in reverse.
        let y = if top_down { index } else { height - 1 - index };
        let start = (y as usize) * row_padded as usize;
        let row = &data[start..start + (width * 3) as usize];
        for chunk in row.as_chunks::<3>().0 {
            out.push(chunk[2]);
            out.push(chunk[1]);
            out.push(chunk[0]);
        }
    }
    out
}

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

        if OpenClipboard(0) == 0 {
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
            let mut buffer = [0u16; 520];
            let len = DragQueryFileW(handle, i, buffer.as_mut_ptr(), 520);
            if len > 0 {
                let wide: Vec<u16> = buffer[..len as usize].to_vec();
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

#[cfg(target_os = "windows")]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    extern "system" {
        fn GetForegroundWindow() -> isize;
        fn GetWindowThreadProcessId(hwnd: isize, process_id: *mut u32) -> u32;
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn QueryFullProcessImageNameW(
            process: isize,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn GetWindowTextW(hwnd: isize, buffer: *mut u16, max_count: i32) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            return crate::platform::ForegroundApp {
                name: String::new(),
                exe_path: String::new(),
            };
        }

        let mut title_buf = [0u16; 256];
        let title_len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 256);
        let title = if title_len > 0 {
            let wide: Vec<u16> = title_buf[..title_len as usize].to_vec();
            OsString::from_wide(&wide).to_string_lossy().to_string()
        } else {
            String::new()
        };

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return crate::platform::ForegroundApp {
                name: title.clone(),
                exe_path: String::new(),
            };
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process == 0 {
            return crate::platform::ForegroundApp {
                name: title.clone(),
                exe_path: String::new(),
            };
        }

        let mut path_buf = [0u16; 520];
        let mut size = 520u32;
        let result = QueryFullProcessImageNameW(process, 0, path_buf.as_mut_ptr(), &mut size);
        CloseHandle(process);

        if result != 0 {
            let wide: Vec<u16> = path_buf[..size as usize].to_vec();
            let full_path = OsString::from_wide(&wide).to_string_lossy().to_string();
            let name = std::path::Path::new(&full_path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            return crate::platform::ForegroundApp {
                name,
                exe_path: full_path,
            };
        }

        crate::platform::ForegroundApp {
            name: title,
            exe_path: String::new(),
        }
    }
}

#[cfg(target_os = "windows")]
pub fn extract_app_icon(
    icon_dir: &std::path::Path,
    app_name: &str,
    exe_path: &str,
) -> Option<String> {
    extern "system" {
        fn SHGetFileInfoW(
            path: *const u16,
            attributes: u32,
            info: *mut SHFILEINFOW,
            info_size: u32,
            flags: u32,
        ) -> usize;
        fn DestroyIcon(icon: isize) -> i32;
    }

    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct SHFILEINFOW {
        hIcon: isize,
        iIcon: i32,
        dwAttributes: u32,
        szDisplayName: [u16; 260],
        szTypeName: [u16; 80],
    }

    const SHGFI_ICON: u32 = 0x100;
    const SHGFI_LARGEICON: u32 = 0x0;

    let app_key = icon_key(app_name);

    if app_key.is_empty() {
        return None;
    }

    let icon_path = icon_dir.join(format!("{}.png", app_key));

    std::fs::create_dir_all(icon_dir).ok();

    if icon_path.exists() && is_normalized_app_icon(&icon_path) {
        return Some(icon_path.file_name().unwrap().to_string_lossy().to_string());
    }
    if icon_path.exists() {
        let _ = std::fs::remove_file(&icon_path);
    }

    let path_for_icon = if exe_path.is_empty() {
        format!("{}.exe", app_name)
    } else {
        exe_path.to_string()
    };
    let wide_name: Vec<u16> = path_for_icon
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut info = SHFILEINFOW {
        hIcon: 0,
        iIcon: 0,
        dwAttributes: 0,
        szDisplayName: [0u16; 260],
        szTypeName: [0u16; 80],
    };

    unsafe {
        let result = SHGetFileInfoW(
            wide_name.as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if result != 0 && info.hIcon != 0 {
            let hicon = info.hIcon;
            let saved = save_hicon_to_png(hicon, &icon_path);
            DestroyIcon(hicon);
            if saved {
                return Some(icon_path.file_name().unwrap().to_string_lossy().to_string());
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn save_hicon_to_png(hicon: isize, path: &std::path::Path) -> bool {
    extern "system" {
        fn GetIconInfo(hicon: isize, info: *mut ICONINFO) -> i32;
        fn DeleteObject(obj: isize) -> i32;
        fn GetDC(hwnd: isize) -> isize;
        fn ReleaseDC(hwnd: isize, dc: isize) -> i32;
        fn CreateCompatibleDC(dc: isize) -> isize;
        fn DeleteDC(dc: isize) -> i32;
        fn SelectObject(dc: isize, obj: isize) -> isize;
        fn GetObjectW(obj: isize, size: i32, buf: *mut u8) -> i32;
        fn GetDIBits(
            dc: isize,
            bitmap: isize,
            start: u32,
            lines: u32,
            bits: *mut u8,
            info: *mut BITMAPINFOHEADER,
            usage: u32,
        ) -> i32;
    }

    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct ICONINFO {
        fIcon: i32,
        xHotspot: u32,
        yHotspot: u32,
        hbmMask: isize,
        hbmColor: isize,
    }

    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct BITMAPINFOHEADER {
        biSize: u32,
        biWidth: i32,
        biHeight: i32,
        biPlanes: u16,
        biBitCount: u16,
        biCompression: u32,
        biSizeImage: u32,
        biXPelsPerMeter: i32,
        biYPelsPerMeter: i32,
        biClrUsed: u32,
        biClrImportant: u32,
    }

    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct BITMAP {
        bmType: i32,
        bmWidth: i32,
        bmHeight: i32,
        bmWidthBytes: i32,
        bmPlanes: u16,
        bmBitsPixel: u16,
        bmBits: isize,
    }

    const DIB_RGB_COLORS: u32 = 0;
    const BI_RGB: u32 = 0;

    unsafe {
        let mut icon_info = ICONINFO {
            fIcon: 0,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: 0,
            hbmColor: 0,
        };
        if GetIconInfo(hicon, &mut icon_info) == 0 {
            return false;
        }

        let mut bmp = BITMAP {
            bmType: 0,
            bmWidth: 0,
            bmHeight: 0,
            bmWidthBytes: 0,
            bmPlanes: 0,
            bmBitsPixel: 0,
            bmBits: 0,
        };
        let hbm = if icon_info.hbmColor != 0 {
            icon_info.hbmColor
        } else {
            icon_info.hbmMask
        };
        if GetObjectW(
            hbm,
            std::mem::size_of::<BITMAP>() as i32,
            &mut bmp as *mut _ as *mut u8,
        ) == 0
        {
            DeleteObject(icon_info.hbmMask);
            if icon_info.hbmColor != 0 {
                DeleteObject(icon_info.hbmColor);
            }
            return false;
        }

        let width = bmp.bmWidth.unsigned_abs();
        let height = bmp.bmHeight.unsigned_abs();
        let row_size = (width * 32).div_ceil(32) * 4;
        let image_size = row_size * height;

        let mut bi = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bmp.bmWidth,
            biHeight: -bmp.bmHeight,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: image_size,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        let dc = GetDC(0);
        let mem_dc = CreateCompatibleDC(dc);
        let old_bmp = SelectObject(mem_dc, hbm);
        let mut pixels = vec![0u8; image_size as usize];
        GetDIBits(
            mem_dc,
            hbm,
            0,
            height,
            pixels.as_mut_ptr(),
            &mut bi,
            DIB_RGB_COLORS,
        );
        SelectObject(mem_dc, old_bmp);
        DeleteDC(mem_dc);
        ReleaseDC(0, dc);

        let mut rgba = vec![0u8; pixels.len()];
        for (i, chunk) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let base = i * 4;
            rgba[base] = chunk[2];
            rgba[base + 1] = chunk[1];
            rgba[base + 2] = chunk[0];
            rgba[base + 3] = chunk[3];
        }

        let result = image::RgbaImage::from_raw(width, height, rgba)
            .and_then(|img| {
                let img = normalize_app_icon(img);
                let mut buf = std::io::Cursor::new(Vec::new());
                img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
                std::fs::write(path, buf.into_inner()).ok()
            })
            .is_some();

        DeleteObject(icon_info.hbmMask);
        if icon_info.hbmColor != 0 {
            DeleteObject(icon_info.hbmColor);
        }
        result
    }
}

#[cfg(not(target_os = "windows"))]
pub fn extract_app_icon(
    _icon_dir: &std::path::Path,
    _app_name: &str,
    _exe_path: &str,
) -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
fn bgra_to_rgba(_data: &[u8], _width: u32, _height: u32) -> Vec<u8> {
    vec![]
}

#[cfg(not(target_os = "windows"))]
fn bgr_to_rgb(_data: &[u8], _width: u32, _height: u32) -> Vec<u8> {
    vec![]
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

// ---------------------------------------------------------------------------
//  Non-Windows stubs for WindowsClipboardMonitor and ClipboardChange
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
pub struct WindowsClipboardMonitor {
    running: bool,
    ignored_apps: Vec<String>,
    sender: Option<mpsc::Sender<ClipboardChange>>,
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub sequence: u32,
}

#[cfg(not(target_os = "windows"))]
impl Default for WindowsClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "windows"))]
impl WindowsClipboardMonitor {
    pub fn new() -> Self {
        Self {
            running: false,
            ignored_apps: vec![],
            sender: None,
            stop_sender: None,
            handle: None,
        }
    }

    pub fn start(&mut self) -> Result<mpsc::Receiver<ClipboardChange>, String> {
        if self.running {
            // The monitor thread exits silently when the capture worker's
            // receiver is dropped (worker panic or spawn failure) — nothing
            // resets this flag. Detect the dead thread so a restart is not
            // permanently blocked by "already running".
            let thread_dead = self
                .handle
                .as_ref()
                .is_some_and(|handle| handle.is_finished());
            if !thread_dead {
                return Err("clipboard monitor is already running".to_string());
            }
            self.running = false;
            self.handle = None;
        }

        let (sender, receiver) = mpsc::channel();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let sender_for_thread = sender.clone();

        let handle = thread::Builder::new()
            .name("clipboard-monitor".to_owned())
            .spawn(move || {
                let mut poll_state = crate::platform::ClipboardPollState::new();
                let mut sequence = 0u32;

                loop {
                    match stop_receiver.recv_timeout(Duration::from_millis(500)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }

                    let platform = crate::platform::platform();
                    let current_text = platform.read_clipboard_text();
                    // Text is the cheap signal. File and image reads only run
                    // when there is no text, so image decoding does not happen
                    // on every tick while a text selection sits on the
                    // clipboard, while image/file copies are still detected.
                    let (current_files, current_image) = if current_text.is_none() {
                        let files = platform.read_clipboard_file_paths();
                        let image = if files.is_empty() {
                            platform.read_clipboard_image()
                        } else {
                            None
                        };
                        (files, image)
                    } else {
                        (Vec::new(), None)
                    };

                    if poll_state.observe(current_text, current_files, current_image) {
                        sequence = sequence.wrapping_add(1);
                        let _ = sender_for_thread.send(ClipboardChange { sequence });
                    }
                }
            })
            .map_err(|error| format!("failed to spawn clipboard monitor: {error}"))?;

        self.sender = Some(sender);
        self.stop_sender = Some(stop_sender);
        self.handle = Some(handle);
        self.running = true;

        Ok(receiver)
    }

    pub fn stop(&mut self) {
        self.running = false;
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        self.sender = None;
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                // A panicked monitor explains why captures silently stopped;
                // swallowing the payload hid that from every log.
                if let Err(panic) = handle.join() {
                    crate::log_event!(
                        "[clipboard-monitor] monitor thread terminated with a panic: {panic:?}"
                    );
                }
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        self.ignored_apps = apps;
    }
}

#[cfg(not(target_os = "windows"))]
impl Drop for WindowsClipboardMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
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

    #[cfg(target_os = "windows")]
    #[test]
    fn double_start_returns_error() {
        let mut monitor = WindowsClipboardMonitor::new();
        let _ = monitor.start().ok();
        let result = monitor.start();
        assert!(result.is_err());
        monitor.stop();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn format_name_lookup() {
        assert_eq!(format_id_to_name(1), "CF_TEXT");
        assert_eq!(format_id_to_name(13), "CF_UNICODETEXT");
        assert_eq!(format_id_to_name(15), "CF_HDROP");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn unknown_format_gets_fallback_name() {
        assert!(format_id_to_name(99999).starts_with("format_"));
    }

    #[test]
    fn self_trigger_marker_covers_text_link_file_and_newline_variants() {
        let text = "https://example.com\nC:\\tmp\\note.txt";
        let marker = self_trigger_marker_for_text(text);
        let marker_text = String::from_utf8(marker).unwrap();

        for kind in ["text", "link", "file"] {
            assert!(
                marker_text.contains(&crate::content::hash::compute_content_hash(
                    kind, text, None
                ))
            );
        }
        assert!(
            marker_text.contains(&crate::content::hash::compute_content_hash(
                "text",
                &text.replace('\n', "\r\n"),
                None,
            ))
        );
    }

    #[test]
    fn self_trigger_marker_matches_only_the_marked_clipboard_text() {
        let text = "https://example.com";
        let marker = self_trigger_marker_for_text(text);

        assert!(clipboard_change_is_self_write(&marker, text));
        assert!(!clipboard_change_is_self_write(
            &marker,
            "https://other.example.com"
        ));
    }

    #[test]
    fn malformed_or_unrelated_markers_are_not_suppressed() {
        assert!(!clipboard_change_is_self_write(
            &[0xff, 0xfe],
            "ordinary text"
        ));
        assert!(!clipboard_change_is_self_write(
            b"not-a-content-hash",
            "ordinary text"
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn drop_files_payload_is_wide_terminated_and_double_null_ended() {
        let payload =
            build_drop_files_bytes(&["C:\\a.png".to_owned(), "D:\\notes\\b.txt".to_owned()])
                .unwrap();
        // DROPFILES header: pFiles=20, pt=(0,0), fNC=false, fWide=true.
        assert_eq!(&payload[0..4], &20u32.to_le_bytes());
        assert_eq!(&payload[12..16], &0u32.to_le_bytes());
        assert_eq!(&payload[16..20], &1u32.to_le_bytes());

        let paths_region = &payload[20..];
        let wide: Vec<u16> = paths_region
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        // Each path is NUL-terminated and the whole list is double-NUL ended.
        assert!(wide.ends_with(&[0, 0]));
        let terminated_at = wide
            .split(|unit| *unit == 0)
            .filter(|part| !part.is_empty());
        let decoded = terminated_at
            .map(String::from_utf16_lossy)
            .collect::<Vec<_>>();
        assert_eq!(decoded, ["C:\\a.png", "D:\\notes\\b.txt"]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn drop_files_payload_rejects_empty_paths() {
        assert!(build_drop_files_bytes(&[String::new()]).is_err());
    }

    #[test]
    fn app_icons_are_normalized_to_32_pixels() {
        let source = image::RgbaImage::new(96, 48);
        let normalized = normalize_app_icon(source);
        assert_eq!(normalized.dimensions(), (APP_ICON_SIZE, APP_ICON_SIZE));
    }

    #[test]
    fn cf_html_extracts_the_fragment_using_byte_offsets() {
        let payload = cf_html_payload("<b>bold</b>");
        assert_eq!(parse_cf_html(&payload).as_deref(), Some("<b>bold</b>"));
    }

    #[test]
    fn cf_html_falls_back_to_full_html_offsets_without_fragment() {
        let payload = cf_html_payload_without_fragment("<i>italic</i>");
        assert_eq!(parse_cf_html(&payload).as_deref(), Some("<i>italic</i>"));
    }

    #[test]
    fn cf_html_rejects_missing_or_empty_content() {
        assert_eq!(parse_cf_html(b""), None);
        assert_eq!(parse_cf_html(b"not a cf-html payload"), None);
        let empty = b"Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000";
        assert_eq!(parse_cf_html(empty), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dib_to_png_rejects_pixel_payload_shorter_than_header_claims() {
        let mut header = [0u8; 40];
        header[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
        header[4..8].copy_from_slice(&4096i32.to_le_bytes()); // biWidth
        header[8..12].copy_from_slice(&4096i32.to_le_bytes()); // biHeight
        header[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount

        // Only 64 bytes of pixel data follow, far less than the 4096*4096*4
        // bytes the header claims.
        let mut dib = header.to_vec();
        dib.extend_from_slice(&[0u8; 64]);
        assert_eq!(dib_to_png(&dib), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dib_to_png_rejects_header_larger_than_the_buffer() {
        let mut header = [0u8; 56];
        header[0..4].copy_from_slice(&100u32.to_le_bytes()); // biSize > buffer length
        header[4..8].copy_from_slice(&8i32.to_le_bytes());
        header[8..12].copy_from_slice(&8i32.to_le_bytes());
        header[14..16].copy_from_slice(&32u16.to_le_bytes());
        assert_eq!(dib_to_png(&header), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dib_to_png_rejects_negative_width() {
        let mut header = [0u8; 40];
        header[0..4].copy_from_slice(&40u32.to_le_bytes());
        header[4..8].copy_from_slice(&(-8i32).to_le_bytes()); // negative width
        header[8..12].copy_from_slice(&8i32.to_le_bytes());
        header[14..16].copy_from_slice(&32u16.to_le_bytes());
        let mut dib = header.to_vec();
        dib.extend_from_slice(&[0u8; 256]);
        assert_eq!(dib_to_png(&dib), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dib_to_png_normalizes_zero_alpha_from_bi_rgb_producers() {
        // A 1x1 BI_RGB 32-bpp DIB whose high byte is 0 (the common case for
        // producers that do not set alpha). The decoded PNG must be opaque,
        // not fully transparent.
        let mut header = [0u8; 40];
        header[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
        header[4..8].copy_from_slice(&1i32.to_le_bytes()); // biWidth
        header[8..12].copy_from_slice(&1i32.to_le_bytes()); // biHeight
        header[12..14].copy_from_slice(&1u16.to_le_bytes()); // biPlanes
        header[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount
        header[16..20].copy_from_slice(&0u32.to_le_bytes()); // BI_RGB
        let mut dib = header.to_vec();
        dib.extend_from_slice(&[0x30, 0x20, 0x10, 0x00]); // B, G, R, A=0

        let (png, width, height) = dib_to_png(&dib).expect("decodable DIB");
        assert_eq!((width, height), (1, 1));
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0[3], 255, "alpha must be opaque");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dib_to_png_respects_top_down_negative_bi_height() {
        // A 1x2 BI_RGB 32-bpp DIB with negative biHeight: rows are stored
        // top-down, so the first buffer row must become the first PNG row.
        let mut header = [0u8; 40];
        header[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
        header[4..8].copy_from_slice(&1i32.to_le_bytes()); // biWidth
        header[8..12].copy_from_slice(&(-2i32).to_le_bytes()); // top-down
        header[12..14].copy_from_slice(&1u16.to_le_bytes()); // biPlanes
        header[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount
        header[16..20].copy_from_slice(&0u32.to_le_bytes()); // BI_RGB
        let mut dib = header.to_vec();
        dib.extend_from_slice(&[0, 0, 255, 255]); // row 0: red
        dib.extend_from_slice(&[255, 0, 0, 255]); // row 1: blue

        let (png, width, height) = dib_to_png(&dib).expect("decodable DIB");
        assert_eq!((width, height), (1, 2));
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255], "first row red");
        assert_eq!(
            decoded.get_pixel(0, 1).0,
            [0, 0, 255, 255],
            "second row blue"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn read_clipboard_file_paths_reads_a_cf_hdrop() {
        use std::os::windows::ffi::OsStrExt;

        extern "system" {
            fn OpenClipboard(hwnd: isize) -> i32;
            fn EmptyClipboard() -> i32;
            fn CloseClipboard() -> i32;
            fn SetClipboardData(format: u32, handle: isize) -> isize;
            fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
            fn GlobalLock(handle: isize) -> *const u8;
            fn GlobalUnlock(handle: isize) -> i32;
        }
        const GMEM_MOVEABLE: u32 = 0x0002;
        const CF_HDROP: u32 = 15;
        const DROPFILES_SIZE: usize = 20;

        let path = r"C:\Windows\notepad.exe";
        let mut wide: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().collect();
        wide.push(0);
        wide.push(0);
        let path_bytes = wide.len() * 2;
        let total = DROPFILES_SIZE + path_bytes;

        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, total);
            assert_ne!(handle, 0, "GlobalAlloc failed");
            let ptr = GlobalLock(handle) as *mut u8;
            assert!(!ptr.is_null(), "GlobalLock failed");
            std::ptr::write_bytes(ptr, 0, DROPFILES_SIZE);
            std::ptr::write_unaligned(ptr as *mut u32, DROPFILES_SIZE as u32); // pFiles
            std::ptr::write_unaligned(ptr.add(16) as *mut i32, 1); // fWide
            std::ptr::copy_nonoverlapping(
                wide.as_ptr() as *const u8,
                ptr.add(DROPFILES_SIZE),
                path_bytes,
            );
            GlobalUnlock(handle);

            assert_ne!(OpenClipboard(0), 0, "OpenClipboard failed");
            EmptyClipboard();
            assert_ne!(
                SetClipboardData(CF_HDROP, handle),
                0,
                "SetClipboardData failed"
            );
            CloseClipboard();
        }

        let paths = read_clipboard_file_paths();
        assert_eq!(paths, vec![path.to_owned()]);
    }

    /// Builds a CF_HTML payload with accurate byte offsets. Header widths are
    /// fixed-width (10 digits), so header length is independent of the values.
    fn cf_html_payload(fragment: &str) -> Vec<u8> {
        let body =
            format!("<html><body><!--StartFragment-->{fragment}<!--EndFragment--></body></html>");
        let fragment_start =
            body.find("<!--StartFragment-->").unwrap() + "<!--StartFragment-->".len();
        let fragment_end = body.find("<!--EndFragment-->").unwrap();
        write_cf_html_offsets(body, fragment_start, fragment_end)
    }

    fn cf_html_payload_without_fragment(body: &str) -> Vec<u8> {
        write_cf_html_offsets(body.to_owned(), 0, 0)
    }

    fn write_cf_html_offsets(body: String, fragment_start: usize, fragment_end: usize) -> Vec<u8> {
        let header = "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
        let mut out = header.as_bytes().to_vec();
        let header_len = out.len();
        out.extend_from_slice(body.as_bytes());
        let end_html = header_len + body.len();
        let apply = |out: &mut Vec<u8>, label: &str, value: usize| {
            let needle = label.as_bytes();
            let pos = out
                .windows(needle.len())
                .position(|window| window == needle)
                .unwrap();
            let digits = format!("{value:010}");
            out[pos + needle.len()..pos + needle.len() + 10].copy_from_slice(digits.as_bytes());
        };
        apply(&mut out, "StartHTML:", header_len);
        apply(&mut out, "EndHTML:", end_html);
        if fragment_end > fragment_start {
            apply(&mut out, "StartFragment:", header_len + fragment_start);
            apply(&mut out, "EndFragment:", header_len + fragment_end);
        }
        out
    }
}
