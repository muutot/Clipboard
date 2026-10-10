//! Save-as and file-reference clipboard writes resolved through managed storage.

use crate::commands::clipboard::ClipboardFilesCopyError;
use crate::state::SelfTriggerState;
use crate::storage::{ClipboardRepository, Database, StoragePaths};

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
pub(super) fn managed_source_path(
    paths: &StoragePaths,
    stored: &str,
) -> Result<std::path::PathBuf, String> {
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

/// Copies the files behind one image/file record back to the system clipboard
/// as dropped file references (CF_HDROP on Windows). The record id is the only
/// input from the webview; the actual paths are resolved from the database, so
/// no arbitrary-path clipboard primitive is re-exposed.
///
/// Rejects with a tagged [`ClipboardFilesCopyError`] so the caller can tell a
/// vanished file from a busy clipboard.
#[tauri::command]
pub fn copy_clipboard_item_files(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    self_trigger: tauri::State<'_, SelfTriggerState>,
    id: String,
) -> Result<(), ClipboardFilesCopyError> {
    let item = database
        .get_item(&id)
        .map_err(|error| ClipboardFilesCopyError::Failed(error.to_string()))?
        .ok_or_else(|| ClipboardFilesCopyError::Failed("clipboard item not found".to_owned()))?;
    crate::item_operations::CopyContext {
        paths: Some(paths.inner().clone()),
        self_trigger: Some(self_trigger.0.clone()),
    }
    .write(&database, &item)
}
