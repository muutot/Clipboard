//! Display/file rename commands and their path-rewriting helpers.

use super::*;

#[tauri::command]
pub fn rename_item(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    capture: tauri::State<'_, CaptureState>,
    id: String,
    new_name: String,
) -> Result<ClipboardItem, String> {
    // The rename persists the new paths before moving the file. Hold the
    // storage maintenance lock so a concurrent orphan cleanup cannot judge the
    // not-yet-moved old file unreferenced, delete it, and leave the
    // rolled-back record pointing at a missing file.
    let _maintenance = lock_state(
        &capture.storage_maintenance_lock,
        "storage maintenance lock is poisoned",
    )?;
    let renamed = rename_item_record(&database, &paths, id, new_name)?;
    // Other windows render the title and paths, so they need the renamed row.
    broadcast_content_changed(&app, &database, std::slice::from_ref(&renamed.id));
    Ok(renamed)
}

pub(super) fn rename_item_record(
    database: &Database,
    paths: &StoragePaths,
    id: String,
    new_name: String,
) -> Result<ClipboardItem, String> {
    let items = database
        .get_items_by_ids(std::slice::from_ref(&id))
        .map_err(|e| e.to_string())?;
    let item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    if new_name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let sanitized_name = sanitize_file_stem(new_name.trim());
    if sanitized_name.is_empty() {
        return Err("name contains no usable characters".to_string());
    }
    let mut updated = item.clone();
    if item.kind == ClipboardKind::Image || item.kind == ClipboardKind::File {
        if let Some(ref old_path) = item.resource_path {
            let old = std::path::Path::new(old_path);
            // Storage files are content-hash-named and can be shared by several
            // records (dedup, duplicates, previews). Only rename the physical
            // file when this record is its sole owner; otherwise the rename
            // would break the other records sharing it. In that case fall back
            // to renaming the display title only.
            let shared = database
                .resource_reference_count(old_path, &id)
                .map_err(|e| e.to_string())?
                > 0;
            if is_owned_rename_source(paths, old) && !shared {
                let ext = old.extension().unwrap_or_default().to_string_lossy();
                let parent = old.parent().unwrap_or(std::path::Path::new("."));
                // The new name arrives from the webview and must never be able
                // to escape the managed directory via separators or traversal.
                // An empty extension must not produce a trailing dot: Windows
                // strips it from the real file name, so the DB record would
                // point at a path that does not exist on disk.
                let candidate = if ext.is_empty() {
                    sanitized_name.clone()
                } else {
                    format!("{sanitized_name}.{ext}")
                };
                if std::path::Path::new(&candidate)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .as_deref()
                    != Some(candidate.as_str())
                {
                    return Err("invalid file name".to_string());
                }
                let new_path = parent.join(candidate);
                if new_path != old {
                    if new_path.exists() {
                        return Err(format!("file already exists: {}", new_path.display()));
                    }
                    // Persist the new paths FIRST, then move the file. A
                    // failed database write leaves the disk untouched; a
                    // failed rename is rolled back in the database so the
                    // record never points at a missing file.
                    let old_path_string = old.to_string_lossy().to_string();
                    let new_path_string = new_path.to_string_lossy().to_string();
                    let rollback = updated.clone();
                    updated.resource_path = Some(new_path_string.clone());
                    // preview_path either points at the dedicated thumbnail
                    // under previews/ or falls back to the original image
                    // path until the worker fills it in. Only refresh the
                    // fallback; overwriting a generated thumbnail link would
                    // lose it and orphan the preview file.
                    if updated.preview_path.as_deref() == Some(old_path_string.as_str()) {
                        updated.preview_path = Some(new_path_string.clone());
                    }
                    // The detail panel and multi-file paste read the managed
                    // path out of `metadata_json`/`text_content` before
                    // `resource_path`, so those copies must move with the file.
                    rewrite_stored_resource_paths(&mut updated, &old_path_string, &new_path_string);
                    database.save_item(&updated).map_err(|e| e.to_string())?;
                    if let Err(e) = std::fs::rename(old, &new_path) {
                        database.save_item(&rollback).map_err(|rollback_error| {
                            format!(
                                "rename failed ({e}) and the database rollback also failed ({rollback_error})"
                            )
                        })?;
                        return Err(format!("rename failed: {e}"));
                    }
                }
            }
        }
    }
    updated.title = new_name.trim().to_string();
    if matches!(updated.kind, ClipboardKind::Text | ClipboardKind::Link) {
        updated.metadata_json = Some(set_custom_title_metadata(
            updated.metadata_json.as_deref(),
            true,
        )?);
    }
    database.save_item(&updated).map_err(|e| e.to_string())?;
    Ok(updated)
}

