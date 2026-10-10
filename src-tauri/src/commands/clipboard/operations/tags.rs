//! Tag registry commands: list, rename, delete and color assignment.

use super::*;

#[tauri::command]
pub fn list_all_tags(database: tauri::State<'_, Database>) -> Result<Vec<TagInfo>, String> {
    database.list_all_tags().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn rename_tag(
    database: tauri::State<'_, Database>,
    old: String,
    new: String,
) -> Result<u64, String> {
    database
        .rename_tag(&old, &new)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_tag(database: tauri::State<'_, Database>, name: String) -> Result<u64, String> {
    database
        .delete_tag(&name)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_tag_color(
    database: tauri::State<'_, Database>,
    name: String,
    color: String,
) -> Result<bool, String> {
    database
        .set_tag_color(&name, &color)
        .map_err(|error| error.to_string())
}
