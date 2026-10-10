//! Small pure-ish helpers shared by the capture commands and the worker.

use std::path::Path;

use crate::content;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::platform;
use crate::storage::{ClipboardRepository, Database};

pub fn foreground_app_name(app: &platform::ForegroundApp) -> Option<String> {
    if !app.exe_path.trim().is_empty() {
        let leaf = app
            .exe_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&app.exe_path);
        Path::new(leaf)
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
    } else if !app.name.trim().is_empty() {
        Some(app.name.trim().to_owned())
    } else {
        None
    }
}

/// Normalizes a platform clipboard image payload into encoded PNG bytes.
///
/// Windows supplies encoded PNG; Linux/macOS supply raw RGBA. The adapter
/// labels the representation so equal byte lengths cannot corrupt encoded images.
/// Invalid raw dimensions/buffers are rejected rather than stored as a PNG.
pub fn normalize_platform_image_to_png(
    data: platform::ClipboardImageData,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let data = match data {
        platform::ClipboardImageData::Png(data) => return data,
        platform::ClipboardImageData::Rgba(data) => data,
    };
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4));
    if expected == Some(0) || expected != Some(data.len()) {
        return Vec::new();
    }
    let Some(frame) = image::RgbaImage::from_raw(width, height, data) else {
        // Unreachable: the byte count was validated above.
        return Vec::new();
    };
    let mut png = Vec::new();
    if image::DynamicImage::ImageRgba8(frame)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .is_err()
    {
        // Unreachable for an in-memory RGBA buffer.
        return Vec::new();
    }
    png
}

/// True when `content_hash` matches one of this app's recent clipboard writes.
pub fn should_skip_self_triggered_hash(
    guard: &mut content::self_trigger::SelfTriggerGuard,
    content_hash: &str,
) -> bool {
    guard.is_self_triggered(content_hash)
}

/// True when capturing `text` of `kind` would re-record our own write-back.
pub fn should_skip_self_triggered_text(
    guard: &mut content::self_trigger::SelfTriggerGuard,
    kind: ClipboardKind,
    text: &str,
) -> bool {
    let kind_name = match kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image | ClipboardKind::File => return false,
    };
    guard.is_text_write_self_triggered(kind_name, text)
}

/// Registers hashes for an image this app just wrote so the capture loop can skip it.
pub fn register_image_self_trigger(
    guard: &mut content::self_trigger::SelfTriggerGuard,
    resource_path: Option<&str>,
    fallback_hash: Option<&str>,
) -> Result<(), String> {
    let mut registered = false;

    if let Some(path) = resource_path.filter(|path| !path.trim().is_empty()) {
        match std::fs::read(path) {
            Ok(data) => {
                guard.mark_media_write("image", &data);
                registered = true;
            }
            Err(error) if fallback_hash.is_none() => {
                return Err(format!("failed to read image for self-trigger: {error}"));
            }
            Err(_) => {}
        }
    }

    if let Some(content_hash) = fallback_hash.filter(|hash| !hash.trim().is_empty()) {
        guard.mark_as_self_triggered(content_hash);
        registered = true;
    }

    if registered {
        Ok(())
    } else {
        Err("image self-trigger has no readable resource or content hash".to_owned())
    }
}

/// Loads the stored row for the `clipboard-item-added` event. A re-copied
/// entry is de-duplicated onto its existing row with a frozen `created_at_ms`
/// and a refreshed `last_used_at_ms`, so emitting the transient capture
/// snapshot would lie about the capture time until the next list reload.
pub(super) fn apply_capture_auto_tags(
    database: &Database,
    capture: &crate::CaptureState,
    id: &str,
    item: &ClipboardItem,
) {
    let tags = capture.match_auto_tags(item);
    if tags.is_empty() {
        return;
    }
    if let Err(error) = database.add_tags(id, &tags) {
        crate::log_error!("[clipboard-worker] failed to apply auto tags for {id}: {error}");
    }
}

pub(super) fn load_emit_item(
    database: &Database,
    saved_id: &str,
    fallback: &ClipboardItem,
) -> ClipboardItem {
    database
        .get_item(saved_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            let mut item = fallback.clone();
            item.id = saved_id.to_owned();
            item
        })
}
