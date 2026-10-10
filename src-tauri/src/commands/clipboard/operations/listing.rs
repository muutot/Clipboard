//! Read and search commands: history listing, detail lookup, source apps,
//! search, index rebuild and the themed editor clipboard read.

use super::*;

#[tauri::command]
pub fn list_clipboard_items(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    limit: Option<u32>,
    offset: Option<u32>,
    filter: Option<HistoryFilterArgs>,
) -> Result<Vec<ClipboardListItem>, String> {
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
        cursor: args.cursor.map(HistoryCursor::from),
    });

    database
        .list_summaries(limit, offset, &filter, false)
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_clipboard_item(
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<Option<ClipboardItem>, String> {
    database.get_item(&id).map_err(|error| error.to_string())
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
    search_worker: tauri::State<'_, Mutex<Option<SearchSyncWorker>>>,
    query: String,
    limit: Option<usize>,
    offset: Option<usize>,
    sort_rules: Option<Vec<SearchSortRule>>,
    filter: Option<HistoryFilterArgs>,
) -> Result<SearchPage, String> {
    let started = Instant::now();
    let mut filter = filter.unwrap_or_default();
    filter.cursor = None;
    let cache_key = serde_json::to_string(&(&query, &filter)).map_err(|e| e.to_string())?;
    let filter = HistoryFilter {
        kind: filter.kind,
        favorite_only: filter.favorite.unwrap_or(false),
        tag: filter.tag,
        source_app: filter.source_app,
        date_from_ms: filter.date_from_ms,
        date_to_ms: filter.date_to_ms,
        cursor: None,
    };

    let max_results = {
        let config = lock_state(&config, "configuration lock is poisoned")?;
        config.search_page_size_limit() as usize
    };
    let background = lock_state(&search_worker, "search-sync lock is poisoned")?
        .as_ref()
        .is_some_and(SearchSyncWorker::is_running);

    let page_size = limit.unwrap_or(100).clamp(1, max_results);
    let page_offset = offset.unwrap_or(0);

    let rules = sort_rules.unwrap_or_else(|| {
        vec![SearchSortRule {
            field: SearchSortField::LastUsedAt,
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
    if !background {
        let pending = database.has_pending_outbox_events().unwrap_or(true);
        if pending {
            match SearchSynchronizer::default()
                .sync_until_idle(database.inner(), search_index.inner())
            {
                Ok(summary) if summary.processed_events > 0 => search_cache.clear(),
                Ok(_) => {}
                Err(error) => {
                    crate::log_error!("[search] outbox sync before search failed: {error}")
                }
            }
        }
    }

    if let Some((cached, total_count, truncated)) =
        search_cache.get(&cache_key, &rules, max_results, page_offset, page_size)
    {
        performance_tracker.record_search(
            &query,
            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            cached.len(),
        );
        return Ok(SearchPage {
            items: cached.into_iter().map(Into::into).collect(),
            total_count,
            truncated,
        });
    }

    let cache_token = search_cache.write_token();
    let allowed_ids = (filter != HistoryFilter::default())
        .then(|| database.search_filter_ids(&filter))
        .transpose()
        .map_err(|e| e.to_string())?;
    let (all_ids, total_count) = search_index
        .search_filtered_ids(&query, max_results, allowed_ids.as_deref())
        .map_err(|error| error.to_string())?;
    // The candidate list is capped at `max_results` while the total counts
    // every index match, so later pages know matches were dropped instead of
    // mistaking the cap for the end of the result set.
    let truncated = total_count > all_ids.len();

    let items = database
        .get_summary_items_by_ids(&all_ids)
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
        cache_token,
        cache_key,
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
        items: result.into_iter().map(Into::into).collect(),
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

/// Reads the current system clipboard text for the themed editor context menu,
/// so Paste does not depend on the WebView granting clipboard-read permission.
#[tauri::command]
pub fn read_clipboard_text() -> Option<String> {
    crate::platform::platform().read_clipboard_text()
}
