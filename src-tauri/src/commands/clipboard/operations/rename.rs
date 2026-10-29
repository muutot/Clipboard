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
    // Keep maintenance ordered with resource-path changes. Publication also
    // protects the new file until its row commits across independent instances.
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
    rename_item_record_with_copy(database, paths, id, new_name, copy_rename_resource)
}

fn rename_item_record_with_copy(
    database: &Database,
    paths: &StoragePaths,
    id: String,
    new_name: String,
    copy_file: impl FnOnce(&std::path::Path, &std::path::Path) -> std::io::Result<()>,
) -> Result<ClipboardItem, String> {
    // The maintenance mutex is local to one instance. Keep publication active
    // until the copied file's new paths commit. Existing paths remain intact
    // for writers that read them before this rename's reference-count check.
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
            // records (dedup, duplicates, previews). Known shared files receive
            // a display-title change only. A currently unshared file is copied
            // to the new name; a concurrent writer may already have read the
            // old path without committing its reference yet.
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
                    // Finish the copy before publishing any new DB paths.
                    // Keep the old file for in-flight duplicates/captures;
                    // orphan cleanup reclaims it once no writer or row uses it.
                    // A failed save leaves only an unreferenced complete copy.
                    copy_file(old, &new_path).map_err(|e| format!("rename failed: {e}"))?;
                    let old_path_string = old.to_string_lossy().to_string();
                    let new_path_string = new_path.to_string_lossy().to_string();
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

/// Create a new complete body without overwriting a concurrent rename target.
/// The destination is unreferenced until the copy and flush both succeed.
fn copy_rename_resource(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    let mut source = std::fs::File::open(from)?;
    let mut destination = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    let result = std::io::copy(&mut source, &mut destination).and_then(|_| destination.sync_all());
    drop(destination);
    if result.is_err() {
        let _ = std::fs::remove_file(to);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_copy_never_overwrites_an_existing_destination() {
        let root = std::env::temp_dir().join(format!(
            "clipboard-rename-conflict-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.txt");
        let target = root.join("target.txt");
        std::fs::write(&source, b"source bytes").unwrap();
        std::fs::write(&target, b"other rename's bytes").unwrap();
        assert_eq!(
            copy_rename_resource(&source, &target).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists,
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"other rename's bytes");
        assert_eq!(std::fs::read(&source).unwrap(), b"source bytes");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_preserves_old_and_new_files_until_paths_commit() {
        for (fail_copy, fail_save) in [(false, false), (true, false), (false, true)] {
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
            if fail_save {
                database
                    .with_connection(|connection| {
                        connection.execute_batch(
                            "CREATE TRIGGER reject_rename BEFORE UPDATE ON clipboard_items
                         BEGIN SELECT RAISE(ABORT, 'injected save failure'); END;",
                        )?;
                        Ok(())
                    })
                    .unwrap();
            }
            let result = rename_item_record_with_copy(
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
                        from.to_str()
                    );
                    if !fail_copy {
                        copy_rename_resource(from, to)?;
                        std::fs::File::options()
                            .write(true)
                            .open(to)?
                            .set_modified(
                                std::time::UNIX_EPOCH + std::time::Duration::from_secs(3600),
                            )?;
                    }
                    let cleanup =
                        crate::commands::cleanup::cleanup_orphan_storage_files_with_grace(
                            &cleaner,
                            &paths,
                            crate::commands::cleanup::ORPHAN_FILE_GRACE,
                        )
                        .unwrap();
                    assert_eq!(
                        cleanup.removed_files, 0,
                        "cleanup must retain both bodies before the path commit"
                    );
                    assert!(from.is_file());
                    if fail_copy {
                        Err(std::io::Error::other("injected copy failure"))
                    } else {
                        Ok(())
                    }
                },
            );
            let stored = database.get_item(&item.id).unwrap().unwrap();
            if fail_copy || fail_save {
                let message = result.unwrap_err();
                assert!(message.contains(if fail_copy {
                    "injected copy failure"
                } else {
                    "injected save failure"
                }));
                assert_eq!(stored.resource_path, item.resource_path);
                assert_eq!(stored.title, item.title);
                assert!(old.is_file());
                if fail_save {
                    let cleanup =
                        crate::commands::cleanup::cleanup_orphan_storage_files_with_grace(
                            &cleaner,
                            &paths,
                            std::time::Duration::ZERO,
                        )
                        .unwrap();
                    assert_eq!(cleanup.removed_files, 1);
                    assert!(!paths.files.join("new.txt").exists());
                    assert!(old.is_file());
                }
            } else {
                assert!(result.is_ok());
                assert_eq!(
                    stored.resource_path.as_deref(),
                    paths.files.join("new.txt").to_str()
                );
                assert!(old.is_file());
                let cleanup = crate::commands::cleanup::cleanup_orphan_storage_files_with_grace(
                    &cleaner,
                    &paths,
                    std::time::Duration::ZERO,
                )
                .unwrap();
                assert_eq!(cleanup.removed_files, 1);
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
    use crate::storage::{
        ResourceRootRole, RESOURCE_ROOT_DATABASES, RESOURCE_ROOT_LOCK, RESOURCE_ROOT_MARKER,
    };
    if source.file_name().is_some_and(|name| {
        name == RESOURCE_ROOT_MARKER
            || name == RESOURCE_ROOT_LOCK
            || name == RESOURCE_ROOT_DATABASES
    }) || !std::fs::symlink_metadata(source).is_ok_and(|meta| meta.file_type().is_file())
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
