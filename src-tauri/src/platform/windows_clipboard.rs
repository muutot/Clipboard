#![allow(non_snake_case, dead_code)]

use std::sync::mpsc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::content::hash::icon_key;

pub const CF_UNICODETEXT: u32 = 13;
pub const CF_DIB: u32 = 8;
pub const CF_DIBV5: u32 = 17;
pub const CF_HDROP: u32 = 15;
pub const CF_BITMAP: u32 = 2;

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

pub const VK_V: u32 = 0x56;

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

pub fn clipboard_change_is_self_write(
    marker: &[u8],
    observed_text: &str,
) -> bool {
    let Ok(marker_text) = std::str::from_utf8(marker) else {
        return false;
    };
    let marker_hashes = marker_text.trim_matches('\0').split('\n');
    let expected_hashes = crate::content::hash::compute_clipboard_write_hashes(observed_text);
    expected_hashes
        .iter()
        .any(|expected| marker_hashes.clone().any(|marked| marked == expected))
}

fn normalize_app_icon(image: image::RgbaImage) -> image::RgbaImage {
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

pub struct WindowsClipboardMonitor {
    running: bool,
    ignored_apps: Vec<String>,
    last_sequence: u32,
    sender: Option<mpsc::Sender<ClipboardChange>>,
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub sequence: u32,
}

impl Default for WindowsClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

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
            return Err("clipboard monitor is already running".to_string());
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
                let _ = handle.join();
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
    use std::sync::OnceLock;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

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

/// Writes CF_UNICODETEXT plus a private hash marker. Marker failures are
/// intentionally best-effort: the text write must retain its original
/// behavior even when a clipboard implementation rejects custom formats.
#[cfg(target_os = "windows")]
pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;

    const GMEM_MOVEABLE: u32 = 0x0002;

    #[link(name = "User32")]
    extern "system" {
        fn OpenClipboard(window: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, memory: isize) -> isize;
    }

    #[link(name = "Kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
        fn GlobalFree(memory: isize) -> isize;
        fn GlobalLock(memory: isize) -> *const u8;
        fn GlobalUnlock(memory: isize) -> i32;
    }

    struct ClipboardGuard;

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }

    unsafe fn allocate_global_bytes(
        bytes: &[u8],
        global_alloc: unsafe extern "system" fn(u32, usize) -> isize,
        global_free: unsafe extern "system" fn(isize) -> isize,
        global_lock: unsafe extern "system" fn(isize) -> *const u8,
        global_unlock: unsafe extern "system" fn(isize) -> i32,
    ) -> Result<isize, String> {
        let memory = global_alloc(GMEM_MOVEABLE, bytes.len());
        if memory == 0 {
            return Err("failed to allocate clipboard memory".to_owned());
        }
        let target = global_lock(memory).cast_mut();
        if target.is_null() {
            global_free(memory);
            return Err("failed to lock clipboard memory".to_owned());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        global_unlock(memory);
        Ok(memory)
    }

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
        let text_memory = allocate_global_bytes(
            wide_bytes,
            GlobalAlloc,
            GlobalFree,
            GlobalLock,
            GlobalUnlock,
        )?;
        if SetClipboardData(CF_UNICODETEXT, text_memory) == 0 {
            GlobalFree(text_memory);
            return Err("failed to write text to the system clipboard".to_owned());
        }

        if let Some(format) = self_trigger_format_id() {
            if let Ok(marker_memory) =
                allocate_global_bytes(&marker, GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock)
            {
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

#[cfg(target_os = "windows")]
fn format_id_to_name(format_id: u32) -> String {
    use std::collections::BTreeMap;
    use std::sync::LazyLock;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    static PREDEFINED: LazyLock<BTreeMap<u32, &'static str>> = LazyLock::new(|| {
        BTreeMap::from([
            (1, "CF_TEXT"), (2, "CF_BITMAP"), (3, "CF_METAFILEPICT"),
            (4, "CF_SYLK"), (5, "CF_DIF"), (6, "CF_TIFF"),
            (7, "CF_OEMTEXT"), (8, "CF_DIB"), (9, "CF_PALETTE"),
            (10, "CF_PENDATA"), (11, "CF_RIFF"), (12, "CF_WAVE"),
            (13, "CF_UNICODETEXT"), (14, "CF_ENHMETAFILE"), (15, "CF_HDROP"),
            (16, "CF_LOCALE"), (17, "CF_DIBV5"),
            (128, "CF_OWNERDISPLAY"), (129, "CF_DSPTEXT"),
            (130, "CF_DSPBITMAP"), (131, "CF_DSPMETAFILEPICT"), (132, "CF_DSPENHMETAFILE"),
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

        let len = (0..).take_while(|&i| *ptr.add(i) != 0).count();
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

        dib_to_png(&data)
    }
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
    let height_abs = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]).unsigned_abs();
    let bit_count = u16::from_le_bytes([dib[14], dib[15]]);

    let pixel_data = &dib[header_size as usize..];

    let img = match bit_count {
        32 => {
            let rgba = bgra_to_rgba(pixel_data, width as u32, height_abs);
            image::RgbaImage::from_raw(width as u32, height_abs, rgba)?
        }
        24 => {
            let rgb = bgr_to_rgb(pixel_data, width as u32, height_abs);
            let mut buf = Vec::with_capacity(rgb.len());
            for chunk in rgb.chunks_exact(3) {
                buf.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            image::RgbaImage::from_raw(width as u32, height_abs, buf)?
        }
        _ => return None,
    };

    let mut png_bytes = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png_bytes, image::ImageFormat::Png).ok()?;
    Some((png_bytes.into_inner(), width as u32, height_abs))
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    None
}

#[cfg(target_os = "windows")]
fn bgra_to_rgba(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let row_size = (width * 4) as usize;
    for y in (0..height).rev() {
        let start = (y as usize) * row_size;
        let row = &data[start..start + row_size];
        for chunk in row.chunks_exact(4) {
            out.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]);
        }
    }
    out
}

