pub mod config;
pub mod domain;
pub mod ocr;
pub mod platform;
pub mod search;
pub mod storage;

use std::{path::PathBuf, sync::Mutex};

use config::ConfigStore;
use domain::{ClipboardItem, OcrResult};
use platform::RuntimeInfo;
use serde::Serialize;
use storage::{ClipboardRepository, Database, OcrRepository, StoragePaths};
use tauri::Manager;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageStatus {
    schema_version: i64,
    item_count: u64,
    project_path: String,
    config_path: String,
    data_directory_path: String,
    uses_custom_data_directory: bool,
    storage_path: String,
    database_path: String,
    files_path: String,
    image_path: String,
    search_index_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageDirectoryUpdate {
    data_directory_path: String,
    storage_path: String,
    restart_required: bool,
}

#[tauri::command]
fn get_runtime_info() -> RuntimeInfo {
    platform::runtime_info()
}

#[tauri::command]
fn get_storage_status(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<StorageStatus, String> {
    let config_path = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .path()
        .display()
        .to_string();

    Ok(StorageStatus {
        schema_version: database
            .schema_version()
            .map_err(|error| error.to_string())?,
        item_count: database.item_count().map_err(|error| error.to_string())?,
        project_path: paths.project.display().to_string(),
        config_path,
        data_directory_path: paths.data_directory.display().to_string(),
        uses_custom_data_directory: paths.uses_custom_data_directory(),
        storage_path: paths.storage.display().to_string(),
        database_path: paths.database.display().to_string(),
        files_path: paths.files.display().to_string(),
        image_path: paths.images.display().to_string(),
        search_index_path: paths.search_index.display().to_string(),
    })
}

#[tauri::command]
fn configure_storage_directory(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    active_paths: tauri::State<'_, StoragePaths>,
    data_directory: Option<String>,
) -> Result<StorageDirectoryUpdate, String> {
    let requested_directory = data_directory.map(PathBuf::from);
    let target_paths = StoragePaths::initialize_with_data_directory(
        active_paths.project.clone(),
        requested_directory,
    )
    .map_err(|error| error.to_string())?;
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
fn list_clipboard_items(
    database: tauri::State<'_, Database>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ClipboardItem>, String> {
    database
        .list_recent(limit.unwrap_or(100), offset.unwrap_or(0))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_clipboard_item_favorite(
    database: tauri::State<'_, Database>,
    id: String,
    is_favorite: bool,
) -> Result<bool, String> {
    database
        .set_favorite(&id, is_favorite)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_clipboard_item(database: tauri::State<'_, Database>, id: String) -> Result<bool, String> {
    database.delete_item(&id).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_clipboard_item_ocr(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<Option<OcrResult>, String> {
    database
        .get_ocr_result(&id)
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let project_directory = app.path().app_data_dir()?;
            let config = ConfigStore::load(&project_directory)?;
            let paths = StoragePaths::initialize_with_data_directory(
                project_directory,
                config.storage_directory().map(PathBuf::from),
            )?;
            let database = Database::open(&paths.database)?;
            database.requeue_interrupted_ocr()?;

            app.manage(Mutex::new(config));
            app.manage(paths);
            app.manage(database);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_info,
            get_storage_status,
            configure_storage_directory,
            list_clipboard_items,
            set_clipboard_item_favorite,
            delete_clipboard_item,
            get_clipboard_item_ocr
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
