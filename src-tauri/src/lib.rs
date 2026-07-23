pub mod domain;
pub mod ocr;
pub mod platform;
pub mod search;
pub mod storage;

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
    storage_path: String,
    database_path: String,
    files_path: String,
    image_path: String,
    search_index_path: String,
}

#[tauri::command]
fn get_runtime_info() -> RuntimeInfo {
    platform::runtime_info()
}

#[tauri::command]
fn get_storage_status(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<StorageStatus, String> {
    Ok(StorageStatus {
        schema_version: database
            .schema_version()
            .map_err(|error| error.to_string())?,
        item_count: database.item_count().map_err(|error| error.to_string())?,
        project_path: paths.project.display().to_string(),
        storage_path: paths.storage.display().to_string(),
        database_path: paths.database.display().to_string(),
        files_path: paths.files.display().to_string(),
        image_path: paths.images.display().to_string(),
        search_index_path: paths.search_index.display().to_string(),
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
            let paths = StoragePaths::initialize(app.path().app_data_dir()?)?;
            let database = Database::open(&paths.database)?;
            database.requeue_interrupted_ocr()?;

            app.manage(paths);
            app.manage(database);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_info,
            get_storage_status,
            list_clipboard_items,
            set_clipboard_item_favorite,
            delete_clipboard_item,
            get_clipboard_item_ocr
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
