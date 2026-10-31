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
pub fn set_item_code_language(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    id: String,
    language: Option<String>,
) -> Result<ClipboardItem, String> {
    // The value flows into `metadata_json` and back into every window, so
    // constrain it to identifier-shaped language names ("C++", "C#", "Shell").
    let validated = language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.len() > 24
                || !value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_' | '.'))
            {
                return Err("invalid code language".to_string());
            }
            Ok(value.to_owned())
        })
        .transpose()?;
    let updated = database
        .set_code_language(&id, validated.as_deref())
        .map_err(|error| error.to_string())?;
    if updated {
        broadcast_content_changed(&app, &database, std::slice::from_ref(&id));
    }
    // The webview patches its store from the merged metadata, so hand back
    // the full record instead of a bare boolean.
    database
        .get_item(&id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "item not found".to_string())
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
