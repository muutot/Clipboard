//! Metadata-only duplication and save-as-new commands.

use super::*;

/// Create a metadata-only duplicate of a clipboard item.
///
/// The duplicate copies **metadata only** — `resource_path` and
/// `preview_path` still point at the original item's underlying file (image,
/// file copy, etc.). The shared file is safe: disk reclamation happens only
/// through reference-scanned orphan cleanup, so the file survives until every
/// row pointing at it is gone, and `rename_item` renames the physical file
/// only when this record is its sole owner.
///
/// `id` and `content_hash` are namespaced with a random UUID suffix so
/// `UNIQUE (kind, content_hash)` admits a second row for the same content and
/// two rapid duplicates never collapse into one upsert.
#[tauri::command]
pub fn duplicate_clipboard_item(
    database: tauri::State<'_, Database>,
    app: tauri::AppHandle,
    id: String,
) -> Result<String, String> {
    let item = duplicate_clipboard_item_record(database.inner(), &id)?;
    if let Err(error) = app.emit("clipboard-item-added", &item) {
        crate::log_error!("[clipboard] failed to emit duplicate: {error}");
    }
    Ok(item.id)
}

pub fn duplicate_clipboard_item_record(
    database: &Database,
    id: &str,
) -> Result<ClipboardItem, String> {
    let _resource_publication = database.begin_resource_write();
    let items = database
        .get_items_by_ids(&[id.to_owned()])
        .map_err(|e| e.to_string())?;
    let mut item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let unique = uuid::Uuid::new_v4().simple();
    let namespaced = format!("{}-{unique}", item.content_hash);
    item.id = namespaced.clone();
    item.content_hash = namespaced;
    item.created_at_ms = now_ms;
    item.last_used_at_ms = Some(now_ms);
    item.is_favorite = false;
    database.save_item(&item).map_err(|e| e.to_string())?;
    Ok(item)
}

/// Insert a new text/link record from an edited copy of `id` in one step.
///
/// Replaces the UI's former `duplicate_clipboard_item` + `update_clipboard_text`
/// pair: a failure can no longer leave a half-created duplicate, and a content
/// hash that already exists (including unchanged text saved beside its source)
/// is UUID-namespaced instead of tripping `UNIQUE (kind, content_hash)`.
#[tauri::command]
pub fn save_clipboard_item_as_new(
    database: tauri::State<'_, Database>,
    app: tauri::AppHandle,
    id: String,
    new_title: String,
    new_text_content: String,
) -> Result<String, String> {
    let item =
        save_clipboard_item_as_new_record(database.inner(), &id, &new_title, &new_text_content)?;
    if let Err(error) = app.emit("clipboard-item-added", &item) {
        crate::log_error!("[clipboard] failed to emit save-as-new: {error}");
    }
    Ok(item.id)
}

pub fn save_clipboard_item_as_new_record(
    database: &Database,
    id: &str,
    new_title: &str,
    new_text_content: &str,
) -> Result<ClipboardItem, String> {
    if new_text_content.trim().is_empty() {
        return Err("text content cannot be empty".to_owned());
    }
    let items = database
        .get_items_by_ids(&[id.to_owned()])
        .map_err(|e| e.to_string())?;
    let source = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    if !matches!(source.kind, ClipboardKind::Text | ClipboardKind::Link) {
        return Err("only text and link items can be edited".to_owned());
    }
    let kind_name = match source.kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image | ClipboardKind::File => unreachable!(),
    };
    let custom_title =
        resolve_custom_title(new_title, new_text_content, source.metadata_json.as_deref());
    let metadata_json = set_custom_title_metadata(source.metadata_json.as_deref(), custom_title)?;
    let proposed_hash = content::hash::compute_content_hash(kind_name, new_text_content, None);
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let content_hash = if database
        .content_exists(source.kind, &proposed_hash)
        .map_err(|e| e.to_string())?
    {
        format!("{proposed_hash}-{unique}")
    } else {
        proposed_hash
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let mut item = source;
    item.id = format!("{content_hash}-{unique}");
    item.title = new_title.to_owned();
    item.text_content = Some(new_text_content.to_owned());
    item.html_content = None;
    item.rtf_content = None;
    item.content_hash = content_hash;
    item.size_bytes = new_text_content.len() as u64;
    item.created_at_ms = now_ms;
    item.last_used_at_ms = Some(now_ms);
    item.is_favorite = false;
    item.metadata_json = Some(metadata_json);
    database.save_item(&item).map_err(|e| e.to_string())?;
    Ok(item)
}
