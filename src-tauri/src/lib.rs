pub mod domain;
pub mod ocr;
pub mod platform;
pub mod search;
pub mod storage;

use platform::RuntimeInfo;
use serde::Serialize;
use storage::{ClipboardRepository, Database, OcrRepository, StoragePaths};
use tauri::Manager;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageStatus {
    schema_version: i64,
    item_count: u64,
    database_path: String,
    files_path: String,
    screenshots_path: String,
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
        database_path: paths.database.display().to_string(),
        files_path: paths.files.display().to_string(),
        screenshots_path: paths.screenshots.display().to_string(),
        search_index_path: paths.search_index.display().to_string(),
    })
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
            get_storage_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
