use serde::Serialize;

use crate::content::{compute_content_hash, compute_normalized_media_hash, icon_key};
use crate::domain::{ClipboardItem, ClipboardKind};
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

/// Resolves the existing local files behind an image/file record. Image items
/// use the stored png; file items re-reference the still-existing original
/// file for each entry (so pasting keeps the original name) and fall back to
/// the managed storage copy / the record's resource path when the original is
/// gone. When `database` is given and the original for a managed copy is gone,
/// the freshest other record sharing that managed file is consulted so the
/// pasted name stays the one from the most recent copy of the same content.
/// Pass-through files that exceed the copy-size limit keep their original
/// absolute location and are accepted here: the paths originate from the
/// record, never from the webview.
fn resolve_clipboard_file_paths(
    item: &ClipboardItem,
    paths: &StoragePaths,
    database: Option<&Database>,
) -> Result<Vec<std::path::PathBuf>, String> {
    let mut candidates = Vec::new();
    match item.kind {
        ClipboardKind::Image => {
            if let Some(resource) = item.resource_path.as_deref() {
                candidates.push(resource.to_owned());
            }
        }
        ClipboardKind::File => {
            if let Some(entries) = file_metadata_entries(&item.metadata_json) {
                for (storage, original) in entries {
                    // OS clipboard semantics: re-reference the original file so
                    // the pasted copy keeps its original name instead of the
                    // managed (hash-named) storage copy. The managed copy stays
                    // the fallback when the original no longer exists on disk;
                    // with a database, the latest copy of the same content then
                    // donates its recorded original name.
                    if let Some(original) = original {
                        if std::path::Path::new(&original).is_file() {
                            candidates.push(original);
                            continue;
                        }
                    }
                    if let Some(storage) = storage {
                        if let Some(database) = database {
                            if let Some(inherited) =
                                latest_copied_original_path(database, &storage, &item.id)
                            {
                                candidates.push(inherited.to_string_lossy().to_string());
                                continue;
                            }
                        }
                        candidates.push(storage);
                    }
                }
            }
            if candidates.is_empty() {
                if let Some(resource) = item.resource_path.as_deref() {
                    candidates.push(resource.to_owned());
                }
            }
        }
        _ => return Err("clipboard item is not an image or file".to_string()),
    }

    let mut resolved = Vec::new();
    for candidate in &candidates {
        let raw = std::path::Path::new(candidate);
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            [&paths.images, &paths.files, &paths.storage]
                .iter()
                .map(|root| root.join(raw))
                .find(|candidate| candidate.is_file())
                .unwrap_or_else(|| paths.storage.join(raw))
        };
        if path.is_file() {
            resolved.push(path);
        }
    }

    if resolved.is_empty() {
        return Err("clipboard item has no available files on disk".to_string());
    }
    Ok(resolved)
}

/// Locates the original file recorded by the most recent copy of the same
/// managed file (excluding `exclude_id`) and returns its path only when it
/// still exists on disk. Content-storage dedup guarantees a matching record
/// holds the identical bytes, so its original path is the last name given to
/// this content by the user.
fn latest_copied_original_path(
    database: &Database,
    storage: &str,
    exclude_id: &str,
) -> Option<std::path::PathBuf> {
    let record = database
        .latest_file_record_referencing_storage(storage, exclude_id)
        .ok()??;
    let original = file_metadata_entries(&record.metadata_json)?
        .into_iter()
        .find_map(
            |(entry_storage, entry_original)| match (entry_storage, entry_original) {
                (Some(entry_storage), Some(entry_original))
                    if entry_storage.as_str() == storage =>
                {
                    Some(entry_original)
                }
                _ => None,
            },
        )?;
    let path = std::path::PathBuf::from(&original);
    path.is_file().then_some(path)
}

