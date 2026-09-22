use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::Emitter;

use crate::commands::lock::lock_state;
use crate::config::{ConfigStore, SearchIndexSyncMode};
use crate::content;
use crate::domain::{ClipboardItem, ClipboardKind, OcrResult};
use crate::performance::PerformanceTracker;
use crate::search::{SearchIndex, SearchSyncSummary, SearchSynchronizer};
use crate::storage::{
    ClipboardRepository, Database, HistoryFilter, KindStorageStats, OcrRepository,
    SearchRepository, StoragePaths, TagInfo, TextItemUpdate,
};
use crate::CaptureState;

use super::types::{
    permanently_delete_storage_kind_for, ClipboardHistoryInvalidated, HistoryFilterArgs,
    SearchPage, SearchResultCache, SearchSortDirection, SearchSortField, SearchSortRule,
    StorageKindDeleteExpectation, StorageKindDeleteResult,
};

#[tauri::command]
pub fn list_clipboard_items(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    limit: Option<u32>,
    offset: Option<u32>,
    filter: Option<HistoryFilterArgs>,
) -> Result<Vec<ClipboardItem>, String> {
    let max_limit = lock_state(&config, "configuration lock is poisoned")?.page_size_limit();
    let offset = offset.unwrap_or(0);
    // `max_limit` caps the per-page size only; the row offset is independent.
    // Coupling them truncated deep pages (e.g. offset 600 with a 500 cap).
    let limit = limit.unwrap_or(100).clamp(1, max_limit);

    let filter = filter.map_or_else(HistoryFilter::default, |args| HistoryFilter {
        kind: args.kind,
        favorite_only: args.favorite.unwrap_or(false),
        tag: args.tag,
        source_app: args.source_app,
        date_from_ms: args.date_from_ms,
        date_to_ms: args.date_to_ms,
    });

    database
        .list_recent(limit, offset, &filter)
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
pub fn set_clipboard_item_tags(
    database: tauri::State<'_, Database>,
    id: String,
    tags: Vec<String>,
) -> Result<bool, String> {
    database
        .set_tags(&id, &tags)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_clipboard_item_last_used(
    database: tauri::State<'_, Database>,
    search_cache: tauri::State<'_, SearchResultCache>,
    id: String,
) -> Result<bool, String> {
    record_item_usage(&database, &search_cache, &id)
}

/// Stamps usage and invalidates the search result cache in one step.
/// `set_last_used` writes no `search_outbox` event (usage is device-local and
/// unwatched by the sync triggers), so neither the lazy drain nor the
/// background worker ever clears `SearchResultCache` for it; a cached
/// `lastUsedAt` page would keep serving the pre-usage order.
pub fn record_item_usage(
    database: &Database,
    search_cache: &SearchResultCache,
    id: &str,
) -> Result<bool, String> {
    let updated = database.set_last_used(id).map_err(|e| e.to_string())?;
    if updated {
        search_cache.clear();
    }
    Ok(updated)
}

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
    search_index: tauri::State<'_, Arc<SearchIndex>>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    performance_tracker: tauri::State<'_, PerformanceTracker>,
    search_cache: tauri::State<'_, SearchResultCache>,
    query: String,
    limit: Option<usize>,
    offset: Option<usize>,
    sort_rules: Option<Vec<SearchSortRule>>,
) -> Result<SearchPage, String> {
    let started = Instant::now();

    let (max_results, sync_mode) = {
        let config = lock_state(&config, "configuration lock is poisoned")?;
        (
            config.search_page_size_limit() as usize,
            config.search_index_sync_mode(),
        )
    };

    let page_size = limit.unwrap_or(100).clamp(1, max_results);
    let page_offset = offset.unwrap_or(0);

    let rules = sort_rules.unwrap_or_else(|| {
        vec![SearchSortRule {
            field: SearchSortField::CreatedAt,
            direction: SearchSortDirection::Desc,
        }]
    });

    // Drain pending search-outbox events so newly captured or mutated items
    // are reflected in Tantivy before querying. The outbox is populated by
    // SQLite triggers on every mutation (capture, OCR, delete, favorite,
    // restore, import) but is only applied to the index here, so clear the
    // result cache when the index actually changed to avoid serving stale
    // pages; leave it untouched when no mutations occurred. The cheap
    // `has_pending_outbox_events` probe avoids the `LIMIT` scan and reader
    // reload that `sync_until_idle` would trigger on every search.
    // When the user opts into background sync (`SearchIndexSyncMode::Background`)
    // the `SearchSyncWorker` drains the outbox off the hot path, so search
    // never blocks on indexing here.
    if sync_mode == SearchIndexSyncMode::Lazy {
        let pending = database.has_pending_outbox_events().unwrap_or(true);
        if pending {
            match SearchSynchronizer::default()
                .sync_until_idle(database.inner(), search_index.inner())
            {
                Ok(summary) if summary.processed_events > 0 => search_cache.clear(),
                Ok(_) => {}
                Err(error) => {
                    crate::log_event!("[search] outbox sync before search failed: {error}")
                }
            }
        }
    }

    if let Some((cached, total_count, truncated)) =
        search_cache.get(&query, &rules, max_results, page_offset, page_size)
    {
        performance_tracker.record_search(
            &query,
            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            cached.len(),
        );
        return Ok(SearchPage {
            items: cached,
            total_count,
            truncated,
        });
    }

    let (all_ids, total_count) = search_index
        .search_all_ids(&query, max_results)
        .map_err(|error| error.to_string())?;
    // The candidate list is capped at `max_results` while the total counts
    // every index match, so later pages know matches were dropped instead of
    // mistaking the cap for the end of the result set.
    let truncated = total_count > all_ids.len();

    let items = database
        .get_items_by_ids(&all_ids)
        .map_err(|error| error.to_string())?;

    let mut sorted = items;
    apply_sort_rules(&mut sorted, &rules);

    // Cache takes ownership of the full sorted vector; slice the requested
    // page from it before moving to avoid a full extra clone.
    let reachable = sorted.len();
    let page_end = (page_offset + page_size).min(reachable);
    let page_start = page_offset.min(reachable);
    let result: Vec<ClipboardItem> = if page_start < page_end {
        sorted[page_start..page_end].to_vec()
    } else {
        Vec::new()
    };

    search_cache.set(
        query.clone(),
        rules.clone(),
        max_results,
        sorted,
        total_count,
        truncated,
    );

    performance_tracker.record_search(
        &query,
        started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        result.len(),
    );
    Ok(SearchPage {
        items: result,
        total_count,
        truncated,
    })
}

#[tauri::command]
pub fn rebuild_search_index(
    database: tauri::State<'_, Database>,
    search_index: tauri::State<'_, Arc<SearchIndex>>,
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
    let max_limit = lock_state(&config, "configuration lock is poisoned")?.page_size_limit();
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
    search_index: tauri::State<'_, Arc<SearchIndex>>,
    capture: tauri::State<'_, CaptureState>,
    app: tauri::AppHandle,
    kind: ClipboardKind,
    expected: StorageKindDeleteExpectation,
) -> Result<StorageKindDeleteResult, String> {
    let ingestion_guard = lock_state(
        &capture.ingestion_guard,
        "clipboard ingestion lock is poisoned",
    )?;
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
        crate::log_event!("[clipboard] failed to emit duplicate: {error}");
    }
    Ok(item.id)
}

