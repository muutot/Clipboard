use super::{
    captured_file_metadata, foreground_app_name, normalize_platform_image_to_png,
    register_image_self_trigger, should_skip_self_triggered_hash, should_skip_self_triggered_text,
    store_captured_file_references, CapturedFileReference,
};
use crate::content::self_trigger::SelfTriggerGuard;
use crate::content::RESOURCE_METADATA_SCHEMA_VERSION;
use crate::domain::ClipboardKind;
use crate::platform::ForegroundApp;
use std::fs;
use std::path::PathBuf;

fn temp_dir(label: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-capture-test-{label}-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    fs::create_dir_all(&directory).expect("create temp dir");
    directory
}

#[test]
fn foreground_app_name_prefers_exe_leaf_stem() {
    let app = ForegroundApp {
        name: "Window Title".to_owned(),
        exe_path: r"C:\Tools\MyApp\editor.exe".to_owned(),
    };
    assert_eq!(foreground_app_name(&app).as_deref(), Some("editor"));

    // Forward slashes and a missing extension still resolve to the stem.
    let unix_like = ForegroundApp {
        name: "ignored".to_owned(),
        exe_path: "/opt/my-app/tool".to_owned(),
    };
    assert_eq!(foreground_app_name(&unix_like).as_deref(), Some("tool"));
}

#[test]
fn foreground_app_name_falls_back_to_window_title_then_none() {
    let titled = ForegroundApp {
        name: "  Notepad  ".to_owned(),
        exe_path: String::new(),
    };
    assert_eq!(foreground_app_name(&titled).as_deref(), Some("Notepad"));

    let blank = ForegroundApp {
        name: "   ".to_owned(),
        exe_path: " ".to_owned(),
    };
    assert_eq!(foreground_app_name(&blank), None);
}

#[test]
fn normalize_platform_image_preserves_png_with_raw_pixel_byte_length() {
    let image = image::RgbaImage::from_pixel(10, 10, image::Rgba([10, 20, 30, 255]));
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    // A valid ancillary tEXt chunk makes the encoded file exactly 400 bytes.
    let mut text = b"Comment\0".to_vec();
    text.resize(400 - png.len() - 12, b'x');
    let mut chunk = (text.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(b"tEXt");
    chunk.extend(text);
    let mut crc = !0u32;
    for byte in &chunk[4..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    chunk.extend_from_slice(&(!crc).to_be_bytes());
    png.splice(png.len() - 12..png.len() - 12, chunk);
    assert_eq!(png.len(), 400);
    assert_eq!(image::load_from_memory(&png).unwrap().to_rgba8(), image);
    let normalized =
        normalize_platform_image_to_png(crate::platform::ClipboardImageData::Png(png), 10, 10);
    assert_eq!(
        image::load_from_memory(&normalized).unwrap().to_rgba8(),
        image
    );
}

#[test]
fn normalize_platform_image_reencodes_raw_rgba_buffers() {
    // 2x2 opaque red pixels — the exact shape the Linux/macOS adapters
    // return (`rgba.into_raw()`).
    let raw = vec![
        255u8, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
    ];
    let png = normalize_platform_image_to_png(
        crate::platform::ClipboardImageData::Rgba(raw.clone()),
        2,
        2,
    );
    assert_ne!(png, raw, "raw pixels must be re-encoded");
    let decoded = image::load_from_memory(&png).expect("normalized payload must decode");
    assert_eq!((decoded.width(), decoded.height()), (2, 2));
    assert_eq!(decoded.to_rgba8().into_raw(), raw);
}

#[test]
fn normalize_platform_image_keeps_encoded_and_rejects_malformed_raw_payloads() {
    // An already-encoded PNG (the Windows adapter shape) passes through
    // untouched even when dimensions are provided.
    let image = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
    let mut encoded = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut encoded),
            image::ImageFormat::Png,
        )
        .expect("encode fixture");
    let passthrough = normalize_platform_image_to_png(
        crate::platform::ClipboardImageData::Png(encoded.clone()),
        image.width(),
        image.height(),
    );
    assert_eq!(passthrough, encoded);

    // Empty raw payloads and length/dimension mismatches are rejected.
    assert!(normalize_platform_image_to_png(
        crate::platform::ClipboardImageData::Rgba(Vec::new()),
        2,
        2
    )
    .is_empty());
    let short = vec![0u8; 4];
    assert!(normalize_platform_image_to_png(
        crate::platform::ClipboardImageData::Rgba(short),
        2,
        2
    )
    .is_empty());
}

#[test]
fn text_self_trigger_matches_marked_kind_and_content() {
    let mut guard = SelfTriggerGuard::new();
    guard.mark_clipboard_write("secret-token");

    assert!(should_skip_self_triggered_text(
        &mut guard,
        ClipboardKind::Text,
        "secret-token"
    ));
    assert!(should_skip_self_triggered_text(
        &mut guard,
        ClipboardKind::Link,
        "secret-token"
    ));
    // Different text is a genuine user copy and must be captured.
    assert!(!should_skip_self_triggered_text(
        &mut guard,
        ClipboardKind::Text,
        "other"
    ));

    // Media kinds never match through the text path.
    assert!(!should_skip_self_triggered_text(
        &mut guard,
        ClipboardKind::Image,
        "secret-token"
    ));
    assert!(!should_skip_self_triggered_text(
        &mut guard,
        ClipboardKind::File,
        "secret-token"
    ));
}

