pub mod domain;
pub mod ocr;
pub mod platform;
pub mod search;
pub mod storage;

use platform::RuntimeInfo;

#[tauri::command]
fn get_runtime_info() -> RuntimeInfo {
    platform::runtime_info()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_runtime_info])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