#[cfg(target_os = "windows")]
fn bgr_to_rgb(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let row_padded = (width * 3).div_ceil(4) * 4;
    for y in (0..height).rev() {
        let start = (y as usize) * row_padded as usize;
        let row = &data[start..start + (width * 3) as usize];
        for chunk in row.chunks_exact(3) {
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
        fn GlobalLock(handle: isize) -> *const u8;
        fn GlobalUnlock(handle: isize) -> i32;
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

        let handle = GetClipboardData(CF_HDROP);
        if handle == 0 {
            CloseClipboard();
            return vec![];
        }

        let ptr = GlobalLock(handle) as isize;
        if ptr == 0 {
            CloseClipboard();
            return vec![];
        }

        let file_count = DragQueryFileW(ptr, 0xFFFFFFFF, std::ptr::null_mut(), 0);
        let mut paths = Vec::new();

        for i in 0..file_count {
            let mut buffer = [0u16; 520];
            let len = DragQueryFileW(ptr, i, buffer.as_mut_ptr(), 520);
            if len > 0 {
                let wide: Vec<u16> = buffer[..len as usize].to_vec();
                paths.push(OsString::from_wide(&wide).to_string_lossy().to_string());
            }
        }

        GlobalUnlock(ptr);
        CloseClipboard();
        paths
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_file_paths() -> Vec<String> {
    vec![]
}

#[cfg(target_os = "windows")]
pub struct ForegroundApp {
    pub name: String,
    pub exe_path: String,
}

#[cfg(target_os = "windows")]
pub fn get_foreground_app() -> ForegroundApp {
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
            return ForegroundApp {
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
            return ForegroundApp {
                name: title.clone(),
                exe_path: String::new(),
            };
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process == 0 {
            return ForegroundApp {
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
            return ForegroundApp {
                name,
                exe_path: full_path,
            };
        }

        ForegroundApp {
            name: title,
            exe_path: String::new(),
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_foreground_app() -> ForegroundApp {
    ForegroundApp {
        name: String::new(),
        exe_path: String::new(),
    }
}

#[cfg(not(target_os = "windows"))]
pub struct ForegroundApp {
    pub name: String,
    pub exe_path: String,
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
        for (i, chunk) in pixels.chunks_exact(4).enumerate() {
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

    #[test]
    fn app_icons_are_normalized_to_32_pixels() {
        let source = image::RgbaImage::new(96, 48);
        let normalized = normalize_app_icon(source);
        assert_eq!(normalized.dimensions(), (APP_ICON_SIZE, APP_ICON_SIZE));
    }
}
