use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use crate::config::ConfigStore;
use crate::storage::{
    ClipboardRepository, Database, StorageFileReferences, StoragePaths, RESOURCE_ROOT_MARKER,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCleanupResult {
    pub(crate) removed_files: u64,
    pub(crate) freed_bytes: u64,
}

#[tauri::command]
pub fn enforce_history_cleanup(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<u64, String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    enforce_history_cleanup_for(&database, &guard, &paths, Duration::ZERO)
}

pub fn enforce_history_cleanup_for(
    database: &Database,
    config: &ConfigStore,
    paths: &StoragePaths,
    orphan_file_grace: Duration,
) -> Result<u64, String> {
    let retention_days = config.retention_days();
    let max_items = config.max_items();
    let recycle_bin_days = config.recycle_bin_days();
    let mut total_deleted = 0u64;
    total_deleted += database
        .delete_older_than(retention_days)
        .map_err(|error| error.to_string())?;
    total_deleted += database
        .enforce_capacity_limit(max_items as u64)
        .map_err(|error| error.to_string())?;
    total_deleted += database
        .permanently_delete_expired(recycle_bin_days)
        .map_err(|error| error.to_string())?;
    total_deleted += database
        .cleanup_orphan_search_index()
        .map_err(|error| error.to_string())?;

    let _ = cleanup_orphan_storage_files_with_grace(database, paths, orphan_file_grace);

    Ok(total_deleted)
}

pub fn cleanup_orphan_storage_files(
    database: &Database,
    paths: &StoragePaths,
) -> Result<StorageCleanupResult, String> {
    cleanup_orphan_storage_files_with_grace(database, paths, Duration::ZERO)
}

pub fn cleanup_orphan_storage_files_with_grace(
    database: &Database,
    paths: &StoragePaths,
    orphan_file_grace: Duration,
) -> Result<StorageCleanupResult, String> {
    let references = database
        .list_storage_file_references()
        .map_err(|error| error.to_string())?;
    let icons = paths.storage.join("icons");
    let referenced_paths = resolve_storage_file_references(paths, &icons, references);

    let mut removed_files = 0u64;
    let mut freed_bytes = 0u64;

    let scan_dirs: &[(&Path, bool)] = &[
        (&paths.images, paths.image_cleanup_enabled),
        (&paths.previews, paths.image_cleanup_enabled),
        (&paths.files, paths.file_cleanup_enabled),
        (&icons, true),
    ];

    for (dir, cleanup_enabled) in scan_dirs {
        if !cleanup_enabled {
            crate::log_event!(
                "[cleanup] skipping unowned resource directory {}",
                dir.display()
            );
            continue;
        }
        if !dir.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                continue;
            }
            if entry_path
                .file_name()
                .is_some_and(|name| name == RESOURCE_ROOT_MARKER)
            {
                continue;
            }
            if referenced_paths.contains(&normalized_cleanup_path(&entry_path)) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !orphan_file_grace.is_zero() {
                let Ok(modified_at) = metadata.modified() else {
                    continue;
                };
                if modified_at.elapsed().unwrap_or_default() < orphan_file_grace {
                    continue;
                }
            }
            freed_bytes += metadata.len();
            if let Err(e) = std::fs::remove_file(&entry_path) {
                crate::log_event!(
                    "[cleanup] failed to remove orphan file {}: {e}",
                    entry_path.display()
                );
            } else {
                removed_files += 1;
            }
        }
    }

    Ok(StorageCleanupResult {
        removed_files,
        freed_bytes,
    })
}

#[tauri::command]
pub fn cleanup_storage_files(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<StorageCleanupResult, String> {
    cleanup_orphan_storage_files(&database, &paths)
}

fn resolve_storage_file_references(
    paths: &StoragePaths,
    icons: &Path,
    references: StorageFileReferences,
) -> HashSet<PathBuf> {
    let mut resolved = HashSet::new();
    extend_cleanup_references(
        &mut resolved,
        references.resource_paths,
        &[&paths.storage, &paths.images, &paths.files],
    );
    extend_cleanup_references(
        &mut resolved,
        references.preview_paths,
        &[&paths.storage, &paths.images, &paths.previews],
    );
    extend_cleanup_references(
        &mut resolved,
        references.icon_paths,
        &[&paths.storage, icons],
    );
    resolved
}

fn extend_cleanup_references(
    resolved: &mut HashSet<PathBuf>,
    references: Vec<String>,
    relative_bases: &[&Path],
) {
    for reference in references {
        if reference.trim().is_empty() {
            continue;
        }
        let path = Path::new(&reference);
        if path.is_absolute() {
            resolved.insert(normalized_cleanup_path(path));
        } else {
            resolved.extend(
                relative_bases
                    .iter()
                    .map(|base| normalized_cleanup_path(&base.join(&reference))),
            );
        }
    }
}

fn normalized_cleanup_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
