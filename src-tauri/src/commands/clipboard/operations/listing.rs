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
pub async fn search_clipboard_items(
    app: tauri::AppHandle,
    query: String,
    limit: Option<usize>,
    offset: Option<usize>,
    sort_rules: Option<Vec<SearchSortRule>>,
    filter: Option<HistoryFilterArgs>,
) -> Result<SearchPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        let search_index = app.state::<Arc<SearchIndex>>();
        let config = app.state::<Mutex<ConfigStore>>();
        let performance_tracker = app.state::<PerformanceTracker>();
        let search_cache = app.state::<SearchResultCache>();
        let search_worker = app.state::<Mutex<Option<SearchSyncWorker>>>();
        search_clipboard_items_blocking(
            database.inner(),
            search_index.inner().as_ref(),
            config.inner(),
            performance_tracker.inner(),
            search_cache.inner(),
            search_worker.inner(),
            query,
            limit,
            offset,
            sort_rules,
            filter,
        )
    })
    .await
    .map_err(|error| format!("search task join failed: {error}"))?
}

#[allow(clippy::too_many_arguments)]
pub(super) fn search_clipboard_items_blocking(
    database: &Database,
    search_index: &SearchIndex,
    config: &Mutex<ConfigStore>,
    performance_tracker: &PerformanceTracker,
    search_cache: &SearchResultCache,
    search_worker: &Mutex<Option<SearchSyncWorker>>,
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
            match SearchSynchronizer::default().sync_until_idle(database, search_index) {
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
pub async fn rebuild_search_index(app: tauri::AppHandle) -> Result<SearchSyncSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        let search_index = app.state::<Arc<SearchIndex>>();
        let search_cache = app.state::<SearchResultCache>();
        rebuild_search_index_blocking(
            database.inner(),
            search_index.inner().as_ref(),
            search_cache.inner(),
            || {
                if let Err(error) = app.emit("search-index-changed", ()) {
                    crate::log_warn!("[search] unable to announce index changes: {error}");
                }
            },
        )
    })
    .await
    .map_err(|error| format!("search rebuild task join failed: {error}"))?
}

fn rebuild_search_index_blocking(
    database: &Database,
    search_index: &SearchIndex,
    search_cache: &SearchResultCache,
    on_index_changed: impl FnOnce(),
) -> Result<SearchSyncSummary, String> {
    search_cache.clear();
    let result = SearchSynchronizer::default()
        .rebuild(database, search_index)
        .map_err(|error| error.to_string());
    // Queries can observe an empty or partial reader during a rebuild. Fence
    // their cached pages and delayed writes after it ends, including errors
    // after Tantivy was already changed, then refresh displayed queries.
    search_cache.clear();
    on_index_changed();
    result
}

/// Reads the current system clipboard text for the themed editor context menu,
/// so Paste does not depend on the WebView granting clipboard-read permission.
#[tauri::command]
pub fn read_clipboard_text() -> Option<String> {
    crate::platform::platform().read_clipboard_text()
}

/// Writes text through the platform adapter, which attaches the native
/// self-trigger marker. WebView2 rejects `navigator.clipboard.writeText` while
/// the window is hidden, and bulk paste writes every item after the previous
/// delivery already hid the window.
#[tauri::command]
pub fn write_clipboard_text(text: String) -> Result<(), String> {
    let result = crate::platform::platform().write_clipboard_text_with_self_trigger(&text);
    // Bulk paste stages every item here before the delivery call, so a wrong
    // or missing payload at the target means this write, not the injection.
    // The leading snippet identifies which record actually landed on the
    // clipboard without dumping whole histories into the log.
    let snippet: String = text.chars().take(48).collect();
    crate::dbg_log(&format!(
        "write_clipboard_text: len={} result={:?} head={:?}",
        text.chars().count(),
        result.as_ref().map(|_| "ok"),
        snippet,
    ));
    result
}