/// Reads the `(storagePath, originalPath)` pair (legacy `path` for storage) of
/// each entry in the `files` array of the record's resource metadata. The
/// original path is optional: it is absent when the source is not a file on
/// disk (for example an in-memory screenshot).
fn file_metadata_entries(
    metadata_json: &Option<String>,
) -> Option<Vec<(Option<String>, Option<String>)>> {
    let json = metadata_json.as_deref()?;
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
    let files = parsed.get("files")?.as_array()?;
    let mut entries = Vec::new();
    for file in files {
        let storage = file
            .get("storagePath")
            .or_else(|| file.get("path"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let original = file
            .get("originalPath")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if storage.is_some() || original.is_some() {
            entries.push((storage, original));
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Copies the files behind one image/file record back to the system clipboard
/// as dropped file references (CF_HDROP on Windows). The record id is the only
/// input from the webview; the actual paths are resolved from the database, so
/// no arbitrary-path clipboard primitive is re-exposed.
#[tauri::command]
pub fn copy_clipboard_item_files(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    id: String,
) -> Result<(), String> {
    let item = database
        .get_item(&id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "clipboard item not found".to_string())?;
    let resolved = resolve_clipboard_file_paths(&item, paths.inner(), Some(&database))?;
    let files = resolved
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    crate::platform::platform().write_clipboard_files_with_self_trigger(&files)
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

    fn item(
        kind: ClipboardKind,
        resource_path: Option<String>,
        metadata_json: Option<String>,
    ) -> ClipboardItem {
        ClipboardItem {
            id: "id".to_string(),
            kind,
            title: "title".to_string(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path,
            preview_path: None,
            content_hash: "hash".to_string(),
            source_app: None,
            icon_path: None,
            size_bytes: 0,
            created_at_ms: 0,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json,
        }
    }

    #[test]
    fn resolve_clipboard_file_paths_accepts_an_image_resource() {
        let project = save_file_test_project("media-root");
        let paths = StoragePaths::initialize(project.clone()).unwrap();
        touch(&paths.images, "a.png", b"img");

        let resolved = resolve_clipboard_file_paths(
            &item(
                ClipboardKind::Image,
                Some(paths.images.join("a.png").to_string_lossy().to_string()),
                None,
            ),
            &paths,
            None,
        )
        .unwrap();

        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].ends_with("a.png"));
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn resolve_clipboard_file_paths_prefers_metadata_storage_paths() {
        let project = save_file_test_project("file-list");
        let paths = StoragePaths::initialize(project.clone()).unwrap();
        touch(&paths.files, "a.txt", b"a");
        touch(&paths.files, "b.txt", b"b");
        let metadata = Some(
            serde_json::json!({
                "files": [
                    { "storagePath": paths.files.join("a.txt").to_string_lossy().to_string() },
                    { "storagePath": paths.files.join("b.txt").to_string_lossy().to_string() },
                ]
            })
            .to_string(),
        );

        let resolved =
            resolve_clipboard_file_paths(&item(ClipboardKind::File, None, metadata), &paths, None)
                .unwrap();

        assert_eq!(resolved.len(), 2);
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn resolve_clipboard_file_paths_restores_the_original_file_name() {
        let project = save_file_test_project("original-name");
        let paths = StoragePaths::initialize(project.clone()).unwrap();
        // The managed copy carries a hash-derived name...
        touch(
            &paths.files,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08.txt",
            b"original content",
        );
        // ...while the user's real file still exists under its original name.
        touch(&project, "my report.txt", b"original content");
        let metadata = Some(
            serde_json::json!({
                "files": [{
                    "storagePath": paths.files.join("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08.txt").to_string_lossy().to_string(),
                    "originalPath": project.join("my report.txt").to_string_lossy().to_string(),
                }]
            })
            .to_string(),
        );

        let resolved =
            resolve_clipboard_file_paths(&item(ClipboardKind::File, None, metadata), &paths, None)
                .unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].file_name().unwrap().to_string_lossy(),
            "my report.txt"
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn resolve_clipboard_file_paths_falls_back_to_storage_when_original_is_gone() {
        let project = save_file_test_project("original-gone");
        let paths = StoragePaths::initialize(project.clone()).unwrap();
        touch(&paths.files, "missing-original.txt", b"content");
        let metadata = Some(
            serde_json::json!({
                "files": [{
                    "storagePath": paths.files.join("missing-original.txt").to_string_lossy().to_string(),
                    "originalPath": project.join("gone.txt").to_string_lossy().to_string(),
                }]
            })
            .to_string(),
        );

        let resolved =
            resolve_clipboard_file_paths(&item(ClipboardKind::File, None, metadata), &paths, None)
                .unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].file_name().unwrap().to_string_lossy(),
            "missing-original.txt"
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    /// A full file record persisted via the repository, mirroring how the
    /// capture pipeline stores one managed copy plus its original location.
    fn persisted_file_item(
        id: &str,
        content_hash: &str,
        created_at_ms: i64,
        storage_path: String,
        original_path: String,
    ) -> ClipboardItem {
        let mut item = item(ClipboardKind::File, Some(storage_path.clone()), None);
        item.id = id.to_owned();
        item.content_hash = content_hash.to_owned();
        item.created_at_ms = created_at_ms;
        item.metadata_json = Some(
            serde_json::json!({
                "files": [{
                    "name": original_path.rsplit(['/', '\\']).next().unwrap_or(""),
                    "storagePath": storage_path,
                    "originalPath": original_path,
                }]
            })
            .to_string(),
        );
        item
    }

    #[test]
    fn resolve_clipboard_file_paths_inherits_the_latest_copied_name() {
        let project = save_file_test_project("latest-name");
        let paths = StoragePaths::initialize(project.clone()).unwrap();
        let database = Database::open_in_memory().unwrap();
        // The managed storage copy (hash-named) still exists...
        touch(&paths.files, "a1b2c3d4.txt", b"same content");
        let storage = paths
            .files
            .join("a1b2c3d4.txt")
            .to_string_lossy()
            .to_string();
        // ...but the first copy's original was renamed away afterwards.
        let stale_original = project.join("Old Name.txt").to_string_lossy().to_string();
        // The most recent copy of the same content lives under a new name.
        let latest_original = project
            .join("Current Name.txt")
            .to_string_lossy()
            .to_string();
        touch(&project, "Current Name.txt", b"same content");

        let old = persisted_file_item(
            "file-old",
            "hash-old",
            100,
            storage.clone(),
            stale_original.clone(),
        );
        let new = persisted_file_item(
            "file-new",
            "hash-new",
            200,
            storage.clone(),
            latest_original.clone(),
        );
        database.save_item(&old).unwrap();
        database.save_item(&new).unwrap();

        let resolved = resolve_clipboard_file_paths(&old, &paths, Some(&database)).unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].file_name().unwrap().to_string_lossy(),
            "Current Name.txt"
        );

        // Without a database the stale original falls back to the (non-existent
        // here) managed copy; here it resolves to the managed storage name.
        let fallback = resolve_clipboard_file_paths(&old, &paths, None).unwrap();
        assert_eq!(
            fallback[0].file_name().unwrap().to_string_lossy(),
            "a1b2c3d4.txt"
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn resolve_clipboard_file_paths_accepts_pass_through_absolute_paths() {
        let project = save_file_test_project("passthrough");
        let paths = StoragePaths::initialize(project.clone()).unwrap();
        touch(&project, "large.bin", b"big");
        let original = project.join("large.bin").to_string_lossy().to_string();

        let resolved = resolve_clipboard_file_paths(
            &item(ClipboardKind::File, Some(original), None),
            &paths,
            None,
        )
        .unwrap();

        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].ends_with("large.bin"));
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn resolve_clipboard_file_paths_rejects_text_items_and_missing_files() {
        let project = save_file_test_project("cleanup");
        let paths = StoragePaths::initialize(project.clone()).unwrap();

        assert!(
            resolve_clipboard_file_paths(&item(ClipboardKind::Text, None, None), &paths, None)
                .is_err()
        );
        assert!(resolve_clipboard_file_paths(
            &item(ClipboardKind::File, Some("gone.txt".to_string()), None),
            &paths,
            None
        )
        .is_err());
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn file_metadata_entries_reads_storage_and_original_path_fields() {
        let metadata = Some(
            r#"{"files":[{"storagePath":"C:\\managed\\a.txt","originalPath":"C:\\docs\\a.txt"},{"path":"C:\\orig\\b.txt"},{"name":"c.txt"}]}"#
                .to_owned(),
        );

        assert_eq!(
            file_metadata_entries(&metadata).unwrap(),
            vec![
                (
                    Some("C:\\managed\\a.txt".to_owned()),
                    Some("C:\\docs\\a.txt".to_owned())
                ),
                (Some("C:\\orig\\b.txt".to_owned()), None),
            ]
        );
        assert!(file_metadata_entries(&None).is_none());
        assert!(file_metadata_entries(&Some("{}".to_owned())).is_none());
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