#[test]
fn hash_self_trigger_skips_registered_hashes_only() {
    let mut guard = SelfTriggerGuard::new();
    guard.mark_as_self_triggered("hash-1");

    assert!(should_skip_self_triggered_hash(&mut guard, "hash-1"));
    assert!(!should_skip_self_triggered_hash(&mut guard, "hash-2"));
}

#[test]
fn register_image_self_trigger_reads_resource_or_falls_back() {
    let directory = temp_dir("register");
    let image_path = directory.join("shot.png");
    fs::write(&image_path, b"png-bytes").unwrap();

    // A readable resource registers its bytes.
    let mut guard = SelfTriggerGuard::new();
    register_image_self_trigger(
        &mut guard,
        Some(image_path.to_str().expect("temp path is utf-8")),
        None,
    )
    .expect("readable file must register");
    let hashes = guard.media_write_hashes("image", b"png-bytes");
    assert!(
        hashes
            .iter()
            .any(|hash| should_skip_self_triggered_hash(&mut guard, hash)),
        "the written bytes must be suppressed afterwards"
    );

    // An unreadable resource falls back to the explicit content hash.
    let mut fallback_guard = SelfTriggerGuard::new();
    register_image_self_trigger(
        &mut fallback_guard,
        Some(
            directory
                .join("missing.png")
                .to_str()
                .expect("temp path is utf-8"),
        ),
        Some("fallback-hash"),
    )
    .expect("fallback hash must register");
    assert!(should_skip_self_triggered_hash(
        &mut fallback_guard,
        "fallback-hash"
    ));

    // Nothing readable and no hash is an error.
    let mut empty_guard = SelfTriggerGuard::new();
    assert!(register_image_self_trigger(&mut empty_guard, None, None).is_err());
    assert!(register_image_self_trigger(&mut empty_guard, Some("  "), Some("  ")).is_err());

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn store_captured_file_references_copies_small_files_into_storage() {
    let source_dir = temp_dir("files-src");
    let storage_dir = temp_dir("files-store");
    let source = source_dir.join("report.txt");
    fs::write(&source, b"hello capture").unwrap();

    let references = store_captured_file_references(
        &[source.to_string_lossy().into_owned()],
        &storage_dir,
        1024,
    );
    assert_eq!(references.len(), 1);
    let reference = &references[0];
    assert!(reference.copied, "a small file must be copied into storage");
    assert_ne!(PathBuf::from(&reference.storage_path), source);
    assert!(PathBuf::from(&reference.storage_path).exists());
    assert_eq!(reference.original_name, "report.txt");
    assert_eq!(reference.size_bytes, 13);
    assert!(reference.content_hash.is_some());

    // A missing source degrades to pass-through metadata instead of
    // aborting the whole capture batch.
    let missing = source_dir.join("gone.txt");
    let degraded = store_captured_file_references(
        &[missing.to_string_lossy().into_owned()],
        &storage_dir,
        1024,
    );
    assert!(!degraded[0].copied);
    assert_eq!(degraded[0].original_name, "gone.txt");
    assert_eq!(degraded[0].size_bytes, 0);
    assert!(degraded[0].content_hash.is_none());

    fs::remove_dir_all(&source_dir).unwrap();
    fs::remove_dir_all(&storage_dir).unwrap();
}

fn sample_reference(name: &str, size: u64) -> CapturedFileReference {
    CapturedFileReference {
        original_path: format!(r"C:\Users\u\Desktop\{name}"),
        storage_path: format!(r"D:\storage\files\hash-{name}"),
        original_name: name.to_owned(),
        size_bytes: size,
        content_hash: Some(format!("hash-{name}")),
        extension: Some("txt".to_owned()),
        mime_type: "text/plain".to_owned(),
        created_at_ms: Some(111),
        modified_at_ms: Some(222),
        copied: true,
    }
}

#[test]
fn captured_file_metadata_serializes_single_and_multi_selections() {
    let single = captured_file_metadata(&[sample_reference("one.txt", 10)]);
    let value: serde_json::Value = serde_json::from_str(&single).unwrap();
    assert_eq!(value["schemaVersion"], RESOURCE_METADATA_SCHEMA_VERSION);
    assert_eq!(value["sizeBytes"], 10);
    // A single-file record keeps the original desktop path for "reveal".
    assert_eq!(
        value["originalPath"],
        serde_json::json!(r"C:\Users\u\Desktop\one.txt")
    );
    assert_eq!(value["files"].as_array().unwrap().len(), 1);

    let multi = captured_file_metadata(&[
        sample_reference("one.txt", 10),
        sample_reference("two.bin", 32),
    ]);
    let multi_value: serde_json::Value = serde_json::from_str(&multi).unwrap();
    assert_eq!(multi_value["sizeBytes"], 42);
    // Multi-file captures drop the singular original path; the per-entry
    // list keeps each origin.
    assert!(multi_value["originalPath"].is_null());
    assert_eq!(multi_value["files"].as_array().unwrap().len(), 2);
}
