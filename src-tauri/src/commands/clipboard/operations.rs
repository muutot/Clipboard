use std::sync::Mutex;
use std::time::Instant;

use tauri::Emitter;

use crate::config::ConfigStore;
use crate::content;
use crate::domain::{ClipboardItem, ClipboardKind, OcrResult};
use crate::performance::PerformanceTracker;
use crate::search::{SearchIndex, SearchSyncSummary, SearchSynchronizer};
use crate::storage::{
    ClipboardRepository, Database, KindStorageStats, OcrRepository, SearchRepository, StoragePaths,
    TextItemUpdate,
};
use crate::CaptureState;

use super::types::{
    permanently_delete_storage_kind_for, ClipboardHistoryInvalidated, SearchResultCache,
    SearchSortDirection, SearchSortField, SearchSortRule, StorageKindDeleteExpectation,
    StorageKindDeleteResult,
};

#[tauri::command]
pub fn list_clipboard_items(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ClipboardItem>, String> {
    let max_limit = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .page_size_limit();
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(100).clamp(1, max_limit);

    let limit = if offset >= max_limit {
        0
    } else {
        (max_limit - offset).min(limit)
    };

    database
        .list_recent(limit, offset)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_clipboard_item_favorite(
    database: tauri::State<'_, Database>,
    id: String,
    is_favorite: bool,
) -> Result<bool, String> {
    database
        .set_favorite(&id, is_favorite)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn batch_set_favorite(
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
    is_favorite: bool,
) -> Result<bool, String> {
    database
        .set_favorite_batch(&ids, is_favorite)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_clipboard_item(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database.delete_item(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn batch_delete_clipboard_items(
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    database
        .soft_delete_batch(&ids)
        .map_err(|error| error.to_string())
}

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

#[tauri::command]
pub fn list_source_applications(
    database: tauri::State<'_, Database>,
) -> Result<Vec<String>, String> {
    database
        .list_source_applications()
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn search_clipboard_items(
    database: tauri::State<'_, Database>,
    search_index: tauri::State<'_, SearchIndex>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    performance_tracker: tauri::State<'_, PerformanceTracker>,
    search_cache: tauri::State<'_, SearchResultCache>,
    query: String,
    limit: Option<usize>,
    offset: Option<usize>,
    sort_rules: Option<Vec<SearchSortRule>>,
) -> Result<Vec<ClipboardItem>, String> {
    let started = Instant::now();
    let page_size = limit.unwrap_or(100).clamp(1, 500);
    let page_offset = offset.unwrap_or(0);

    let rules = sort_rules.unwrap_or_else(|| {
        vec![SearchSortRule {
            field: SearchSortField::CreatedAt,
            direction: SearchSortDirection::Desc,
        }]
    });

    let max_results = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .search_page_size_limit() as usize;

    // Drain pending search-outbox events so newly captured or mutated items
    // are reflected in Tantivy before querying. The outbox is populated by
    // SQLite triggers on every mutation (capture, OCR, delete, favorite,
    // restore, import) but is only applied to the index here, so clear the
    // result cache when the index actually changed to avoid serving stale
    // pages; leave it untouched when no mutations occurred. The cheap
    // `has_pending_outbox_events` probe avoids the `LIMIT` scan and reader
    // reload that `sync_until_idle` would trigger on every search.
    let pending = database.has_pending_outbox_events().unwrap_or(true);
    if pending {
        match SearchSynchronizer::default().sync_until_idle(database.inner(), search_index.inner()) {
            Ok(summary) if summary.processed_events > 0 => search_cache.clear(),
            Ok(_) => {}
            Err(error) => eprintln!("[search] outbox sync before search failed: {error}"),
        }
    }

    if let Some(cached) = search_cache.get(&query, &rules, max_results, page_offset, page_size) {
        performance_tracker.record_search(
            &query,
            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            cached.len(),
        );
        return Ok(cached);
    }

    let (_all_ids, _total) = search_index
        .search_all_ids(&query, max_results)
        .map_err(|error| error.to_string())?;

    let items = database
        .get_items_by_ids(&_all_ids)
        .map_err(|error| error.to_string())?;

    let mut sorted = items;
    apply_sort_rules(&mut sorted, &rules);

    // Cache takes ownership of the full sorted vector; slice the requested
    // page from it before moving to avoid a full extra clone.
    let total = sorted.len();
    let page_end = (page_offset + page_size).min(total);
    let page_start = page_offset.min(total);
    let result: Vec<ClipboardItem> = if page_start < page_end {
        sorted[page_start..page_end].to_vec()
    } else {
        Vec::new()
    };

    search_cache.set(query.clone(), rules.clone(), max_results, sorted);

    performance_tracker.record_search(
        &query,
        started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        result.len(),
    );
    Ok(result)
}

#[tauri::command]
pub fn rebuild_search_index(
    database: tauri::State<'_, Database>,
    search_index: tauri::State<'_, SearchIndex>,
    search_cache: tauri::State<'_, SearchResultCache>,
) -> Result<SearchSyncSummary, String> {
    search_cache.clear();
    SearchSynchronizer::default()
        .rebuild(database.inner(), search_index.inner())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn soft_delete_clipboard_item(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database.soft_delete(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn clear_all_non_favorite_items(database: tauri::State<'_, Database>) -> Result<u64, String> {
    database
        .clear_all_non_favorite_items()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn restore_clipboard_item(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database
        .restore_deleted(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_deleted_clipboard_items(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ClipboardItem>, String> {
    let max_limit = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .page_size_limit();
    database
        .list_deleted(
            limit.unwrap_or(100).clamp(1, max_limit),
            offset.unwrap_or(0),
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn batch_restore_clipboard_items(
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    database
        .restore_deleted_batch(&ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn permanently_delete_clipboard_item(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    database
        .permanently_delete(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn batch_permanently_delete_clipboard_items(
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    database
        .permanently_delete_batch(&ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn permanently_delete_storage_kind(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    search_index: tauri::State<'_, SearchIndex>,
    capture: tauri::State<'_, CaptureState>,
    app: tauri::AppHandle,
    kind: ClipboardKind,
    expected: StorageKindDeleteExpectation,
) -> Result<StorageKindDeleteResult, String> {
    let ingestion_guard = capture
        .ingestion_guard
        .lock()
        .map_err(|_| "clipboard ingestion lock is poisoned".to_owned())?;
    let expected = KindStorageStats {
        item_count: expected.item_count,
        size_bytes: expected.size_bytes,
    };
    let mut result = permanently_delete_storage_kind_for(
        database.inner(),
        paths.inner(),
        search_index.inner(),
        kind,
        Some(expected),
    )?;
    drop(ingestion_guard);
    if result.deleted_count > 0 {
        if let Err(error) = app.emit(
            "clipboard-history-invalidated",
            ClipboardHistoryInvalidated {
                deleted_ids: result.deleted_ids.clone(),
            },
        ) {
            result
                .warnings
                .push(format!("main window refresh is pending: {error}"));
        }
    }
    Ok(result)
}

/// Create a metadata-only duplicate of a clipboard item.
///
/// The duplicate copies **metadata only** — `resource_path` and
/// `preview_path` still point at the original item's underlying file (image,
/// file copy, etc.). Deleting either the original or the duplicate will
/// remove that shared file from disk and break the survivor's preview.
///
/// Users who want to keep both copies should un-favorite / soft-delete the
/// original instead of duplicating it, or duplicate then remove the original
/// via the trash (soft delete) rather than permanent delete.
#[tauri::command]
pub fn duplicate_clipboard_item(
    database: tauri::State<'_, Database>,
    app: tauri::AppHandle,
    id: String,
) -> Result<String, String> {
    let items = database
        .get_items_by_ids(std::slice::from_ref(&id))
        .map_err(|e| e.to_string())?;
    let mut item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    item.id = format!("{}-{}", item.content_hash, now_ms);
    item.content_hash = format!("{}-{}", item.content_hash, now_ms);
    item.created_at_ms = now_ms;
    item.last_used_at_ms = None;
    item.is_favorite = false;
    database.save_item(&item).map_err(|e| e.to_string())?;
    let _ = app.emit("clipboard-item-added", &item);
    Ok(item.id.clone())
}

#[tauri::command]
pub fn rename_item(
    database: tauri::State<'_, Database>,
    id: String,
    new_name: String,
) -> Result<ClipboardItem, String> {
    let items = database
        .get_items_by_ids(std::slice::from_ref(&id))
        .map_err(|e| e.to_string())?;
    let item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    if new_name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let mut updated = item.clone();
    if item.kind == ClipboardKind::Image || item.kind == ClipboardKind::File {
        if let Some(ref old_path) = item.resource_path {
            let old = std::path::Path::new(old_path);
            if old.exists() {
                let ext = old.extension().unwrap_or_default().to_string_lossy();
                let parent = old.parent().unwrap_or(std::path::Path::new("."));
                let new_path = parent.join(format!("{}.{}", new_name.trim(), ext));
                if new_path != old {
                    if new_path.exists() {
                        return Err(format!("file already exists: {}", new_path.display()));
                    }
                    std::fs::rename(old, &new_path).map_err(|e| format!("rename failed: {e}"))?;
                }
                updated.resource_path = Some(new_path.to_string_lossy().to_string());
                updated.preview_path = Some(new_path.to_string_lossy().to_string());
            }
        }
    }
    updated.title = new_name.trim().to_string();
    if matches!(updated.kind, ClipboardKind::Text | ClipboardKind::Link) {
        updated.metadata_json = Some(set_custom_title_metadata(
            updated.metadata_json.as_deref(),
            true,
        )?);
    }
    database.save_item(&updated).map_err(|e| e.to_string())?;
    Ok(updated)
}

#[tauri::command]
pub fn update_clipboard_text(
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

    database
        .update_text_item(&TextItemUpdate {
            id: &id,
            kind: item.kind,
            title: &new_title,
            text_content: &new_text_content,
            content_hash: &content_hash,
            size_bytes,
            metadata_json: Some(&metadata_json),
        })
        .map_err(|e| e.to_string())
}

pub fn cmp_by_field(
    a: &ClipboardItem,
    b: &ClipboardItem,
    field: SearchSortField,
) -> std::cmp::Ordering {
    match field {
        SearchSortField::CreatedAt => b.created_at_ms.cmp(&a.created_at_ms),
        SearchSortField::LastUsedAt => b.last_used_at_ms.cmp(&a.last_used_at_ms),
        SearchSortField::Title => a.title.cmp(&b.title),
        SearchSortField::Size => b.size_bytes.cmp(&a.size_bytes),
        SearchSortField::Kind => a.kind.cmp(&b.kind),
        SearchSortField::Favorite => b.is_favorite.cmp(&a.is_favorite),
    }
}

pub fn apply_sort_rules(items: &mut [ClipboardItem], rules: &[SearchSortRule]) {
    if rules.is_empty() {
        return;
    }
    items.sort_unstable_by(|a, b| {
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
