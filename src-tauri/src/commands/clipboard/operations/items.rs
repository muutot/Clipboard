//! Item mutations that are not deletions: favorite/tags, copy, last-used
//! stamping and batch favorite.

use super::*;

#[tauri::command]
pub fn set_clipboard_item_favorite(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    id: String,
    is_favorite: bool,
) -> Result<bool, String> {
    let updated = database
        .set_favorite(&id, is_favorite)
        .map_err(|error| error.to_string())?;
    if updated {
        broadcast_content_changed(&app, &database, std::slice::from_ref(&id));
    }
    Ok(updated)
}

#[tauri::command]
pub fn set_clipboard_item_tags(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    id: String,
    tags: Vec<String>,
) -> Result<bool, String> {
    let updated = database
        .set_tags(&id, &tags)
        .map_err(|error| error.to_string())?;
    if updated {
        broadcast_content_changed(&app, &database, std::slice::from_ref(&id));
    }
    Ok(updated)
}

#[tauri::command]
pub fn copy_clipboard_item(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    self_trigger: tauri::State<'_, crate::state::SelfTriggerState>,
    search_cache: tauri::State<'_, SearchResultCache>,
    id: String,
) -> Result<bool, crate::commands::clipboard::types::ClipboardFilesCopyError> {
    let context = crate::item_operations::CopyContext {
        paths: Some(paths.inner().clone()),
        self_trigger: Some(self_trigger.0.clone()),
    };
    let (_, updated) = crate::item_operations::copy_item_with(&database, &id, |item| {
        context.write(&database, item)
    })?;
    if updated {
        search_cache.clear();
        broadcast_item_usage(&app, &database, [id]);
    }
    Ok(updated)
}

#[tauri::command]
pub fn set_clipboard_item_last_used(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    search_cache: tauri::State<'_, SearchResultCache>,
    id: String,
) -> Result<bool, String> {
    let updated = record_item_usage(&database, &search_cache, &id)?;
    if updated {
        broadcast_item_usage(&app, &database, std::iter::once(id.as_str()));
    }
    Ok(updated)
}

#[tauri::command]
pub fn batch_set_favorite(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
    is_favorite: bool,
) -> Result<bool, String> {
    let updated = database
        .set_favorite_batch(&ids, is_favorite)
        .map_err(|error| error.to_string())?;
    if updated {
        broadcast_content_changed(&app, &database, &ids);
    }
    Ok(updated)
}
