//! Bitmap reads: HBITMAP-to-DIB conversion, DIB-to-PNG decoding
//! and the row-order/channel helpers shared by the Windows paths.

use super::*;
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

        if !open_clipboard_with_retry() {
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
            if !native_payload_size_allowed(size, MAX_NATIVE_IMAGE_BYTES) {
                CloseClipboard();
                return None;
            }
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                CloseClipboard();
                return None;
            }
            let result = dib_to_png(std::slice::from_raw_parts(ptr, size));
            GlobalUnlock(handle);
            result
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
pub(super) unsafe fn hbitmap_to_dib_bytes(hbitmap: isize) -> Option<Vec<u8>> {
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
    let image_size = dib_32bpp_image_size(width, height)? as usize;

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
    let header_size = std::mem::size_of::<BITMAPINFOHEADER>();
    let mut dib = vec![0u8; header_size + image_size];
    let pixels = &mut dib[header_size..];
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
    if copied != height as i32 {
        return None;
    }

    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }

    dib[..header_size].copy_from_slice(std::slice::from_raw_parts(
        (&header as *const BITMAPINFOHEADER) as *const u8,
        header_size,
    ));
    Some(dib)
}

#[cfg(target_os = "windows")]
pub(super) fn dib_to_png(dib: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
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
    dib_32bpp_image_size(width, height_abs)?;
    let bit_count = u16::from_le_bytes([dib[14], dib[15]]);

    let header_size = header_size as usize;
    if header_size > dib.len() {
        return None;
    }
    // Only uncompressed layouts are decodable by the fixed BGRA/BGR readers
    // below. BITFIELDS headers carry their channel masks around byte 40
    // (inside V4+ headers, or appended right after a 40-byte info header);
    // accept only the standard 32-bpp BGRA masks and reject everything else
    // instead of decoding mask bytes as pixels.
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    const BI_ALPHABITFIELDS: u32 = 6;
    const STANDARD_BGRA_MASKS: (u32, u32, u32) = (0x00FF_0000, 0x0000_FF00, 0x0000_00FF);
    let compression = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
    let mask_bytes: usize = match compression {
        BI_RGB => 0,
        BI_BITFIELDS => 12,
        BI_ALPHABITFIELDS => 16,
        _ => return None,
    };
    if mask_bytes > 0 {
        if bit_count != 32 || dib.len() < 40 + mask_bytes {
            return None;
        }
        let red = u32::from_le_bytes(dib[40..44].try_into().unwrap());
        let green = u32::from_le_bytes(dib[44..48].try_into().unwrap());
        let blue = u32::from_le_bytes(dib[48..52].try_into().unwrap());
        if (red, green, blue) != STANDARD_BGRA_MASKS {
            return None;
        }
        if compression == BI_ALPHABITFIELDS {
            let alpha = u32::from_le_bytes(dib[52..56].try_into().unwrap());
            if alpha != 0xFF00_0000 {
                return None;
            }
        }
    }
    let pixel_start = header_size.max(40 + mask_bytes);
    if pixel_start > dib.len() {
        return None;
    }
    let pixel_data = &dib[pixel_start..];

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

    let pixel_data = &pixel_data[..usize::try_from(required_bytes).ok()?];
    let img = match bit_count {
        32 => {
            let rgba = bgra_to_rgba(pixel_data, width, height_abs, top_down);
            let rgba = normalize_zero_alpha(rgba);
            image::RgbaImage::from_raw(width, height_abs, rgba)?
        }
        24 => {
            let rgb = bgr_to_rgb(pixel_data, width, height_abs, top_down);
            let mut buf = Vec::with_capacity(width as usize * height_abs as usize * 4);
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
pub(super) fn normalize_zero_alpha(mut rgba: Vec<u8>) -> Vec<u8> {
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
pub(super) fn bgra_to_rgba(data: &[u8], width: u32, height: u32, top_down: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
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
pub(super) fn bgr_to_rgb(data: &[u8], width: u32, height: u32, top_down: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(width as usize * height as usize * 3);
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

/// Byte size of a 32-bpp top-down DIB for `width`×`height` pixels. Returns
/// `None` when dimensions overflow or exceed the shared decode budget, so a
/// corrupt bitmap is rejected before allocating or calling GetDIBits.
#[cfg(target_os = "windows")]
pub(super) fn dib_32bpp_image_size(width: u32, height: u32) -> Option<u32> {
    if width == 0
        || height == 0
        || width > crate::content::hash::MAX_DECODE_DIMENSION
        || height > crate::content::hash::MAX_DECODE_DIMENSION
    {
        return None;
    }

    let row_size = width
        .checked_mul(32)
        .and_then(|bits| bits.div_ceil(32).checked_mul(4))?;
    let size = row_size.checked_mul(height)?;
    native_payload_size_allowed(size as usize, MAX_NATIVE_IMAGE_BYTES).then_some(size)
}

#[cfg(not(target_os = "windows"))]
pub(super) fn bgra_to_rgba(_data: &[u8], _width: u32, _height: u32) -> Vec<u8> {
    vec![]
}

#[cfg(not(target_os = "windows"))]
pub(super) fn bgr_to_rgb(_data: &[u8], _width: u32, _height: u32) -> Vec<u8> {
    vec![]
}