pub fn duplicate_clipboard_item_record(
    database: &Database,
    id: &str,
) -> Result<ClipboardItem, String> {
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
    item.last_used_at_ms = None;
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
        crate::log_event!("[clipboard] failed to emit save-as-new: {error}");
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
    item.last_used_at_ms = None;
    item.is_favorite = false;
    item.metadata_json = Some(metadata_json);
    database.save_item(&item).map_err(|e| e.to_string())?;
    Ok(item)
}

#[tauri::command]
pub fn rename_item(
    database: tauri::State<'_, Database>,
    id: String,
    new_name: String,
) -> Result<ClipboardItem, String> {
    rename_item_record(&database, id, new_name)
}

fn rename_item_record(
    database: &Database,
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
    let sanitized_name = sanitize_file_stem(new_name.trim());
    if sanitized_name.is_empty() {
        return Err("name contains no usable characters".to_string());
    }
    let mut updated = item.clone();
    if item.kind == ClipboardKind::Image || item.kind == ClipboardKind::File {
        if let Some(ref old_path) = item.resource_path {
            let old = std::path::Path::new(old_path);
            // Storage files are content-hash-named and can be shared by several
            // records (dedup, duplicates, previews). Only rename the physical
            // file when this record is its sole owner; otherwise the rename
            // would break the other records sharing it. In that case fall back
            // to renaming the display title only.
            let shared = database
                .resource_reference_count(old_path, &id)
                .map_err(|e| e.to_string())?
                > 0;
            if old.exists() && !shared {
                let ext = old.extension().unwrap_or_default().to_string_lossy();
                let parent = old.parent().unwrap_or(std::path::Path::new("."));
                // The new name arrives from the webview and must never be able
                // to escape the managed directory via separators or traversal.
                // An empty extension must not produce a trailing dot: Windows
                // strips it from the real file name, so the DB record would
                // point at a path that does not exist on disk.
                let candidate = if ext.is_empty() {
                    sanitized_name.clone()
                } else {
                    format!("{sanitized_name}.{ext}")
                };
                if std::path::Path::new(&candidate)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .as_deref()
                    != Some(candidate.as_str())
                {
                    return Err("invalid file name".to_string());
                }
                let new_path = parent.join(candidate);
                if new_path != old {
                    if new_path.exists() {
                        return Err(format!("file already exists: {}", new_path.display()));
                    }
                    // Persist the new paths FIRST, then move the file. A
                    // failed database write leaves the disk untouched; a
                    // failed rename is rolled back in the database so the
                    // record never points at a missing file.
                    let old_path_string = old.to_string_lossy().to_string();
                    let new_path_string = new_path.to_string_lossy().to_string();
                    let rollback = updated.clone();
                    updated.resource_path = Some(new_path_string.clone());
                    // preview_path either points at the dedicated thumbnail
                    // under previews/ or falls back to the original image
                    // path until the worker fills it in. Only refresh the
                    // fallback; overwriting a generated thumbnail link would
                    // lose it and orphan the preview file.
                    if updated.preview_path.as_deref() == Some(old_path_string.as_str()) {
                        updated.preview_path = Some(new_path_string.clone());
                    }
                    // The detail panel and multi-file paste read the managed
                    // path out of `metadata_json`/`text_content` before
                    // `resource_path`, so those copies must move with the file.
                    rewrite_stored_resource_paths(&mut updated, &old_path_string, &new_path_string);
                    database.save_item(&updated).map_err(|e| e.to_string())?;
                    if let Err(e) = std::fs::rename(old, &new_path) {
                        database.save_item(&rollback).map_err(|rollback_error| {
                            format!(
                                "rename failed ({e}) and the database rollback also failed ({rollback_error})"
                            )
                        })?;
                        return Err(format!("rename failed: {e}"));
                    }
                }
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

/// Restricts a webview-provided file name stem to characters that cannot
/// alter the destination directory (`/`, `\`, `:`) or form a Windows device
/// name, and strips trailing dots/spaces (illegal on Windows). Returns an
/// empty string when nothing usable remains.
fn sanitize_file_stem(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if (character as u32) < 0x20 || character == '\u{7f}' => '_',
            other => other,
        })
        .collect();
    let trimmed = sanitized.trim_end_matches(['.', ' ']);
    if is_windows_reserved_device_name(trimmed) {
        return "_".to_owned();
    }
    trimmed.to_owned()
}

fn is_windows_reserved_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    ["COM", "LPT"].iter().any(|prefix| {
        upper.starts_with(prefix)
            && !upper[prefix.len()..].is_empty()
            && upper[prefix.len()..]
                .chars()
                .all(|character| character.is_ascii_digit())
    })
}

/// Rewrites every exact occurrence of `old_path` inside a renamed record's
/// `metadata_json` and file-list `text_content`. `rename_item` updates
/// `resource_path`/`preview_path`, but the detail panel and multi-file paste
/// read the managed path out of those fields first, so a rename that skipped
/// them would point the UI at a file that no longer exists.
fn rewrite_stored_resource_paths(item: &mut ClipboardItem, old_path: &str, new_path: &str) {
    if let Some(metadata) = item.metadata_json.as_deref() {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(metadata) {
            replace_path_strings(&mut value, old_path, new_path);
            if let Ok(serialized) = serde_json::to_string(&value) {
                item.metadata_json = Some(serialized);
            }
        }
    }
    if let Some(text) = item.text_content.as_deref() {
        if let Ok(mut paths) = serde_json::from_str::<Vec<String>>(text) {
            let mut changed = false;
            for path in &mut paths {
                if path == old_path {
                    *path = new_path.to_owned();
                    changed = true;
                }
            }
            if changed {
                if let Ok(serialized) = serde_json::to_string(&paths) {
                    item.text_content = Some(serialized);
                }
            }
        }
    }
}

/// Recursively replaces string values equal to `old_path`. Exact matches keep
/// unrelated paths (e.g. the other files of a multi-file capture) untouched.
fn replace_path_strings(value: &mut serde_json::Value, old_path: &str, new_path: &str) {
    match value {
        serde_json::Value::String(text) => {
            if text == old_path {
                *text = new_path.to_owned();
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                replace_path_strings(item, old_path, new_path);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                replace_path_strings(item, old_path, new_path);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_sort_rules, cmp_by_field, duplicate_clipboard_item_record, generated_clipboard_title,
        metadata_custom_title, record_item_usage, rename_item_record, resolve_custom_title,
        rewrite_stored_resource_paths, sanitize_file_stem, save_clipboard_item_as_new_record,
        set_custom_title_metadata,
    };
    use crate::commands::clipboard::types::{
        SearchResultCache, SearchSortDirection, SearchSortField, SearchSortRule,
    };
    use crate::domain::{ClipboardItem, ClipboardKind};
    use crate::storage::ClipboardRepository;
    use crate::storage::Database;

    fn item(id: &str, title: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: title.to_owned(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: None,
            icon_path: None,
            size_bytes: 0,
            created_at_ms: 0,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    #[test]
    fn duplicate_namespaces_hash_and_stays_unique_when_called_twice() {
        let database = Database::open_in_memory().unwrap();
        let mut source = item("src", "title");
        source.content_hash = "abc".to_owned();
        source.text_content = Some("hello".to_owned());
        database.save_item(&source).unwrap();

        let first = duplicate_clipboard_item_record(&database, "src").unwrap();
        let second = duplicate_clipboard_item_record(&database, "src").unwrap();

        assert_ne!(first.id, second.id);
        assert_ne!(first.content_hash, second.content_hash);
        assert_ne!(first.content_hash, "abc");
        assert_eq!(
            database.get_item("src").unwrap().unwrap().content_hash,
            "abc"
        );
        assert!(database.get_item(&first.id).unwrap().is_some());
        assert!(database.get_item(&second.id).unwrap().is_some());
    }

    #[test]
    fn save_as_new_persists_unchanged_content_without_hash_collision() {
        let database = Database::open_in_memory().unwrap();
        let mut source = item("src", "title");
        source.content_hash = "real-hash".to_owned();
        source.text_content = Some("same".to_owned());
        database.save_item(&source).unwrap();

        let created =
            save_clipboard_item_as_new_record(&database, "src", "title2", "same").unwrap();

        assert_ne!(created.id, "src");
        assert_eq!(created.text_content.as_deref(), Some("same"));
        assert_ne!(created.content_hash, "real-hash");
        assert_eq!(
            database.get_item("src").unwrap().unwrap().content_hash,
            "real-hash"
        );
        assert!(database.get_item(&created.id).unwrap().is_some());
    }

    #[test]
    fn save_as_new_uses_real_hash_for_fresh_content() {
        let database = Database::open_in_memory().unwrap();
        let mut source = item("src", "title");
        source.content_hash = "real-hash".to_owned();
        source.text_content = Some("old".to_owned());
        database.save_item(&source).unwrap();

        let created =
            save_clipboard_item_as_new_record(&database, "src", "title", "brand new").unwrap();
        let expected = crate::content::hash::compute_content_hash("text", "brand new", None);
        assert_eq!(created.content_hash, expected);
    }

    #[test]
    fn save_as_new_rejects_empty_text_and_media_kinds() {
        let database = Database::open_in_memory().unwrap();
        let mut source = item("src", "title");
        source.text_content = Some("body".to_owned());
        database.save_item(&source).unwrap();
        assert!(save_clipboard_item_as_new_record(&database, "src", "t", "   ").is_err());

        let mut media = item("img", "shot");
        media.kind = ClipboardKind::Image;
        database.save_item(&media).unwrap();
        assert!(save_clipboard_item_as_new_record(&database, "img", "t", "text").is_err());
    }

    #[test]
    fn replaces_path_separators_and_windows_specials() {
        // Separators become underscores, so the name can never traverse out
        // of the managed directory; leading dots are harmless (same dir).
        assert_eq!(sanitize_file_stem("..\\..\\evil"), ".._.._evil");
        assert_eq!(
            sanitize_file_stem("a/b:c*d?e\"f<g>h|i"),
            "a_b_c_d_e_f_g_h_i"
        );
    }

    #[test]
    fn traversal_only_names_collapse_to_empty() {
        assert_eq!(sanitize_file_stem(".."), "");
        assert_eq!(sanitize_file_stem("."), "");
        assert_eq!(sanitize_file_stem("..."), "");
    }

    #[test]
    fn keeps_unicode_display_names() {
        assert_eq!(sanitize_file_stem("截图 2026"), "截图 2026");
        assert_eq!(sanitize_file_stem("report-v2_final"), "report-v2_final");
    }

    #[test]
    fn rejects_windows_reserved_device_names() {
        assert_eq!(sanitize_file_stem("CON"), "_");
        assert_eq!(sanitize_file_stem("com1"), "_");
        assert_eq!(sanitize_file_stem("LPT4"), "_");
        assert_eq!(sanitize_file_stem("combo"), "combo");
    }

    #[test]
    fn rename_rewrites_managed_paths_in_metadata_and_file_list() {
        let mut record = item("file-1", "old");
        record.kind = ClipboardKind::File;
        record.resource_path = Some("/store/files/old.txt".to_owned());
        record.preview_path = Some("/store/files/old.txt".to_owned());
        record.metadata_json = Some(
            r#"{"resourcePath":"/store/files/old.txt","files":[{"storagePath":"/store/files/old.txt"},{"storagePath":"/store/files/other.txt"}]}"#
                .to_owned(),
        );
        record.text_content =
            Some(r#"["/store/files/old.txt","/store/files/other.txt"]"#.to_owned());

        rewrite_stored_resource_paths(&mut record, "/store/files/old.txt", "/store/files/new.txt");

        let metadata: serde_json::Value =
            serde_json::from_str(record.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(metadata["resourcePath"], "/store/files/new.txt");
        assert_eq!(metadata["files"][0]["storagePath"], "/store/files/new.txt");
        assert_eq!(
            metadata["files"][1]["storagePath"],
            "/store/files/other.txt"
        );
        assert_eq!(
            record.text_content.as_deref(),
            Some(r#"["/store/files/new.txt","/store/files/other.txt"]"#)
        );
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize_file_stem("name. "), "name");
    }

    #[test]
    fn rename_keeps_generated_thumbnail_preview_path() {
        let project = std::env::temp_dir().join(format!(
            "clipboard-rename-preview-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let paths = crate::storage::StoragePaths::initialize(project.clone()).unwrap();
        let database = Database::open(&paths.database).unwrap();

        let images_dir = project.join("images");
        std::fs::create_dir_all(&images_dir).unwrap();
        let old_path = images_dir.join("old.png");
        std::fs::write(&old_path, b"png").unwrap();

        // A generated thumbnail (preview_path != resource_path) must survive
        // the rename: overwriting it would drop the preview link and orphan
        // the file under previews/.
        let mut record = item("img-1", "old");
        record.kind = ClipboardKind::Image;
        record.resource_path = Some(old_path.display().to_string());
        record.preview_path = Some("/store/previews/thumb.jpg".to_owned());
        database.save_item(&record).unwrap();

        let renamed = rename_item_record(&database, "img-1".to_owned(), "new".to_owned()).unwrap();
        let new_path = images_dir.join("new.png");
        assert_eq!(
            renamed.resource_path.as_deref(),
            Some(new_path.to_str().unwrap())
        );
        assert_eq!(
            renamed.preview_path.as_deref(),
            Some("/store/previews/thumb.jpg")
        );
        assert!(new_path.exists());
        assert!(!old_path.exists());

        // A fallback preview (thumbnail not yet generated) follows the file.
        let mut fallback = item("img-2", "old2");
        fallback.kind = ClipboardKind::Image;
        let old2 = images_dir.join("old2.png");
        std::fs::write(&old2, b"png").unwrap();
        fallback.resource_path = Some(old2.display().to_string());
        fallback.preview_path = fallback.resource_path.clone();
        database.save_item(&fallback).unwrap();
        let renamed2 =
            rename_item_record(&database, "img-2".to_owned(), "new2".to_owned()).unwrap();
        let new2 = images_dir.join("new2.png");
        assert_eq!(
            renamed2.preview_path.as_deref(),
            Some(new2.to_str().unwrap())
        );

        drop(database);
        std::fs::remove_dir_all(project).unwrap();
    }

    fn rule(field: SearchSortField, direction: SearchSortDirection) -> SearchSortRule {
        SearchSortRule { field, direction }
    }

    #[test]
    fn cmp_by_field_orders_descending_by_default_for_recency_fields() {
        let mut older = item("a", "a");
        older.created_at_ms = 100;
        let mut newer = item("b", "b");
        newer.created_at_ms = 200;

        assert_eq!(
            cmp_by_field(&newer, &older, SearchSortField::CreatedAt),
            std::cmp::Ordering::Less
        );

        // LastUsedAt falls back to created_at_ms for never-used entries so a
        // fresh copy still outranks used entries.
        newer.last_used_at_ms = Some(50);
        older.last_used_at_ms = None;
        older.created_at_ms = 300;
        assert_eq!(
            cmp_by_field(&older, &newer, SearchSortField::LastUsedAt),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            cmp_by_field(&newer, &older, SearchSortField::LastUsedAt),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn cmp_by_field_prefers_newer_capture_over_older_use() {
        let mut recaptured = item("a", "a");
        recaptured.created_at_ms = 300;
        recaptured.last_used_at_ms = Some(150);
        let mut other = item("b", "b");
        other.created_at_ms = 200;
        other.last_used_at_ms = None;

        assert_eq!(
            cmp_by_field(&recaptured, &other, SearchSortField::LastUsedAt),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            cmp_by_field(&other, &recaptured, SearchSortField::LastUsedAt),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn cmp_by_field_covers_title_size_kind_and_favorite() {
        let mut small = item("s", "beta");
        small.size_bytes = 10;
        let mut big = item("b", "alpha");
        big.size_bytes = 500;
        big.is_favorite = true;
        big.kind = ClipboardKind::Image;

        assert_eq!(
            cmp_by_field(&small, &big, SearchSortField::Title),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            cmp_by_field(&big, &small, SearchSortField::Size),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            cmp_by_field(&big, &small, SearchSortField::Kind),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            cmp_by_field(&big, &small, SearchSortField::Favorite),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn title_asc_sorts_lexicographically_after_the_direction_fix() {
        let alpha = item("a", "Alpha");
        let beta = item("b", "Beta");

        // Asc must be A→Z now that every field shares a descending base.
        let mut items = vec![beta.clone(), alpha.clone()];
        apply_sort_rules(
            &mut items,
            &[rule(SearchSortField::Title, SearchSortDirection::Asc)],
        );
        assert_eq!(
            items
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );

        // Desc is Z→A.
        let mut reversed = vec![alpha, beta];
        apply_sort_rules(
            &mut reversed,
            &[rule(SearchSortField::Title, SearchSortDirection::Desc)],
        );
        assert_eq!(
            reversed
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
    }

    #[test]
    fn apply_sort_rules_sorts_multi_key_with_direction() {
        let mut first = item("1", "b");
        first.created_at_ms = 100;
        first.is_favorite = false;
        let mut second = item("2", "a");
        second.created_at_ms = 100;
        second.is_favorite = true;
        let mut third = item("3", "c");
        third.created_at_ms = 300;

        let mut items = vec![first, second, third];
        apply_sort_rules(
            &mut items,
            &[rule(SearchSortField::CreatedAt, SearchSortDirection::Asc)],
        );
        // Ascending recency = oldest entries first.
        assert_eq!(
            items
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["1", "2", "3"]
        );

        // Favorite desc wins over the title asc tiebreak for items 1 and 2.
        apply_sort_rules(
            &mut items,
            &[
                rule(SearchSortField::Favorite, SearchSortDirection::Desc),
                rule(SearchSortField::Title, SearchSortDirection::Asc),
            ],
        );
        assert_eq!(
            items
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["2", "1", "3"]
        );
    }

    #[test]
    fn apply_sort_rules_keeps_order_without_rules() {
        let mut items = vec![item("x", "x"), item("a", "a")];
        apply_sort_rules(&mut items, &[]);
        assert_eq!(items[0].id, "x");
    }

    /// Module scenario for the usage-stamp staleness: the default history
    /// sort is `lastUsedAt desc`, a usage stamp writes no `search_outbox`
    /// event, and `SearchResultCache` therefore outlives the re-ranking the
    /// stamp should cause. After pasting an entry out of history, the next
    /// cached page must no longer serve the pre-usage order.
    #[test]
    fn usage_stamp_invalidates_the_cached_last_used_page() {
        let database = Database::open_in_memory().unwrap();
        let mut older = item("older", "record-older");
        older.text_content = Some("content-older".to_owned());
        older.created_at_ms = 100;
        let mut newer = item("newer", "record-newer");
        newer.text_content = Some("content-newer".to_owned());
        newer.created_at_ms = 200;
        database.save_item(&older).unwrap();
        database.save_item(&newer).unwrap();

        // Simulate the search pipeline for the default sort: fetch, sort,
        // cache the full sorted result.
        let rules = vec![SearchSortRule {
            field: SearchSortField::LastUsedAt,
            direction: SearchSortDirection::Desc,
        }];
        let ids = vec!["older".to_owned(), "newer".to_owned()];
        let mut sorted = database.get_items_by_ids(&ids).unwrap();
        apply_sort_rules(&mut sorted, &rules);
        assert_eq!(
            sorted
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["newer", "older"]
        );

        let cache = SearchResultCache::new();
        cache.set(String::new(), rules.clone(), 100, sorted, 2, false);
        assert_eq!(cache.get("", &rules, 100, 0, 10).unwrap().0[0].id, "newer");

        // The user pastes the older entry out of history; the usage entry
        // point must drop the cached page since no outbox event fires.
        let updated = record_item_usage(&database, &cache, "older").unwrap();
        assert!(updated);

        // The cached page must be gone now; while it survives, scrolling
        // serves the stale pre-usage order even though the entry has been
        // re-ranked in the database.
        assert!(cache.get("", &rules, 100, 0, 10).is_none());

        // A cache-miss re-query re-sorts with the used entry on top.
        let mut resorted = database.get_items_by_ids(&ids).unwrap();
        apply_sort_rules(&mut resorted, &rules);
        assert_eq!(
            resorted
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["older", "newer"]
        );
    }

    /// Module scenario for tie ordering: the search pipeline feeds items in
    /// Tantivy relevance order and the reference strategy documents that
    /// order as the fallback for equal sort keys. With `sort_unstable_by`
    /// the tie groups of a larger input are permuted, so the documented
    /// fallback only holds deterministically under a stable sort.
    #[test]
    fn apply_sort_rules_keeps_incoming_order_for_tied_keys() {
        // 160 items in 40 ascending size groups of four. Sorting by size
        // desc must fully reverse the group order while every tie group
        // keeps its incoming (relevance) sequence.
        let mut items: Vec<ClipboardItem> = (0..160)
            .map(|index| {
                let mut entry = item(&format!("id-{index:03}"), "record");
                entry.size_bytes = (index / 4) as u64;
                entry
            })
            .collect();
        apply_sort_rules(
            &mut items,
            &[rule(SearchSortField::Size, SearchSortDirection::Desc)],
        );

        let expected: Vec<String> = (0..40)
            .rev()
            .flat_map(|group| (0..4).map(move |offset| format!("id-{:03}", group * 4 + offset)))
            .collect();
        assert_eq!(
            items
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn generated_clipboard_title_truncates_by_chars_not_bytes() {
        let ascii = "k".repeat(500);
        assert_eq!(generated_clipboard_title(&ascii).len(), 200);

        // 300 CJK characters (900 bytes): byte slicing would panic or split a
        // code point; char-based truncation yields exactly 200 chars.
        let cjk: String = "剪".repeat(300);
        let title = generated_clipboard_title(&cjk);
        assert_eq!(title.chars().count(), 200);
    }

    #[test]
    fn metadata_custom_title_reads_only_boolean_flags() {
        assert_eq!(metadata_custom_title(None), None);
        assert_eq!(metadata_custom_title(Some("not-json")), None);
        assert_eq!(
            metadata_custom_title(Some(r#"{"customTitle":true}"#)),
            Some(true)
        );
        assert_eq!(
            metadata_custom_title(Some(r#"{"customTitle":false}"#)),
            Some(false)
        );
        // Non-boolean values must not be coerced into a decision.
        assert_eq!(
            metadata_custom_title(Some(r#"{"customTitle":"yes"}"#)),
            None
        );
    }

    #[test]
    fn resolve_custom_title_prefers_metadata_then_compares_titles() {
        assert!(resolve_custom_title(
            "My title",
            "body",
            Some(r#"{"customTitle":true}"#)
        ));
        assert!(!resolve_custom_title(
            "My title",
            "body",
            Some(r#"{"customTitle":false}"#)
        ));

        // Without metadata the generated title decides.
        let body = "hello";
        assert!(!resolve_custom_title(body, body, None));
        assert!(resolve_custom_title("edited", body, None));
    }

    #[test]
    fn set_custom_title_metadata_preserves_existing_fields() {
        let updated = set_custom_title_metadata(Some(r#"{"tags":["work"],"n":1}"#), true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(value["customTitle"], serde_json::Value::Bool(true));
        assert_eq!(value["tags"], serde_json::json!(["work"]));
        assert_eq!(value["n"], serde_json::json!(1));

        // A non-object payload is replaced rather than corrupted.
        let replaced = set_custom_title_metadata(Some("[1,2]"), false).unwrap();
        let replaced_value: serde_json::Value = serde_json::from_str(&replaced).unwrap();
        assert_eq!(
            replaced_value["customTitle"],
            serde_json::Value::Bool(false)
        );

        assert!(set_custom_title_metadata(None, true).is_ok());
    }
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
    // Every field uses a descending base comparison so `apply_sort_rules`
    // can treat Asc uniformly as "reverse of the natural desc order". The
    // previous mixed convention (Title/Kind ascending bases) made a user's
    // "title A→Z" (Asc) selection sort Z→A.
    match field {
        SearchSortField::CreatedAt => b.created_at_ms.cmp(&a.created_at_ms),
        // Items never used from history have `last_used_at_ms = NULL`. Fall back
        // to `created_at_ms` so a freshly copied (but unused) entry still sorts
        // to the top instead of sinking below used entries. A re-copied entry
        // stamps `last_used_at_ms` while its `created_at_ms` stays frozen, so
        // the effective recency is the greater of the two; the max (rather
        // than a plain fallback) also keeps imported/synced rows whose stored
        // `last_used_at_ms` predates their capture time ordered sanely.
        SearchSortField::LastUsedAt => {
            let a_recency = a
                .last_used_at_ms
                .unwrap_or(a.created_at_ms)
                .max(a.created_at_ms);
            let b_recency = b
                .last_used_at_ms
                .unwrap_or(b.created_at_ms)
                .max(b.created_at_ms);
            b_recency.cmp(&a_recency)
        }
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
