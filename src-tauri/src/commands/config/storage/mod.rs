use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::commands::lock::lock_state;
use crate::config::ConfigStore;
use crate::domain::ClipboardKind;
use crate::keyboard::KeyboardManager;
use crate::platform;
use crate::search::{SearchIndex, SEARCH_INDEX_VERSION};
use crate::storage::{ClipboardRepository, Database, ResourceRootRole, StoragePaths};
use crate::{CaptureState, STORAGE_KIND_DELETE_SCOPE};

use super::{
    ResourceStorageUpdate, StorageConfigInfo, StorageDirectoryUpdate, StorageKindStats,
    StorageStatus,
};

#[tauri::command]
pub fn get_storage_status(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    search_index: tauri::State<'_, Arc<SearchIndex>>,
) -> Result<StorageStatus, String> {
    let config_path = lock_state(&config, "configuration lock is poisoned")?
        .path()
        .display()
        .to_string();
    let keyboard_config_path = lock_state(&keyboard, "keyboard configuration lock is poisoned")?
        .path()
        .display()
        .to_string();

    let disk_space = platform::disk_space(&paths.data_directory);
    let resource_owned = (!paths.image_marker_required
        || paths.resource_root_marker_valid(ResourceRootRole::Image))
        && (!paths.file_marker_required
            || paths.resource_root_marker_valid(ResourceRootRole::File));

    Ok(StorageStatus {
        item_count: database.item_count().map_err(|error| error.to_string())?,
        image_count: database.count_by_kind("image").unwrap_or(0),
        image_size_bytes: database.size_by_kind("image").unwrap_or(0),
        file_count: database.count_by_kind("file").unwrap_or(0),
        file_size_bytes: database.size_by_kind("file").unwrap_or(0),
        text_count: database.count_by_kind("text").unwrap_or(0),
        link_count: database.count_by_kind("link").unwrap_or(0),
        project_path: paths.project.display().to_string(),
        config_path,
        keyboard_config_path,
        data_directory_path: paths.data_directory.display().to_string(),
        uses_custom_data_directory: paths.uses_custom_data_directory(),
        storage_path: paths.storage.display().to_string(),
        icons_dir: paths.storage.join("icons").display().to_string(),
        database_path: paths.database.display().to_string(),
        database_size_bytes: file_or_dir_size(&paths.database),
        files_path: paths.files.display().to_string(),
        image_path: paths.images.display().to_string(),
        image_cleanup_enabled: paths.image_cleanup_enabled,
        file_cleanup_enabled: paths.file_cleanup_enabled,
        resource_ownership_required: paths.image_marker_required || paths.file_marker_required,
        resource_owned,
        search_index_path: paths.search_index.display().to_string(),
        search_index_size_bytes: dir_size(&paths.search_index),
        search_index_version: SEARCH_INDEX_VERSION,
        search_index_rebuild_required: search_index.requires_full_rebuild(),
        disk_total_bytes: disk_space.map(|space| space.total_bytes),
        disk_available_bytes: disk_space.map(|space| space.available_bytes),
    })
}

#[tauri::command]
pub fn get_storage_kind_stats(
    database: tauri::State<'_, Database>,
    kind: ClipboardKind,
) -> Result<StorageKindStats, String> {
    database
        .kind_storage_stats(kind, STORAGE_KIND_DELETE_SCOPE)
        .map(StorageKindStats::from)
        .map_err(|error| error.to_string())
}

/// Async: the directory copy + VACUUM INTO can take minutes and would
/// otherwise freeze the window event loop on the main thread.
#[tauri::command]
pub async fn configure_storage_directory(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    active_paths: tauri::State<'_, StoragePaths>,
    database: tauri::State<'_, Database>,
    capture: tauri::State<'_, CaptureState>,
    app: tauri::AppHandle,
    data_directory: Option<String>,
) -> Result<StorageDirectoryUpdate, String> {
    let requested_directory = data_directory.map(PathBuf::from);
    let (image_storage_path, file_storage_path) = {
        let config = lock_state(&config, "configuration lock is poisoned")?;
        (
            config.image_storage_path().map(PathBuf::from),
            config.file_storage_path().map(PathBuf::from),
        )
    };
    let target_paths = StoragePaths::initialize_with_resource_directories_for_configuration(
        active_paths.project.clone(),
        requested_directory,
        image_storage_path,
        file_storage_path,
    )
    .map_err(|error| error.to_string())?;

    let saved_directory = target_paths
        .uses_custom_data_directory()
        .then(|| target_paths.data_directory.clone());
    let save = || {
        lock_state(&config, "configuration lock is poisoned")?
            .set_storage_directory(saved_directory)
            .map_err(|error| error.to_string())
    };

    if target_paths.data_directory != active_paths.data_directory {
        // Shutdown's bounded auto-sync wait is insufficient for migration.
        // Reject an active manual/automatic run before stopping any services.
        let _sync = crate::commands::sync::try_lock_sync_run().map_err(|error| {
            format!("{error}; wait for sync to finish before migrating storage")
        })?;
        crate::shutdown::stop_runtime_services(&app);
        let previous_paused = capture.is_paused();
        capture.set_paused(true);
        // Drain path-before-rename operations and file-before-record ingestion.
        let _maintenance = lock_state(
            &capture.storage_maintenance_lock,
            "storage maintenance lock is poisoned",
        )?;
        let _ingestion = lock_state(
            &capture.ingestion_guard,
            "clipboard ingestion lock is poisoned",
        )?;
        if let Err(error) = migrate_and_save_storage(&active_paths, &target_paths, &database, save)
        {
            // Restore the caller's pause preference where possible; services
            // stay stopped because the database location may be half-moved.
            capture.set_paused(previous_paused);
            return Err(format!(
                "{error}; background services were stopped during the failed \
                 migration — restart the app before retrying"
            ));
        }
    } else {
        save()?;
    }

    Ok(StorageDirectoryUpdate {
        restart_required: target_paths.data_directory != active_paths.data_directory,
        data_directory_path: target_paths.data_directory.display().to_string(),
        storage_path: target_paths.storage.display().to_string(),
    })
}

