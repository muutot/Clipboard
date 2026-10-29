//! Display/file rename commands and their path-rewriting helpers.

use rusqlite::params;

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
    rename_item_record_with_move(database, paths, id, new_name, |from, to| {
        std::fs::rename(from, to)
    })
}

fn rename_item_record_with_move(
    database: &Database,
    paths: &StoragePaths,
    id: String,
    new_name: String,
    move_file: impl FnOnce(&std::path::Path, &std::path::Path) -> std::io::Result<()>,
) -> Result<ClipboardItem, String> {
    // The maintenance mutex is local to one instance. Keep publication active
    // through the DB-first move and any rollback so other instances cannot
    // delete the temporarily unreferenced old path after save_item returns.
    let _publication = database.begin_resource_write().map_err(|e| e.to_string())?;
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
                    if let Err(e) = move_file(old, &new_path) {
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
        // User-initiated renames bypass the content-dedup upsert title guard so a
        // second rename of the same item is not silently discarded. Persist with
        // a direct UPDATE by id instead of save_item; customTitle stays true so
        // future re-captures still cannot overwrite the user title.
        let metadata = updated.metadata_json.clone();
        let rows = database
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE clipboard_items
                 SET title = ?2, metadata_json = COALESCE(?3, metadata_json)
                 WHERE id = ?1 AND deleted = 0 AND kind IN ('text', 'link')",
                        params![updated.id, updated.title, metadata],
                    )
                    .map_err(crate::storage::StorageError::from)
            })
            .map_err(|e| e.to_string())?;
        // The row can disappear between the earlier read and this UPDATE when
        // another window deletes it concurrently; report that instead of a
        // silent fake success.
        if rows == 0 {
            return Err("item not found".to_string());
        }
        return Ok(updated);
    }
    database.save_item(&updated).map_err(|e| e.to_string())?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_preserves_the_old_file_until_move_or_rollback_finishes() {
        for fail_move in [false, true] {
            let root = std::env::temp_dir()
                .join(format!("clipboard-rename-cleanup-{}", uuid::Uuid::new_v4()));
            let paths = StoragePaths::initialize(root.clone()).unwrap();
            let database = Database::open(&paths.database).unwrap();
            // An independent Clipboard instance has its own maintenance mutex.
            // Its cleanup still uses this database and the real orphan scanner.
            let cleaner = Database::open(&paths.database).unwrap();
            let old = paths.files.join("old.txt");
            std::fs::write(&old, b"fixture").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&old)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(3600))
                .unwrap();
            let item = ClipboardItem {
                id: "renamed".into(),
                kind: ClipboardKind::File,
                title: "old".into(),
                text_content: None,
                html_content: None,
                rtf_content: None,
                resource_path: Some(old.display().to_string()),
                preview_path: None,
                content_hash: "fixture".into(),
                source_app: None,
                size_bytes: 7,
                created_at_ms: 1,
                last_used_at_ms: None,
                is_favorite: false,
                icon_path: None,
                metadata_json: None,
            };
            database.save_item(&item).unwrap();
            let result = rename_item_record_with_move(
                &database,
                &paths,
                item.id.clone(),
                "new".into(),
                |from, to| {
                    assert_eq!(
                        cleaner
                            .get_item(&item.id)
                            .unwrap()
                            .unwrap()
                            .resource_path
                            .as_deref(),
                        to.to_str()
                    );
                    let cleanup =
                        crate::commands::cleanup::cleanup_orphan_storage_files_with_grace(
                            &cleaner,
                            &paths,
                            crate::commands::cleanup::ORPHAN_FILE_GRACE,
                        )
                        .unwrap();
                    assert_eq!(
                        cleanup.removed_files, 0,
                        "cleanup must retain the file between the path commit and move"
                    );
                    assert!(from.is_file());
                    if fail_move {
                        Err(std::io::Error::other("injected move failure"))
                    } else {
                        std::fs::rename(from, to)
                    }
                },
            );
            let stored = database.get_item(&item.id).unwrap().unwrap();
            if fail_move {
                assert!(result.unwrap_err().contains("injected move failure"));
                assert_eq!(stored.resource_path, item.resource_path);
                assert!(old.is_file());
            } else {
                assert!(result.is_ok());
                assert_eq!(
                    stored.resource_path.as_deref(),
                    paths.files.join("new.txt").to_str()
                );
                assert!(!old.exists());
            }
            assert_eq!(
                std::fs::read(stored.resource_path.unwrap()).unwrap(),
                b"fixture"
            );
            drop(cleaner);
            drop(database);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
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
