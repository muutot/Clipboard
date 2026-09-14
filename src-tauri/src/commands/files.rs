use serde::Serialize;

use crate::content::{compute_content_hash, compute_normalized_media_hash, icon_key};
use crate::storage::{ClipboardRepository, Database, StoragePaths};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconCacheEntry {
    /// Some(app) when a recorded application backs this row; None for orphan icon files.
    pub app_name: Option<String>,
    /// Column-1 text: the application name, or the icon file name without extension.
    pub display_name: String,
    /// Existing icon file name in the icons directory; None when an app has no cached icon.
    pub icon_name: Option<String>,
    /// Content hash of the icon file so identical images with different names deduplicate.
    pub content_hash: Option<String>,
    /// File name that a replace action writes to (may not exist yet for icon-less apps).
    pub target_icon_name: String,
    /// Size of the icon file in bytes; 0 when no cached icon exists.
    pub size_bytes: u64,
    /// First display character used for the letter-text icon.
    pub first_char: String,
}

fn icon_first_char(value: &str) -> String {
    value
        .chars()
        .next()
        .map(|c| c.to_string().to_uppercase())
        .unwrap_or_default()
}

fn icon_content_hash(path: &std::path::Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|bytes| compute_normalized_media_hash("image", &bytes))
}

/// Resolves the canonical icon file name for a recorded application. Prefers
/// the Windows `icon_key(app).png` layout and falls back to the content-hash
/// layout used by `AppIconStore` on other platforms.
fn app_icon_file_name(app: &str) -> String {
    let key = icon_key(app);
    if key.is_empty() {
        return format!("{}.png", compute_content_hash("icon", app, None));
    }
    format!("{key}.png")
}

fn build_icon_cache(
    icons_dir: &std::path::Path,
    apps: Vec<(String, Option<String>)>,
) -> Vec<IconCacheEntry> {
    let mut owned_files = std::collections::HashSet::new();
    let mut entries = Vec::new();

    for (app, _db_icon) in apps {
        let key_name = app_icon_file_name(&app);
        owned_files.insert(key_name.clone());

        let mut icon_name: Option<String> = None;
        let mut size_bytes: u64 = 0;
        let key_path = icons_dir.join(&key_name);
        if key_path.is_file() {
            icon_name = Some(key_name.clone());
            size_bytes = std::fs::metadata(&key_path).map(|m| m.len()).unwrap_or(0);
        } else {
            let hash = compute_content_hash("icon", &icon_key(&app), None);
            for ext in ["png", "ico", "svg", "jpg", "jpeg"] {
                let candidate = icons_dir.join(format!("{hash}.{ext}"));
                if candidate.is_file() {
                    let name = candidate
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    owned_files.insert(name.clone());
                    icon_name = Some(name);
                    size_bytes = std::fs::metadata(&candidate).map(|m| m.len()).unwrap_or(0);
                    break;
                }
            }
        }

        let first_char = icon_first_char(&app);
        let content_hash = icon_name
            .as_ref()
            .and_then(|name| icon_content_hash(&icons_dir.join(name)));
        entries.push(IconCacheEntry {
            app_name: Some(app.clone()),
            display_name: app,
            icon_name,
            content_hash,
            target_icon_name: key_name,
            size_bytes,
            first_char,
        });
    }

    if let Ok(dir) = std::fs::read_dir(icons_dir) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "png") {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if owned_files.contains(&name) {
                continue;
            }
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let first_char = icon_first_char(&stem);
            entries.push(IconCacheEntry {
                app_name: None,
                display_name: stem,
                icon_name: Some(name.clone()),
                content_hash: icon_content_hash(&path),
                target_icon_name: name,
                size_bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
                first_char,
            });
        }
    }

    entries.sort_by(|a, b| {
        b.app_name
            .is_some()
            .cmp(&a.app_name.is_some())
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    entries
}

#[tauri::command]
pub fn list_icon_cache(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<Vec<IconCacheEntry>, String> {
    let apps = database
        .list_source_applications_with_icons()
        .map_err(|e| e.to_string())?;
    Ok(build_icon_cache(&paths.storage.join("icons"), apps))
}

#[tauri::command]
pub fn delete_icon_files(
    paths: tauri::State<'_, StoragePaths>,
    names: Vec<String>,
) -> Result<u64, String> {
    let icons_dir = paths.storage.join("icons");
    let mut deleted = 0u64;
    for name in &names {
        // Names arrive from the webview: strip any directory components and
        // verify the resolved file stays inside the managed icons directory
        // before deleting, so `..\..\x.png` or absolute paths cannot remove
        // arbitrary `.png` files on disk.
        let Some(file_name) = std::path::Path::new(name)
            .file_name()
            .and_then(|n| n.to_str())
        else {
            continue;
        };
        let path = icons_dir.join(file_name);
        let in_icons_dir = path
            .canonicalize()
            .map(|resolved| resolved.starts_with(icons_dir.canonicalize().unwrap_or_default()))
            .unwrap_or(false);
        if !in_icons_dir {
            continue;
        }
        if path.extension().is_some_and(|e| e == "png") && path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

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
    std::fs::copy(source, &target).map_err(|e| format!("failed to replace icon: {e}"))?;
    Ok(())
}

/// Mirrors the file-dialog filters in `IconCacheSettingsPanel.svelte`.
const REPLACE_ALLOWED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "ico", "webp", "svg"];
/// Icons are small; refuse anything larger before touching the cache.
const REPLACE_MAX_SOURCE_BYTES: u64 = 10 * 1024 * 1024;

/// Rejects non-image sources before they can be staged into the icons
/// directory. Raster formats must header-decode via the `image` crate; `ico`
/// and `svg` are extension-gated only (no decoder is bundled) and render inert
/// inside `<img>`. Residual risk (a compromised renderer staging *other valid
/// images*) is accepted: the dialog flow is explicit user consent.
fn validate_replace_source(source: &std::path::Path) -> Result<(), String> {
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
    if extension != "svg" && extension != "ico" {
        image::image_dimensions(source)
            .map_err(|_| "source is not a decodable image".to_string())?;
    }
    Ok(())
}

// NOTE: the former `copy_file_to` command (an unrestricted src→dst file copy
// reachable from the webview) was removed: it had no frontend caller and was
// an arbitrary-file-copy primitive. `replace_icon_file` is the constrained
// replacement for the one legitimate use case.
//
// `save_clipboard_item_file` is the constrained replacement for the "Save As"
// flow: the webview passes only the item id and a user-chosen destination, and
// the source is resolved from the stored record and required to live under an
// owned resource root.

/// Resolves a stored resource reference to an existing file that is inside an
/// owned image/file root. Absolute stored paths are canonicalized and checked
/// against both roots; relative references are resolved against them first.
fn managed_source_path(paths: &StoragePaths, stored: &str) -> Result<std::path::PathBuf, String> {
    let raw = std::path::Path::new(stored);
    let resolved = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        [&paths.images, &paths.files, &paths.storage]
            .iter()
            .map(|root| root.join(raw))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| "clipboard item resource was not found".to_string())?
    };
    let canonical = std::fs::canonicalize(&resolved)
        .map_err(|_| "clipboard item resource was not found".to_string())?;
    let inside_managed_root = [&paths.images, &paths.files].iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|root| canonical.starts_with(&root))
            .unwrap_or(false)
    });
    if !inside_managed_root {
        return Err("clipboard item resource is outside managed storage".to_string());
    }
    Ok(canonical)
}

