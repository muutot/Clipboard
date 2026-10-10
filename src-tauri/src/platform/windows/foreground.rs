//! Foreground-window inspection and per-app icon extraction/caching.

use super::*;

#[cfg(target_os = "windows")]
use crate::content::icon_key;
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
        return icon_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string());
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
                return icon_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string());
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
pub(super) fn save_hicon_to_png(hicon: isize, path: &std::path::Path) -> bool {
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
        // `bmWidth`/`bmHeight` come from an OS bitmap. Keep the size math
        // checked so a corrupt or extreme value cannot wrap and under-allocate
        // the DIB buffer that `GetDIBits` then fills (the sibling
        // `hbitmap_to_dib_bytes` does the same).
        let Some(image_size) = dib_32bpp_image_size(width, height) else {
            DeleteObject(icon_info.hbmMask);
            if icon_info.hbmColor != 0 {
                DeleteObject(icon_info.hbmColor);
            }
            return false;
        };

        let mut bi = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bmp.bmWidth,
            biHeight: bmp.bmHeight.saturating_neg(),
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
        if dc == 0 {
            // A null display DC (session without a desktop, exhausted GDI
            // handles) must not flow into CreateCompatibleDC/SelectObject:
            // the subsequent GetDIBits would silently produce a zero-filled
            // buffer and poison the icon cache with a black square.
            DeleteObject(icon_info.hbmMask);
            if icon_info.hbmColor != 0 {
                DeleteObject(icon_info.hbmColor);
            }
            return false;
        }
        let mem_dc = CreateCompatibleDC(dc);
        if mem_dc == 0 {
            ReleaseDC(0, dc);
            DeleteObject(icon_info.hbmMask);
            if icon_info.hbmColor != 0 {
                DeleteObject(icon_info.hbmColor);
            }
            return false;
        }
        let old_bmp = SelectObject(mem_dc, hbm);
        let mut pixels = vec![0u8; image_size as usize];
        let scanned_lines = GetDIBits(
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
        if scanned_lines == 0 {
            // No scanlines were produced: `pixels` is still all zeros and
            // must never be encoded into the cache.
            DeleteObject(icon_info.hbmMask);
            if icon_info.hbmColor != 0 {
                DeleteObject(icon_info.hbmColor);
            }
            return false;
        }

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
                // Atomic write so a crash cannot leave a truncated icon that
                // the cache would then serve forever.
                crate::content::FileStore::save_bytes_atomically(path, buf.into_inner().as_slice())
                    .ok()
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
