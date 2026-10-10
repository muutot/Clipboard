//! Themed text editor commands plus the sort/title helpers they and the
//! history views share.

use super::*;

#[tauri::command]
pub fn update_clipboard_text(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    id: String,
    new_title: String,
    new_text_content: String,
) -> Result<bool, String> {
    if new_text_content.trim().is_empty() {
        return Err("text content cannot be empty".to_owned());
    }

    let items = database
        .get_items_by_ids(std::slice::from_ref(&id))
        .map_err(|e| e.to_string())?;
    let item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    if !matches!(item.kind, ClipboardKind::Text | ClipboardKind::Link) {
        return Err("only text and link items can be edited".to_owned());
    }

    let kind_name = match item.kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image | ClipboardKind::File => unreachable!(),
    };
    let custom_title =
        resolve_custom_title(&new_title, &new_text_content, item.metadata_json.as_deref());
    let metadata_json = set_custom_title_metadata(item.metadata_json.as_deref(), custom_title)?;
    let content_hash = content::hash::compute_content_hash(kind_name, &new_text_content, None);
    let size_bytes = new_text_content.len() as u64;

    let updated = database
        .update_text_item(&TextItemUpdate {
            id: &id,
            kind: item.kind,
            title: &new_title,
            text_content: &new_text_content,
            content_hash: &content_hash,
            size_bytes,
            metadata_json: Some(&metadata_json),
        })
        .map_err(|e| e.to_string())?;
    if updated {
        // The edited title and body are what the other windows render.
        broadcast_content_changed(&app, &database, std::slice::from_ref(&id));
    }
    Ok(updated)
}

pub fn cmp_by_field(
    a: &ClipboardItem,
    b: &ClipboardItem,
    field: SearchSortField,
) -> std::cmp::Ordering {
    // Every field uses a descending base comparison so `apply_sort_rules`
    // can treat Asc uniformly as "reverse of the natural desc order". The
    // previous mixed convention (Title/Kind ascending bases) made a user's
    // "title A→Z" (Asc) selection sort Z→A.
    match field {
        SearchSortField::CreatedAt => b.created_at_ms.cmp(&a.created_at_ms),
        // Persistence initializes missing usage times; sorting uses only the
        // stored value, even when an imported usage time predates creation.
        SearchSortField::LastUsedAt => b.last_used_at_ms.cmp(&a.last_used_at_ms),
        SearchSortField::Title => b.title.cmp(&a.title),
        SearchSortField::Size => b.size_bytes.cmp(&a.size_bytes),
        SearchSortField::Kind => b.kind.cmp(&a.kind),
        SearchSortField::Favorite => b.is_favorite.cmp(&a.is_favorite),
    }
}

pub fn apply_sort_rules(items: &mut [ClipboardItem], rules: &[SearchSortRule]) {
    if rules.is_empty() {
        return;
    }
    // Stable on purpose: when every rule key ties, the incoming order —
    // the Tantivy relevance order the search pipeline feeds in — must
    // survive as the fallback. `sort_unstable_by` permutes tied elements
    // for larger inputs, which scrambles pages and breaks that contract.
    items.sort_by(|a, b| {
        let mut ord = std::cmp::Ordering::Equal;
        for rule in rules {
            ord = cmp_by_field(a, b, rule.field);
            if rule.direction == SearchSortDirection::Asc {
                ord = ord.reverse();
            }
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        ord
    });
}

pub fn generated_clipboard_title(text: &str) -> String {
    if text.is_ascii() {
        let end = text.len().min(200);
        text[..end].to_owned()
    } else {
        text.chars().take(200).collect()
    }
}

pub fn metadata_custom_title(metadata_json: Option<&str>) -> Option<bool> {
    let value =
        metadata_json.and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())?;
    value
        .get("customTitle")
        .and_then(serde_json::Value::as_bool)
}

pub fn resolve_custom_title(title: &str, text_content: &str, metadata_json: Option<&str>) -> bool {
    metadata_custom_title(metadata_json)
        .unwrap_or_else(|| title != generated_clipboard_title(text_content))
}

pub fn set_custom_title_metadata(
    metadata_json: Option<&str>,
    custom_title: bool,
) -> Result<String, String> {
    let mut value = metadata_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if !value.is_object() {
        value = serde_json::json!({});
    }
    value
        .as_object_mut()
        .expect("custom-title metadata must be an object")
        .insert(
            "customTitle".to_owned(),
            serde_json::Value::Bool(custom_title),
        );
    serde_json::to_string(&value)
        .map_err(|error| format!("serialize custom title metadata: {error}"))
}
