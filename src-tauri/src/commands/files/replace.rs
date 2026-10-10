//! Constrained replacement of an icon file from a webview-picked source.

use crate::storage::StoragePaths;

#[tauri::command]
pub fn replace_icon_file(
    paths: tauri::State<'_, StoragePaths>,
    name: String,
    source_path: String,
) -> Result<(), String> {
    let file_name = std::path::Path::new(&name)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "invalid icon filename".to_string())?;
    let target = paths.storage.join("icons").join(file_name);
    match target.extension().map(|e| e.to_string_lossy().to_string()) {
        Some(ext) if ext == "png" => {}
        _ => return Err("icon filename must end in .png".to_string()),
    }
    let source = std::path::Path::new(&source_path);
    // The source arrives from the webview (either a user-picked dialog path or
    // an icons-dir path). Without validation this command is an arbitrary-file
    // copy into the asset-served icons directory: a compromised renderer could
    // stage any disk file there and fetch it back. Gate on real image content.
    validate_replace_source(source)?;
    // A planted symlink at the target must not be followed by fs::copy —
    // otherwise the link's own target would be overwritten with the new icon.
    if let Ok(metadata) = std::fs::symlink_metadata(&target) {
        if metadata.file_type().is_symlink() {
            return Err("icon target is a symlink; refusing to overwrite".to_string());
        }
    }
    std::fs::copy(source, &target).map_err(|e| format!("failed to replace icon: {e}"))?;
    Ok(())
}

/// Mirrors the file-dialog filters in `IconCacheSettingsPanel.svelte`.
const REPLACE_ALLOWED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "ico", "webp", "svg"];
/// Icons are small; refuse anything larger before touching the cache.
pub(super) const REPLACE_MAX_SOURCE_BYTES: u64 = 10 * 1024 * 1024;

/// Rejects non-image sources before they can be staged into the icons
/// directory. Raster formats must header-decode via the `image` crate; `ico`
/// must carry its 4-byte magic and `svg` must start with `<` after an
/// optional BOM/whitespace (any XML document does, so valid SVGs — including
/// ones with prologs — pass while renamed binaries are rejected). Both still
/// render inert inside `<img>`. Residual risk (a compromised renderer staging
/// *other valid images*) is accepted: the dialog flow is explicit user consent.
pub(super) fn validate_replace_source(source: &std::path::Path) -> Result<(), String> {
    if !source.is_file() {
        return Err("source file not found".to_string());
    }
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !REPLACE_ALLOWED_EXTENSIONS.contains(&extension.as_str()) {
        return Err("only image files (png/jpg/jpeg/ico/webp/svg) can be used".to_string());
    }
    let size = std::fs::metadata(source)
        .map(|m| m.len())
        .map_err(|e| format!("cannot read source file: {e}"))?;
    if size > REPLACE_MAX_SOURCE_BYTES {
        return Err("source image exceeds the 10 MiB limit".to_string());
    }
    if extension == "ico" {
        validate_ico_magic(source)?;
    } else if extension == "svg" {
        validate_svg_prolog(source)?;
    } else {
        image::image_dimensions(source)
            .map_err(|_| "source is not a decodable image".to_string())?;
    }
    Ok(())
}

/// ICO/CUR files start with a reserved zero word plus the 1 (icon) or 2
/// (cursor) type word; anything else is a renamed foreign file.
fn validate_ico_magic(source: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    let mut header = [0u8; 4];
    std::fs::File::open(source)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| "source is not a decodable image".to_string())?;
    if header[0] == 0 && header[1] == 0 && (header[2] == 1 || header[2] == 2) && header[3] == 0 {
        Ok(())
    } else {
        Err("source is not a decodable image".to_string())
    }
}

/// SVG is XML text: after an optional UTF-8 BOM and whitespace the first
/// byte must be `<` (this covers `<?xml?>` prologs and comments, which also
/// start with `<`). Binary decoys fail here.
fn validate_svg_prolog(source: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(source).map_err(|_| "source is not a decodable image".to_string())?;
    let mut start = bytes.as_slice();
    if start.starts_with(&[0xEF, 0xBB, 0xBF]) {
        start = &start[3..];
    }
    let text =
        std::str::from_utf8(start).map_err(|_| "source is not a decodable image".to_string())?;
    if text.trim_start().starts_with('<') {
        Ok(())
    } else {
        Err("source is not a decodable image".to_string())
    }
}