#[tauri::command]
pub fn set_resource_storage_paths(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    active_paths: tauri::State<'_, StoragePaths>,
    image_storage_path: Option<String>,
    file_storage_path: Option<String>,
) -> Result<ResourceStorageUpdate, String> {
    let image_storage_path = image_storage_path.and_then(|path| {
        let path = path.trim().to_owned();
        (!path.is_empty()).then(|| PathBuf::from(path))
    });
    let file_storage_path = file_storage_path.and_then(|path| {
        let path = path.trim().to_owned();
        (!path.is_empty()).then(|| PathBuf::from(path))
    });

    let target_paths = StoragePaths::initialize_with_resource_directories_for_configuration(
        active_paths.project.clone(),
        Some(active_paths.data_directory.clone()),
        image_storage_path.clone(),
        file_storage_path.clone(),
    )
    .map_err(|error| error.to_string())?;

    lock_state(&config, "configuration lock is poisoned")?
        .set_resource_storage_paths(image_storage_path, file_storage_path)
        .map_err(|error| error.to_string())?;

    Ok(ResourceStorageUpdate {
        image_storage_path: target_paths.images.display().to_string(),
        file_storage_path: target_paths.files.display().to_string(),
        restart_required: target_paths.images != active_paths.images
            || target_paths.files != active_paths.files,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMarkerUpdate {
    restart_required: bool,
}

/// Enables or disables ownership of the active resource roots by writing or
/// removing the `.clipboard-resource-root` marker that gates orphan cleanup.
/// This is the recovery path for roots that are already in use but unmarked
/// (for example a custom data directory created before marker support). It
/// never deletes files: enabling only takes effect after a restart, and cleanup
/// still preserves everything referenced by the database; disabling removes the
/// marker so cleanup stops scanning the directory.
#[tauri::command]
pub fn set_resource_ownership(
    paths: tauri::State<'_, StoragePaths>,
    owned: bool,
) -> Result<ResourceMarkerUpdate, String> {
    for (role, required) in [
        (ResourceRootRole::Image, paths.image_marker_required),
        (ResourceRootRole::File, paths.file_marker_required),
    ] {
        // Default project data directories never need a marker; skip them so
        // the switch cannot report a change it did not make.
        if !required || paths.resource_root_marker_valid(role) == owned {
            continue;
        }
        if owned {
            paths
                .claim_resource_root(role)
                .map_err(|error| error.to_string())?;
        } else {
            paths
                .remove_resource_root_marker(role)
                .map_err(|error| error.to_string())?;
        }
    }
    // Only flag a restart when the resulting ownership differs from the cleanup
    // flags the running workers captured at startup. Removing and re-adding a
    // marker in one session ends where it started, so it needs no restart.
    Ok(ResourceMarkerUpdate {
        restart_required: !paths.cleanup_flags_match_markers(),
    })
}

#[tauri::command]
pub fn get_storage_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<StorageConfigInfo, String> {
    let config = lock_state(&config, "configuration lock is poisoned")?;
    Ok(StorageConfigInfo {
        max_file_copy_size_bytes: config.max_file_copy_size_bytes(),
        max_screenshot_size_bytes: config.max_screenshot_size_bytes(),
        image_storage_path: config
            .image_storage_path()
            .map(|path| path.display().to_string()),
        file_storage_path: config
            .file_storage_path()
            .map(|path| path.display().to_string()),
    })
}

#[tauri::command]
pub fn set_storage_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    capture: tauri::State<'_, CaptureState>,
    max_file_copy_size_bytes: Option<u64>,
) -> Result<(), String> {
    let mut config = lock_state(&config, "configuration lock is poisoned")?;
    if let Some(v) = max_file_copy_size_bytes {
        config
            .set_max_file_copy_size_bytes(v)
            .map_err(|e| e.to_string())?;
        capture.set_max_file_copy_size_bytes(v);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Submodules
// ---------------------------------------------------------------------------

mod migration;
mod paths;
#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Re-exports so helpers remain at `crate::commands::config::storage::*`
// ---------------------------------------------------------------------------

pub use migration::*;
pub use paths::*;
