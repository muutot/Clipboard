use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::params;

use crate::config::ConfigStore;
use crate::domain::ClipboardKind;
use crate::keyboard::KeyboardManager;
use crate::platform;
use crate::search::{SearchIndex, SEARCH_INDEX_VERSION};
use crate::storage::{ClipboardRepository, Database, StorageError, StoragePaths};
use crate::{CaptureState, STORAGE_KIND_DELETE_SCOPE};

use super::{ResourceStorageUpdate, StorageConfigInfo, StorageDirectoryUpdate, StorageKindStats, StorageStatus};

#[tauri::command]
pub fn get_storage_status(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    keyboard: tauri::State<'_, Mutex<KeyboardManager>>,
    search_index: tauri::State<'_, SearchIndex>,
) -> Result<StorageStatus, String> {
    let config_path = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .path()
        .display()
        .to_string();
    let keyboard_config_path = keyboard
        .lock()
        .map_err(|_| "keyboard configuration lock is poisoned".to_owned())?
        .path()
        .display()
        .to_string();

    let disk_space = platform::disk_space(&paths.data_directory);

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

#[tauri::command]
pub fn configure_storage_directory(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    active_paths: tauri::State<'_, StoragePaths>,
    database: tauri::State<'_, Database>,
    capture: tauri::State<'_, CaptureState>,
    data_directory: Option<String>,
) -> Result<StorageDirectoryUpdate, String> {
    let requested_directory = data_directory.map(PathBuf::from);
    let (image_storage_path, file_storage_path) = {
        let config = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
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

    if target_paths.data_directory != active_paths.data_directory {
        capture.set_paused(true);
        migrate_storage_data(&active_paths, &target_paths, &database)?;
    }

    let saved_directory = target_paths
        .uses_custom_data_directory()
        .then(|| target_paths.data_directory.clone());

    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_storage_directory(saved_directory)
        .map_err(|error| error.to_string())?;

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

    config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .set_resource_storage_paths(image_storage_path, file_storage_path)
        .map_err(|error| error.to_string())?;

    Ok(ResourceStorageUpdate {
        image_storage_path: target_paths.images.display().to_string(),
        file_storage_path: target_paths.files.display().to_string(),
        restart_required: target_paths.images != active_paths.images
            || target_paths.files != active_paths.files,
    })
}

#[tauri::command]
pub fn get_storage_config(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<StorageConfigInfo, String> {
    let config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
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
    let mut config = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    if let Some(v) = max_file_copy_size_bytes {
        config
            .set_max_file_copy_size_bytes(v)
            .map_err(|e| e.to_string())?;
        capture.set_max_file_copy_size_bytes(v);
    }
    Ok(())
}

pub fn copy_dir_contents(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("create dir: {}", e))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("read dir: {}", e))? {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let dest = to.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|e| format!("read file type for {}: {e}", entry.path().display()))?
            .is_dir()
        {
            copy_dir_contents(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest).map_err(|e| {
                format!("copy {} to {}: {e}", entry.path().display(), dest.display())
            })?;
        }
    }
    Ok(())
}

pub fn file_or_dir_size(path: &PathBuf) -> u64 {
    if path.is_file() {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let wal = path.with_extension("sqlite3-wal");
        let shm = path.with_extension("sqlite3-shm");
        let wal_size = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        let shm_size = std::fs::metadata(&shm).map(|m| m.len()).unwrap_or(0);
        return size + wal_size + shm_size;
    }
    dir_size(path)
}

