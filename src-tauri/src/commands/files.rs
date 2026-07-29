use serde::Serialize;

use crate::storage::StoragePaths;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconFileInfo {
    name: String,
    size_bytes: u64,
}

#[tauri::command]
pub fn list_icon_files(paths: tauri::State<'_, StoragePaths>) -> Result<Vec<IconFileInfo>, String> {
    let icons_dir = paths.storage.join("icons");
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&icons_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "png") {
                if let Ok(meta) = entry.metadata() {
                    files.push(IconFileInfo {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        size_bytes: meta.len(),
                    });
                }
            }
        }
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

#[tauri::command]
pub fn delete_icon_files(
    paths: tauri::State<'_, StoragePaths>,
    names: Vec<String>,
) -> Result<u64, String> {
    let icons_dir = paths.storage.join("icons");
    let mut deleted = 0u64;
    for name in &names {
        let path = icons_dir.join(name);
        if path.extension().is_some_and(|e| e == "png") && path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[tauri::command]
pub fn copy_file_to(src: String, dst: String) -> Result<(), String> {
    std::fs::copy(&src, &dst)
        .map(|_| ())
        .map_err(|e| format!("copy failed: {e}"))
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| format!("failed to open URL: {e}"))
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