#[cfg(test)]
mod rebuild_tests {
    use super::*;
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn manual_rebuild_invalidates_pages_and_tokens_created_during_rebuild() {
        let root = std::env::temp_dir().join(format!("clipboard-rebuild-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("history.sqlite3");
        let database = Database::open(&path).unwrap();
        let reader = Database::open(&path).unwrap();
        let index = SearchIndex::in_memory().unwrap();
        let cache = SearchResultCache::new();
        let config = Mutex::new(ConfigStore::load(&root).unwrap());
        let tracker = PerformanceTracker::new();
        let worker = Mutex::new(None);
        let record = ClipboardItem {
            id: "needle".into(),
            kind: ClipboardKind::Text,
            title: "needle".into(),
            text_content: Some("needle body".into()),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "synthetic-needle".into(),
            source_app: None,
            icon_path: None,
            size_bytes: 11,
            created_at_ms: 0,
            last_used_at_ms: Some(100),
            is_favorite: false,
            metadata_json: None,
        };
        database.save_item(&record).unwrap();
        SearchSynchronizer::default()
            .sync_until_idle(&database, &index)
            .unwrap();
        assert!(!database.has_pending_outbox_events().unwrap());

        let (entered_tx, entered_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        database
            .with_connection(|connection| {
                connection.authorizer(Some(move |context: AuthContext<'_>| {
                    if matches!(
                        context.action,
                        AuthAction::Insert {
                            table_name: "search_outbox"
                        }
                    ) {
                        // Tantivy is already empty, but the rebuild's outbox is not
                        // committed yet. A separate WAL reader controls this window
                        // without a production timing hook or a shared DB-lock wait.
                        entered_tx.send(()).unwrap();
                        resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    }
                    Authorization::Allow
                }))?;
                Ok(())
            })
            .unwrap();

        let search = || {
            search_clipboard_items_blocking(
                &reader,
                &index,
                &config,
                &tracker,
                &cache,
                &worker,
                "needle".into(),
                None,
                None,
                None,
                None,
            )
            .unwrap()
        };
        std::thread::scope(|scope| {
            let rebuild = scope.spawn(|| {
                rebuild_search_index_blocking(&database, &index, &cache, || {
                    let key =
                        serde_json::to_string(&("needle", HistoryFilterArgs::default())).unwrap();
                    let rules = vec![SearchSortRule {
                        field: SearchSortField::LastUsedAt,
                        direction: SearchSortDirection::Desc,
                    }];
                    let maximum = config.lock().unwrap().search_page_size_limit() as usize;
                    assert!(cache.get(&key, &rules, maximum, 0, 100).is_none());
                    assert_eq!(index.search_filtered_ids("needle", 100, None).unwrap().1, 1);
                })
            });
            entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            assert!(!reader.has_pending_outbox_events().unwrap());
            let token = cache.write_token();
            let intermediate = search();
            resume_tx.send(()).unwrap();
            let summary = rebuild.join().unwrap().unwrap();
            assert_eq!(summary.upserted_documents, 1);
            assert!(intermediate.items.is_empty());
            assert_eq!(index.search_filtered_ids("needle", 100, None).unwrap().1, 1);
            assert!(!reader.has_pending_outbox_events().unwrap());

            // A delayed query must not put its intermediate page back after
            // the completed rebuild invalidated it.
            let restored = search();
            cache.set(token, "delayed".into(), vec![], 100, vec![], 0, false);
            let delayed_rejected = cache.get("delayed", &[], 100, 0, 100).is_none();
            assert_eq!((restored.total_count, delayed_rejected), (1, true));
            assert_eq!(restored.items[0].item.id, "needle");
        });
        drop(reader);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_rebuild_announces_an_empty_index_after_invalidating_its_cache() {
        let database = Database::open_in_memory().unwrap();
        let index = SearchIndex::in_memory().unwrap();
        let cache = SearchResultCache::new();
        let notified = std::cell::Cell::new(false);
        let summary = rebuild_search_index_blocking(&database, &index, &cache, || {
            notified.set(true);
            assert!(!index.requires_full_rebuild());
        })
        .unwrap();
        assert_eq!(summary.processed_events, 0);
        assert!(notified.get());
    }

    #[test]
    fn manual_rebuild_failure_invalidates_intermediate_pages_before_announcing() {
        let database = Database::open_in_memory().unwrap();
        let index = SearchIndex::in_memory().unwrap();
        let cache = Arc::new(SearchResultCache::new());
        let during_rebuild = cache.clone();
        database
            .with_connection(|connection| {
                connection.authorizer(Some(move |context: AuthContext<'_>| {
                    if matches!(
                        context.action,
                        AuthAction::Insert {
                            table_name: "search_outbox"
                        }
                    ) {
                        during_rebuild.set(
                            during_rebuild.write_token(),
                            "partial".into(),
                            vec![],
                            100,
                            vec![],
                            0,
                            false,
                        );
                        return Authorization::Deny;
                    }
                    Authorization::Allow
                }))?;
                Ok(())
            })
            .unwrap();
        let notified = std::cell::Cell::new(false);
        let result = rebuild_search_index_blocking(&database, &index, &cache, || {
            notified.set(true);
            assert!(cache.get("partial", &[], 100, 0, 100).is_none());
            assert!(index.requires_full_rebuild());
        });
        assert!(result.is_err());
        assert!(notified.get());
    }
}
