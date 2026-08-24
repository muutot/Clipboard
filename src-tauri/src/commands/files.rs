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
    if !source.is_file() {
        return Err("source file not found".to_string());
    }
    std::fs::copy(source, &target).map_err(|e| format!("failed to replace icon: {e}"))?;
    Ok(())
}

// NOTE: the former `copy_file_to` command (an unrestricted src→dst file copy
// reachable from the webview) was removed: it had no frontend caller and was
// an arbitrary-file-copy primitive. `replace_icon_file` is the constrained
// replacement for the one legitimate use case.

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    // Only real web URLs may reach the OS opener. The URL text can originate
    // from clipboard content, so anything else (`file://`, bare paths,
    // `javascript:`) must never be handed to `open::that`.
    let trimmed = url.trim();
    // RFC 3986 schemes are case-insensitive; accept any casing but still
    // require an http(s) scheme.
    let scheme_ok = ["http://", "https://"].iter().any(|scheme| {
        trimmed.len() >= scheme.len() && trimmed[..scheme.len()].eq_ignore_ascii_case(scheme)
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