/// Pass-through originals and unclaimed roots are not ours to rename.
/// Canonical containment rejects traversal and directory links escaping a root;
/// symlink metadata rejects a linked leaf and directories themselves.
pub(super) fn is_owned_rename_source(paths: &StoragePaths, source: &std::path::Path) -> bool {
    use crate::storage::{ResourceRootRole, RESOURCE_ROOT_MARKER};
    if source
        .file_name()
        .is_some_and(|name| name == RESOURCE_ROOT_MARKER)
        || !std::fs::symlink_metadata(source).is_ok_and(|meta| meta.file_type().is_file())
    {
        return false;
    }
    let Ok(source) = source.canonicalize() else {
        return false;
    };
    [
        (
            &paths.images,
            paths.image_cleanup_enabled,
            paths.image_marker_required,
            ResourceRootRole::Image,
        ),
        (
            &paths.files,
            paths.file_cleanup_enabled,
            paths.file_marker_required,
            ResourceRootRole::File,
        ),
    ]
    .into_iter()
    .any(|(root, enabled, marker_required, role)| {
        enabled
            && (!marker_required || paths.resource_root_marker_valid(role))
            && root
                .canonicalize()
                .is_ok_and(|root| source.starts_with(root))
    })
}

/// Restricts a webview-provided file name stem to characters that cannot
/// alter the destination directory (`/`, `\`, `:`) or form a Windows device
/// name, and strips trailing dots/spaces (illegal on Windows). Returns an
/// empty string when nothing usable remains.
pub(super) fn sanitize_file_stem(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if (character as u32) < 0x20 || character == '\u{7f}' => '_',
            other => other,
        })
        .collect();
    let trimmed = sanitized.trim_end_matches(['.', ' ']);
    if is_windows_reserved_device_name(trimmed) {
        return "_".to_owned();
    }
    trimmed.to_owned()
}

fn is_windows_reserved_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    ["COM", "LPT"].iter().any(|prefix| {
        upper.starts_with(prefix)
            && !upper[prefix.len()..].is_empty()
            && upper[prefix.len()..]
                .chars()
                .all(|character| character.is_ascii_digit())
    })
}

/// Rewrites every exact occurrence of `old_path` inside a renamed record's
/// `metadata_json` and file-list `text_content`. `rename_item` updates
/// `resource_path`/`preview_path`, but the detail panel and multi-file paste
/// read the managed path out of those fields first, so a rename that skipped
/// them would point the UI at a file that no longer exists.
pub(super) fn rewrite_stored_resource_paths(
    item: &mut ClipboardItem,
    old_path: &str,
    new_path: &str,
) {
    if let Some(metadata) = item.metadata_json.as_deref() {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(metadata) {
            replace_path_strings(&mut value, old_path, new_path);
            if let Ok(serialized) = serde_json::to_string(&value) {
                item.metadata_json = Some(serialized);
            }
        }
    }
    if let Some(text) = item.text_content.as_deref() {
        if let Ok(mut paths) = serde_json::from_str::<Vec<String>>(text) {
            let mut changed = false;
            for path in &mut paths {
                if path == old_path {
                    *path = new_path.to_owned();
                    changed = true;
                }
            }
            if changed {
                if let Ok(serialized) = serde_json::to_string(&paths) {
                    item.text_content = Some(serialized);
                }
            }
        }
    }
}

/// Recursively replaces string values equal to `old_path`. Exact matches keep
/// unrelated paths (e.g. the other files of a multi-file capture) untouched.
fn replace_path_strings(value: &mut serde_json::Value, old_path: &str, new_path: &str) {
    match value {
        serde_json::Value::String(text) => {
            if text == old_path {
                *text = new_path.to_owned();
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                replace_path_strings(item, old_path, new_path);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                replace_path_strings(item, old_path, new_path);
            }
        }
        _ => {}
    }
}