pub fn dir_size(path: &PathBuf) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                total += dir_size(&entry.path());
            } else {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

pub fn migrate_storage_data(
    old: &StoragePaths,
    new: &StoragePaths,
    database: &Database,
) -> Result<(), String> {
    let dirs_to_migrate: &[(PathBuf, PathBuf, &str)] = &[
        (old.images.clone(), new.images.clone(), "images"),
        (old.files.clone(), new.files.clone(), "files"),
    ];

    for (old_dir, new_dir, label) in dirs_to_migrate {
        if old_dir == new_dir {
            continue;
        }
        if old_dir.exists() {
            copy_dir_contents(old_dir, new_dir)
                .map_err(|e| format!("failed to migrate {}: {}", label, e))?;
        }
    }

    if old.search_index != new.search_index {
        std::fs::create_dir_all(&new.search_index)
            .map_err(|e| format!("failed to create search index directory: {}", e))?;
    }

    let icons_old = old.storage.join("icons");
    let icons_new = new.storage.join("icons");
    if icons_old != icons_new && icons_old.exists() {
        copy_dir_contents(&icons_old, &icons_new)
            .map_err(|e| format!("failed to migrate icons: {}", e))?;
    }

    if old.database != new.database && old.database.exists() {
        database
            .vacuum_into(&new.database)
            .map_err(|e| format!("failed to migrate database: {}", e))?;
        let migrated_database = Database::open(&new.database)
            .map_err(|e| format!("failed to open migrated database: {e}"))?;
        rewrite_database_storage_paths(&migrated_database, &storage_path_mappings(old, new))
            .map_err(|e| format!("failed to update migrated resource paths: {e}"))?;
    }

    Ok(())
}

pub fn storage_path_mappings(old: &StoragePaths, new: &StoragePaths) -> Vec<(PathBuf, PathBuf)> {
    let mut mappings = vec![
        (old.previews.clone(), new.previews.clone()),
        (old.images.clone(), new.images.clone()),
        (old.files.clone(), new.files.clone()),
        (old.storage.join("icons"), new.storage.join("icons")),
        (old.storage.clone(), new.storage.clone()),
    ];
    mappings.retain(|(from, to)| from != to);
    mappings.sort_by_key(|(from, _)| std::cmp::Reverse(from.components().count()));
    mappings
}

pub fn rewrite_database_storage_paths(
    database: &Database,
    mappings: &[(PathBuf, PathBuf)],
) -> Result<u64, StorageError> {
    database.with_connection(|connection| {
        let transaction = connection.transaction()?;
        let records = {
            let mut statement = transaction.prepare(
                "SELECT id, kind, text_content, resource_path, preview_path, icon_path, metadata_json
                 FROM clipboard_items",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut updated = 0u64;
        for (id, kind, text_content, resource_path, preview_path, icon_path, metadata_json) in
            records
        {
            let rewritten_resource = rewrite_optional_storage_path(resource_path.as_deref(), mappings);
            let rewritten_preview = rewrite_optional_storage_path(preview_path.as_deref(), mappings);
            let rewritten_icon = rewrite_optional_storage_path(icon_path.as_deref(), mappings);
            let rewritten_text = if kind == "file" {
                rewrite_json_storage_paths(text_content.as_deref(), mappings)
            } else {
                text_content.clone()
            };
            let rewritten_metadata = rewrite_json_storage_paths(metadata_json.as_deref(), mappings);

            if rewritten_resource == resource_path
                && rewritten_preview == preview_path
                && rewritten_icon == icon_path
                && rewritten_text == text_content
                && rewritten_metadata == metadata_json
            {
                continue;
            }

            transaction.execute(
                "UPDATE clipboard_items
                 SET text_content = ?2,
                     resource_path = ?3,
                     preview_path = ?4,
                     icon_path = ?5,
                     metadata_json = ?6
                 WHERE id = ?1",
                params![
                    id,
                    rewritten_text,
                    rewritten_resource,
                    rewritten_preview,
                    rewritten_icon,
                    rewritten_metadata,
                ],
            )?;
            updated = updated.saturating_add(1);
        }

        transaction.commit()?;
        Ok(updated)
    })
}

pub fn rewrite_optional_storage_path(
    value: Option<&str>,
    mappings: &[(PathBuf, PathBuf)],
) -> Option<String> {
    value.map(|value| rewrite_storage_path(value, mappings))
}

pub fn rewrite_storage_path(value: &str, mappings: &[(PathBuf, PathBuf)]) -> String {
    let path = Path::new(value);
    for (from, to) in mappings {
        if let Ok(relative) = path.strip_prefix(from) {
            return to.join(relative).to_string_lossy().into_owned();
        }
    }
    value.to_owned()
}

pub fn rewrite_json_storage_paths(
    value: Option<&str>,
    mappings: &[(PathBuf, PathBuf)],
) -> Option<String> {
    let value = value?;
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(value) else {
        return Some(value.to_owned());
    };
    let changed = rewrite_json_value_paths(&mut json, mappings);
    if changed {
        serde_json::to_string(&json)
            .ok()
            .or_else(|| Some(value.to_owned()))
    } else {
        Some(value.to_owned())
    }
}

pub fn rewrite_json_value_paths(
    value: &mut serde_json::Value,
    mappings: &[(PathBuf, PathBuf)],
) -> bool {
    match value {
        serde_json::Value::String(path) => {
            let rewritten = rewrite_storage_path(path, mappings);
            if rewritten == *path {
                false
            } else {
                *path = rewritten;
                true
            }
        }
        serde_json::Value::Array(values) => {
            let mut changed = false;
            for value in values {
                changed |= rewrite_json_value_paths(value, mappings);
            }
            changed
        }
        serde_json::Value::Object(values) => {
            let mut changed = false;
            for value in values.values_mut() {
                changed |= rewrite_json_value_paths(value, mappings);
            }
            changed
        }
        _ => false,
    }
}
