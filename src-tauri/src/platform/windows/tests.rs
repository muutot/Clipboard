//! Decoding/writing regression tests.

use super::*;

use html::{parse_cf_html, parse_cf_html_source_url};
#[cfg(target_os = "windows")]
use read_text::format_id_to_name;
#[cfg(target_os = "windows")]
use write::build_drop_files_bytes;

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

/// The self-trigger marker decides whether a clipboard change is the
/// application's own write, so the clipboard must be opened with the same
/// retry every other reader uses: a clipboard that another application holds
/// for a moment must not turn the check into a false negative.
#[cfg(target_os = "windows")]
#[test]
fn the_clipboard_open_retries_while_another_application_holds_it() {
    let mut attempts = 0u32;
    assert!(access::open_clipboard_with_retry_using(|index| {
        attempts = index + 1;
        index >= 2
    }));
    assert_eq!(attempts, 3, "the open is retried until it succeeds");

    let mut attempts = 0u32;
    assert!(!access::open_clipboard_with_retry_using(|index| {
        attempts = index + 1;
        false
    }));
    assert_eq!(attempts, 20, "the retry is bounded");
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
        build_drop_files_bytes(&["C:\\a.png".to_owned(), "D:\\notes\\b.txt".to_owned()]).unwrap();
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

#[test]
fn cf_html_extracts_the_source_url_header_line() {
    let payload =
        cf_html_payload_with_source_url("<b>bold</b>", Some("https://example.com/page?q=1"));
    assert_eq!(
        parse_cf_html_source_url(&payload).as_deref(),
        Some("https://example.com/page?q=1")
    );
    // The fragment itself still parses from the same payload.
    assert_eq!(parse_cf_html(&payload).as_deref(), Some("<b>bold</b>"));
}

#[test]
fn cf_html_source_url_scan_is_bounded_to_the_header_region() {
    // A copied page that itself mentions "SourceURL:" inside its markup must
    // not shadow the real header: only the header region is scanned.
    let payload = cf_html_payload("SourceURL:https://evil.example/page");
    assert_eq!(parse_cf_html_source_url(&payload), None);
}

#[test]
fn cf_html_source_url_is_absent_without_the_header_line() {
    assert_eq!(
        parse_cf_html_source_url(&cf_html_payload("<b>bold</b>")),
        None
    );
    assert_eq!(parse_cf_html_source_url(b""), None);
    assert_eq!(parse_cf_html_source_url(b"not a cf-html payload"), None);
}

#[cfg(target_os = "windows")]
#[test]
fn dib_to_png_rejects_dimensions_above_the_shared_decode_ceiling() {
    let width = crate::content::hash::MAX_DECODE_DIMENSION + 1;
    let mut dib = vec![0u8; 40 + width as usize * 4];
    dib[0..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&width.to_le_bytes());
    dib[8..12].copy_from_slice(&1i32.to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    assert!(dib_to_png(&dib).is_none());
}

#[cfg(target_os = "windows")]
#[test]
fn bgra_conversion_does_not_reserve_trailing_allocation_bytes() {
    let bytes = vec![255; 1024 * 1024];
    let rgba = bgra_to_rgba(&bytes, 1, 1, false);
    assert_eq!(rgba, vec![255; 4]);
    assert_eq!(rgba.capacity(), 4);
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
fn dib_to_png_skips_standard_bitfields_masks_after_info_header() {
    // A 1x1 32-bpp BI_BITFIELDS DIB with a 40-byte info header: the 12
    // mask bytes follow the header and must not be decoded as pixels.
    let mut header = [0u8; 40];
    header[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
    header[4..8].copy_from_slice(&1i32.to_le_bytes()); // biWidth
    header[8..12].copy_from_slice(&1i32.to_le_bytes()); // biHeight
    header[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount
    header[16..20].copy_from_slice(&3u32.to_le_bytes()); // BI_BITFIELDS
    let mut dib = header.to_vec();
    dib.extend_from_slice(&0x00FF_0000u32.to_le_bytes()); // red mask
    dib.extend_from_slice(&0x0000_FF00u32.to_le_bytes()); // green mask
    dib.extend_from_slice(&0x0000_00FFu32.to_le_bytes()); // blue mask
    dib.extend_from_slice(&[0x30, 0x20, 0x10, 0xFF]); // B, G, R, A

    let (png, width, height) = dib_to_png(&dib).expect("decodable DIB");
    assert_eq!((width, height), (1, 1));
    let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(decoded.get_pixel(0, 0).0, [0x10, 0x20, 0x30, 0xFF]);
}

#[cfg(target_os = "windows")]
#[test]
fn dib_to_png_rejects_nonstandard_or_compressed_dibs() {
    let mut header = [0u8; 40];
    header[0..4].copy_from_slice(&40u32.to_le_bytes());
    header[4..8].copy_from_slice(&1i32.to_le_bytes());
    header[8..12].copy_from_slice(&1i32.to_le_bytes());
    header[14..16].copy_from_slice(&32u16.to_le_bytes());
    // BI_JPEG (4): compressed payloads have no decodable pixel rows.
    header[16..20].copy_from_slice(&4u32.to_le_bytes());
    let mut dib = header.to_vec();
    dib.extend_from_slice(&[0u8; 4]);
    assert_eq!(dib_to_png(&dib), None);

    // BI_BITFIELDS with a non-standard red mask cannot be decoded by the
    // fixed BGRA reader.
    header[16..20].copy_from_slice(&3u32.to_le_bytes());
    let mut dib = header.to_vec();
    dib.extend_from_slice(&0x0000_000Fu32.to_le_bytes()); // non-standard red
    dib.extend_from_slice(&0x0000_FF00u32.to_le_bytes());
    dib.extend_from_slice(&0x0000_00FFu32.to_le_bytes());
    dib.extend_from_slice(&[0u8; 4]);
    assert_eq!(dib_to_png(&dib), None);
}

#[cfg(target_os = "windows")]
#[test]
fn dib_size_rejects_overflowing_dimensions() {
    assert_eq!(dib_32bpp_image_size(2, 3), Some(24));
    assert_eq!(dib_32bpp_image_size(u32::MAX, 1), None);
    assert_eq!(dib_32bpp_image_size(1, u32::MAX), None);
    assert_eq!(dib_32bpp_image_size(0, 1), None);
    assert_eq!(dib_32bpp_image_size(16_384, 16_384), None);
    assert_eq!(dib_32bpp_image_size(16_384, 8192), Some(512 * 1024 * 1024));
}

#[test]
fn native_payload_caps_reject_oversized_lengths_without_allocating() {
    for limit in [
        MAX_NATIVE_TEXT_BYTES,
        MAX_NATIVE_IMAGE_BYTES,
        MAX_SELF_TRIGGER_BYTES,
    ] {
        assert!(!native_payload_size_allowed(0, limit));
        assert!(native_payload_size_allowed(limit, limit));
        assert!(!native_payload_size_allowed(limit + 1, limit));
        assert!(!native_payload_size_allowed(usize::MAX, limit));
    }
    assert!(self_trigger_marker_for_text("a\nb\nc").len() < MAX_SELF_TRIGGER_BYTES);
}

#[cfg(target_os = "windows")]
fn set_cf_hdrop(path: &str) {
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

        // The clipboard can be held open briefly by another process
        // (including a running dev instance of this app), so retry the
        // open like the production path does.
        let mut opened = false;
        for _ in 0..100 {
            if OpenClipboard(0) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(opened, "OpenClipboard failed");
        EmptyClipboard();
        assert_ne!(
            SetClipboardData(CF_HDROP, handle),
            0,
            "SetClipboardData failed"
        );
        CloseClipboard();
    }
}

#[cfg(target_os = "windows")]
#[test]
fn read_clipboard_file_paths_reads_a_cf_hdrop() {
    // `set_cf_hdrop` calls `EmptyClipboard` and replaces the system
    // clipboard, so this test destroys whatever a developer had copied.
    // Only run it when explicitly permitted: CI's Windows runner sets the
    // variable, while local `npm run verify` / `ci:local` runs skip it so
    // no real user clipboard is overwritten. Synthetic DIB/CF_HTML tests
    // cover the decoding rules without touching the clipboard.
    if std::env::var_os("CLIPBOARD_ALLOW_REAL_CLIPBOARD_TEST").is_none() {
        eprintln!(
                "skipping read_clipboard_file_paths_reads_a_cf_hdrop: set \
                 CLIPBOARD_ALLOW_REAL_CLIPBOARD_TEST=1 to allow overwriting the real system clipboard"
            );
        return;
    }
    let path = r"C:\Windows\notepad.exe";
    set_cf_hdrop(path);
    assert_eq!(read_clipboard_file_paths(), vec![path.to_owned()]);

    // Regression: `DragQueryFileW` returns the required length when the
    // buffer is too small, so a path above the old fixed 520-unit buffer
    // overran the slice and aborted the process under `panic = "abort"`.
    let long_path = format!(r"C:\{}", "a".repeat(600));
    set_cf_hdrop(&long_path);
    assert_eq!(read_clipboard_file_paths(), vec![long_path]);
}

#[cfg(target_os = "windows")]
fn set_clipboard_format_bytes(format: u32, bytes: &[u8]) {
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

    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) };
    assert_ne!(handle, 0, "GlobalAlloc failed");
    let ptr = unsafe { GlobalLock(handle) } as *mut u8;
    assert!(!ptr.is_null(), "GlobalLock failed");
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        GlobalUnlock(handle);
    }

    let mut opened = false;
    for _ in 0..100 {
        if unsafe { OpenClipboard(0) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(opened, "OpenClipboard failed");
    unsafe {
        EmptyClipboard();
        assert_ne!(
            SetClipboardData(format, handle),
            0,
            "SetClipboardData failed"
        );
        CloseClipboard();
    }
}

/// Reads raw bytes for one clipboard format. Test-only mirror of the native
/// read paths, used to verify the self-trigger marker the write path stores.
#[cfg(target_os = "windows")]
fn get_clipboard_format_bytes(format: u32) -> Option<Vec<u8>> {
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
        let ptr = GlobalLock(handle);
        let data = if ptr.is_null() {
            None
        } else {
            Some(std::slice::from_raw_parts(ptr, size).to_vec())
        };
        GlobalUnlock(handle);
        CloseClipboard();
        data
    }
}

#[cfg(target_os = "windows")]
fn register_clipboard_format(name: &str) -> u32 {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }
    let wide: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let format = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    assert_ne!(format, 0, "RegisterClipboardFormatW failed");
    format
}

/// Exercises the native write/read paths end to end against the real system
/// clipboard. Like the CF_HDROP test above, this destroys the user's current
/// clipboard contents, so it only runs when explicitly permitted; CI's
/// Windows runner sets the variable.
#[cfg(target_os = "windows")]
#[test]
fn real_clipboard_round_trips_cover_native_write_and_read_paths() {
    if std::env::var_os("CLIPBOARD_ALLOW_REAL_CLIPBOARD_TEST").is_none() {
        eprintln!(
            "skipping real_clipboard_round_trips: set \
             CLIPBOARD_ALLOW_REAL_CLIPBOARD_TEST=1 to allow overwriting the real system clipboard"
        );
        return;
    }

    // The production text write stores the payload plus the self-trigger
    // marker; both must be readable back and the marker must suppress a
    // capture of our own text.
    let text = "real clipboard round trip caf\u{e9}";
    write_clipboard_text_with_self_trigger(text).unwrap();
    assert_eq!(read_clipboard_text().as_deref(), Some(text));
    if let Some(marker_format) = self_trigger_format_id() {
        let marker = get_clipboard_format_bytes(marker_format)
            .expect("self-trigger marker must be set after a text write");
        assert!(clipboard_change_is_self_write(&marker, text));
        assert!(!clipboard_change_is_self_write(&marker, "unrelated text"));
    }

    // RTF payloads decode lossily as trimmed text through the registered
    // format name.
    let rtf = "{\\rtf1\\ansi Hello RTF}";
    set_clipboard_format_bytes(
        register_clipboard_format("Rich Text Format"),
        rtf.as_bytes(),
    );
    assert_eq!(read_clipboard_rtf().as_deref(), Some(rtf));

    // CF_HTML payloads decode through the fragment offsets.
    let html_payload = cf_html_payload("<b>frag</b>");
    set_clipboard_format_bytes(register_clipboard_format("HTML Format"), &html_payload);
    assert_eq!(read_clipboard_html().as_deref(), Some("<b>frag</b>"));

    // A synthetic 1x1 opaque 32-bpp DIB placed as CF_DIB decodes to a PNG.
    let mut header = [0u8; 40];
    header[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
    header[4..8].copy_from_slice(&1i32.to_le_bytes()); // biWidth
    header[8..12].copy_from_slice(&1i32.to_le_bytes()); // biHeight
    header[12..14].copy_from_slice(&1u16.to_le_bytes()); // biPlanes
    header[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount
    header[16..20].copy_from_slice(&0u32.to_le_bytes()); // BI_RGB
    let mut dib = header.to_vec();
    dib.extend_from_slice(&[0x30, 0x20, 0x10, 0xFF]); // B, G, R, A
    set_clipboard_format_bytes(CF_DIB, &dib);
    let (png, width, height) = read_clipboard_image().expect("CF_DIB must decode");
    assert_eq!((width, height), (1, 1));
    image::load_from_memory(&png).expect("decoded image bytes are a valid PNG");

    // The file write path round-trips a real temporary file reference.
    let file_path = std::env::temp_dir().join(format!(
        "clipboard-real-files-round-trip-{}.txt",
        std::process::id()
    ));
    std::fs::write(&file_path, b"round trip").unwrap();
    let path_string = file_path.to_string_lossy().into_owned();
    write_clipboard_files_with_self_trigger(std::slice::from_ref(&path_string)).unwrap();
    assert_eq!(read_clipboard_file_paths(), vec![path_string]);

    // The change sequence is observable after every write above.
    assert!(read_clipboard_sequence().is_some());

    let _ = std::fs::remove_file(file_path);
}

/// Builds a CF_HTML payload with accurate byte offsets. Header widths are
/// fixed-width (10 digits), so header length is independent of the values.
fn cf_html_payload(fragment: &str) -> Vec<u8> {
    cf_html_payload_with_source_url(fragment, None)
}

fn cf_html_payload_with_source_url(fragment: &str, source_url: Option<&str>) -> Vec<u8> {
    let body =
        format!("<html><body><!--StartFragment-->{fragment}<!--EndFragment--></body></html>");
    let fragment_start = body.find("<!--StartFragment-->").unwrap() + "<!--StartFragment-->".len();
    let fragment_end = body.find("<!--EndFragment-->").unwrap();
    write_cf_html_offsets(body, fragment_start, fragment_end, source_url)
}

fn cf_html_payload_without_fragment(body: &str) -> Vec<u8> {
    write_cf_html_offsets(body.to_owned(), 0, 0, None)
}

fn write_cf_html_offsets(
    body: String,
    fragment_start: usize,
    fragment_end: usize,
    source_url: Option<&str>,
) -> Vec<u8> {
    let source_url_line = source_url
        .map(|url| format!("SourceURL:{url}\r\n"))
        .unwrap_or_default();
    let header = format!(
        "Version:0.9\r\n{source_url_line}StartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n"
    );
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