#[tauri::command]
pub fn save_clipboard_item_file(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    id: String,
    dst: String,
) -> Result<(), String> {
    let item = database
        .get_item(&id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "clipboard item not found".to_string())?;
    let source = item
        .resource_path
        .ok_or_else(|| "clipboard item has no file resource".to_string())?;
    let source_path = managed_source_path(paths.inner(), &source)?;
    let destination = std::path::Path::new(&dst);
    if !destination.is_absolute() {
        return Err("destination must be an absolute path".to_string());
    }
    if destination.is_dir() {
        return Err("destination is a directory".to_string());
    }
    if destination == source_path {
        return Ok(());
    }
    std::fs::copy(&source_path, destination)
        .map_err(|error| format!("failed to save file: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    // Only real web URLs may reach the OS opener. The URL text can originate
    // from clipboard content, so anything else (`file://`, bare paths,
    // `javascript:`) must never be handed to `open::that`.
    let trimmed = url.trim();
    // RFC 3986 schemes are case-insensitive; accept any casing but still
    // require an http(s) scheme.
    let scheme_ok = ["http://", "https://"].iter().any(|scheme| {
        trimmed
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
    });
    if !scheme_ok {
        return Err("only http(s) URLs can be opened".to_string());
    }
    open::that(trimmed).map_err(|e| format!("failed to open URL: {e}"))
}

#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("file not found".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("explorer: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        open::that(p.parent().unwrap_or(p)).map_err(|e| format!("open: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_file_name_uses_icon_key_for_windows_layout() {
        assert_eq!(app_icon_file_name("Google Chrome"), "google_chrome.png");
        assert_eq!(
            app_icon_file_name("Visual Studio Code"),
            "visual_studio_code.png"
        );
        assert_eq!(app_icon_file_name("1Password"), "1password.png");
    }

    #[test]
    fn app_icon_file_name_falls_back_to_hash_when_key_is_empty() {
        let name = app_icon_file_name("!!!");
        assert!(name.ends_with(".png"));
        assert!(name.len() > 4);
    }

    fn touch(dir: &std::path::Path, name: &str, bytes: &[u8]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    /// Minimal 1x1 transparent PNG used to prove the decode gate accepts real
    /// raster images.
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn replace_source_dir(label: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("icon-replace-test-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn validate_replace_source_accepts_a_decodable_png() {
        let dir = replace_source_dir("ok");
        touch(&dir, "pick.png", TINY_PNG);
        assert!(validate_replace_source(&dir.join("pick.png")).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_replace_source_rejects_non_image_extension() {
        let dir = replace_source_dir("ext");
        touch(&dir, "notes.txt", b"secret");
        let err = validate_replace_source(&dir.join("notes.txt")).unwrap_err();
        assert!(err.contains("only image files"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_replace_source_rejects_undecodable_raster_bytes() {
        let dir = replace_source_dir("bytes");
        touch(&dir, "fake.png", b"this is not png data");
        let err = validate_replace_source(&dir.join("fake.png")).unwrap_err();
        assert!(err.contains("not a decodable image"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_replace_source_rejects_missing_and_oversized_files() {
        let dir = replace_source_dir("size");
        let missing = validate_replace_source(&dir.join("gone.png")).unwrap_err();
        assert!(missing.contains("not found"));
        let big = vec![0u8; (REPLACE_MAX_SOURCE_BYTES + 1) as usize];
        touch(&dir, "big.png", &big);
        let err = validate_replace_source(&dir.join("big.png")).unwrap_err();
        assert!(err.contains("10 MiB"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn save_file_test_project(label: &str) -> std::path::PathBuf {
        let project =
            std::env::temp_dir().join(format!("save-file-test-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&project);
        project
    }

    #[test]
    fn managed_source_path_accepts_a_file_under_the_image_root() {
        let project = save_file_test_project("ok");
        let paths = StoragePaths::initialize(project.clone()).unwrap();
        touch(&paths.images, "a.png", b"img");

        let resolved =
            managed_source_path(&paths, &paths.images.join("a.png").to_string_lossy()).unwrap();

        assert_eq!(resolved.file_name().unwrap(), "a.png");
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn managed_source_path_rejects_a_file_outside_managed_roots() {
        let project = save_file_test_project("outside");
        let paths = StoragePaths::initialize(project.clone()).unwrap();
        touch(&project, "secret.txt", b"secret");

        let err =
            managed_source_path(&paths, &project.join("secret.txt").to_string_lossy()).unwrap_err();

        assert!(err.contains("outside managed storage"), "{err}");
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn managed_source_path_rejects_a_missing_file() {
        let project = save_file_test_project("missing");
        let paths = StoragePaths::initialize(project.clone()).unwrap();

        let err = managed_source_path(&paths, "does-not-exist.png").unwrap_err();

        assert!(err.contains("not found"), "{err}");
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn open_external_url_rejects_multibyte_text_without_panicking() {
        // 9 bytes with char boundaries at 0/3/6/9: slicing at byte 7 would
        // panic, so the scheme check must use a char-boundary-safe accessor.
        assert!(open_external_url("中中中".to_owned()).is_err());
        assert!(open_external_url("javascript:alert(1)".to_owned()).is_err());
    }

    #[test]
    fn merged_entries_distinguish_apps_with_icon_orphans_and_iconless_apps() {
        let dir = std::env::temp_dir().join(format!("icon-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        touch(&dir, "google_chrome.png", b"icon1");
        touch(&dir, "orphan_stray.png", b"icon2");

        let apps = vec![
            (
                "Google Chrome".to_owned(),
                Some("google_chrome.png".to_owned()),
            ),
            ("Edge".to_owned(), None),
            ("Notepad".to_owned(), None),
        ];

        let entries = build_icon_cache(&dir, apps);

        let chrome = entries
            .iter()
            .find(|e| e.display_name == "Google Chrome")
            .unwrap();
        assert_eq!(chrome.app_name.as_deref(), Some("Google Chrome"));
        assert_eq!(chrome.icon_name.as_deref(), Some("google_chrome.png"));
        assert_eq!(chrome.size_bytes, 5);
        assert_eq!(chrome.target_icon_name, "google_chrome.png");

        let edge = entries.iter().find(|e| e.display_name == "Edge").unwrap();
        assert_eq!(edge.app_name.as_deref(), Some("Edge"));
        assert_eq!(edge.icon_name, None);
        assert_eq!(edge.size_bytes, 0);
        assert_eq!(edge.first_char, "E");

        let orphan = entries.iter().find(|e| e.app_name.is_none()).unwrap();
        assert_eq!(orphan.display_name, "orphan_stray");
        assert_eq!(orphan.icon_name.as_deref(), Some("orphan_stray.png"));
        assert_eq!(orphan.size_bytes, 5);
        assert_eq!(orphan.target_icon_name, "orphan_stray.png");

        // recorded apps sort before orphan files
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].display_name, "Edge");
        assert_eq!(entries[1].display_name, "Google Chrome");
        assert_eq!(entries[2].display_name, "Notepad");
        assert_eq!(entries[3].app_name, None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
