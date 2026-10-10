//! OCR result lookup and regeneration commands.

use super::*;

#[tauri::command]
pub fn get_clipboard_item_ocr(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<Option<OcrResult>, String> {
    database
        .get_ocr_result(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn regenerate_clipboard_item_ocr(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database
        .regenerate_ocr(&id)
        .map_err(|error| error.to_string())
}
