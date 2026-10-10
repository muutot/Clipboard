//! Application icon cache: the inventory shown in the icon settings panel and
//! the delete command that prunes orphan icon files.

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
pub(super) fn app_icon_file_name(app: &str) -> String {
    let key = icon_key(app);
    if key.is_empty() {
        return format!("{}.png", compute_content_hash("icon", app, None));
    }
    format!("{key}.png")
}

pub(super) fn build_icon_cache(
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
            for ext in ["png", "ico", "svg", "jpg", "jpeg", "webp"] {
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
        let in_icons_dir = match (path.canonicalize(), icons_dir.canonicalize()) {
            (Ok(resolved), Ok(icons_root)) => resolved.starts_with(icons_root),
            // Fail closed: if either side cannot be canonicalized we cannot
            // prove containment, so refuse the delete.
            _ => false,
        };
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
