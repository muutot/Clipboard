# Search Pagination and Cache Strategy

Search currently has three distinct pieces of state. Do not collapse them conceptually:

1. backend Tantivy ID cache in `src-tauri/src/search/index.rs`;
2. active paginated search results, the `indexed` id view of the item store;
3. frontend spare-result cache, the `cache` id view of the same store.

## Frontend item store

`src/lib/utils/item-store.ts` is the single source of truth for every loaded
record. `ItemStore` holds `byId: ReadonlyMap<string, ClipboardItem>` plus four
id-only views — `historyIds`, `indexedIds` (`null` = no search displayed),
`cacheIds`, and the single `detailId` — so two views cannot disagree about a
record's content: there is only one record to read. `+page.svelte` drives
`createItemStoreView()` from `item-store-view.svelte.ts`, which owns the only
reactive declaration (`$state.raw` plus `$derived` projections exposed through
getters) and keeps the plain `Map` out of Svelte's deep proxy. See
`docs/PITFALLS.md` for why the runes live in that module and not inline.

Invariants every mutator preserves, asserted after each operation in
`item-store.test.ts`:

- `byId` holds exactly the union of the four views; a view never displays an id
  the map lost, and a record that leaves every view is released from the map.
- One id may appear in several views at once (that overlap is the point), but
  never twice inside one view.
- Every mutator is pure and returns the next store, so the route reassigns
  `itemStore.current` and no write can be applied to one view only.

The rune contract itself (projections recompute on a store replacement, records
stay plain objects) is covered by `item-store-view.test.ts`, which mounts
`item-store-view.probe.svelte` with `mount()`. A plain `.test.ts` has no
reaction context, so `$derived` values created there are unowned and go stale —
rune behaviour cannot be asserted without a component.

## Cross-window convergence

Each WebviewWindow holds its own item store; the float panel cannot share the
main route's instance (separate JS realms). Two mechanisms keep them in step:

- `clipboard-items-changed` (see `data-contracts.md`) carries what a mutator
  changed. Both routes fold it through `utils/item-changes.ts`, so a favorite
  toggled in the panel lands in the main window and vice versa.
- The float panel derives visible rows from deletion/favorite state and reloads
  on focus, `clipboard-item-added`, membership/content changes in
  `clipboard-items-changed`, and `clipboard-history-invalidated` (import/sync/
  cleanup). Each reload invalidates the previous request, so stale responses
  cannot resurrect rows removed by a newer event. Usage-only events promote
  loaded rows without reloading. Component tests in `routes/float/float-page.test.ts`
  cover deletion, favorite filtering, restoration, and full invalidation.

Anything that adds a new item-level mutator must broadcast, or the other window
silently keeps the old row: a command that takes no `AppHandle` physically
cannot emit, which is how this drifted for so long.

## Backend Tantivy ID cache

`SearchIndex` stores `cached_ids: Mutex<Option<(String, usize, Option<(i64, i64)>, Vec<String>, usize)>>`.

- Key: normalized query plus requested `max_results` plus the resolved date range.
- Hit: query matches, cached maximum is at least the new maximum, and the resolved date range matches; return only the requested prefix plus the stored total match count. The `Count` collector records every match (not just the capped `TopDocs`), so the total stays truthful even when the id list was truncated at `max_results`; a first capped call already reports the full total. The date range is part of the key because relative phrases ("今天", "本周", "本月") resolve against `Local::now()`; without it a cache entry created before midnight would serve yesterday's range after midnight.
- Miss: query differs or requested maximum grows; run Tantivy again and replace the cache.
- Empty query stores/returns an empty ID set.
- `apply_changes()` clears cached IDs after index mutations and reloads the reader, so a subsequent search reflects the commit without callers having to reload explicitly.
- `begin_full_rebuild()` clears cached IDs before rebuild.
- `clear_cached_ids()` bumps an internal generation counter. A concurrent `search_all_ids` captures the generation before it reads and only repopulates the cache when the generation is still unchanged, so a write cannot be followed by a stale id list; both `apply_changes()` and `begin_full_rebuild()` clear again after the reader reload to drop a snapshot stored during the commit window.

## Backend SearchResultCache

`SearchResultCache` in `src-tauri/src/commands/clipboard/types.rs` stores fully-sorted, fetched `ClipboardItem` results plus the true match total and the truncation flag, keyed by `(query, sort_rules, max_results, local_date_bucket)`. The local calendar-day bucket invalidates the cache at the next local midnight so relative-date queries cannot serve a stale range.

- Hit: slice `[offset..offset+limit]` directly from the cached vector and return it with the stored total/truncation; no DB or index access needed.
- Miss: re-run the full search pipeline (Tantivy → SQL fetch → sort) and cache the result.
- Usage stamps clear this cache explicitly: `record_item_usage` (called by the `set_clipboard_item_last_used` command and the tray copy path) drops the cached result after `set_last_used` succeeds, because usage is device-local, writes no `search_outbox` event, and would otherwise leave a cached `lastUsedAt` page serving the pre-usage order.
- `rebuild_search_index` clears this cache along with Tantivy's `cached_ids`.
- `search_clipboard_items` also clears this cache when it applies pending outbox events, so stale pages are not served after a mutation; see lazy sync below.
- Each cache miss captures a generation/day token before querying candidates. Cache publication checks that token under the same mutex used by `clear`; an in-flight request cannot repopulate a cache invalidated by usage, synchronization, or a local midnight boundary.
- Cache miss when `max_results` is larger than the cached value ensures `searchPageSizeLimit` changes invalidate stale entries.

`search_clipboard_items` obtains a configured maximum, asks Tantivy for candidate IDs plus the true match total, fetches the complete bounded candidate set from SQLite, applies frontend sort rules globally, caches the full sorted result with its total/truncation, and only then slices the requested offset/limit. It returns a `SearchPage` envelope (`items`, `totalCount`, `truncated`); `truncated` is set when index matches beyond `max_results` were dropped, and the frontend (`searchClipboardHistory` → `SearchPage`) drives `searchHasMore` from the total and shows a truncation notice instead of ending pagination silently. `ClipboardRepository::get_items_by_ids` must read every requested active ID in safe query chunks and reconstruct caller order. Sorting after slicing breaks ordering across page boundaries and is forbidden. `apply_sort_rules` sorts stably on purpose: when sort fields tie, the incoming Tantivy relevance order deterministically remains the fallback order (an unstable sort permutes tied elements for larger inputs).

### Lazy sync on search

The capture thread and other mutation commands write to SQLite (triggering `search_outbox`) but do not themselves touch Tantivy. Index freshness is guaranteed by outbox draining, which runs either lazily on the search command or in a background worker, depending on `GeneralConfig.search_index_sync_mode`:

All `SearchSynchronizer` instances share the target index's synchronization mutex. Hold it from reading outbox events through resolving documents, committing the index, and acknowledging events. Full rebuilds hold the same lock across clearing, enqueueing, draining, and marking completion. The Tantivy writer lock alone cannot prevent an older document snapshot from overwriting a newer drain after its outbox events have already been acknowledged.

- `"lazy"` (default): `search_clipboard_items` drains the outbox via `SearchSynchronizer::sync_until_idle` before consulting the cache or Tantivy:
  - Empty outbox: one cheap `SELECT ... LIMIT` and no reader reload; the result cache is preserved so pagination stays a hit.
  - Pending events: Tantivy is updated, `cached_ids` is cleared by `apply_changes`, and the result cache is cleared so the re-query reflects newly captured or mutated items.
  - Sync failure is logged and swallowed so a broken index does not block search; results may be stale until the next successful sync or rebuild.
- `"background"`: a `SearchSyncWorker` started at app startup owns a dedicated database connection and polls the outbox every 500 ms, applying changes via the same `SearchSynchronizer`. The search command skips the blocking drain entirely (no outbox probe), so capture-heavy workloads do not add latency to the search hot path. The worker clears the `SearchResultCache` through an `on_changes_applied` callback. It also retries a required full rebuild (`SearchIndex::requires_full_rebuild`) on each tick, so a failed startup `initialize` does not leave the index incomplete for the whole session. Changing the mode requires a restart because the worker is only created at startup.

This is the only Tantivy search entry point; the CLI/local API uses SQLite scanning and is unaffected. Startup, `rebuild_search_index`, and storage-kind deletion still sync explicitly.

Any new index mutation path must invalidate `cached_ids`. Add a regression test showing an old query result cannot survive an upsert, delete, or rebuild.

## Frontend search request lifecycle

The main route debounces a first-page indexed search by 300 ms.

- Queries shorter than two characters, empty queries, recycle-bin filtering, and recognized date queries do not use Tantivy.
- `searchRequestId` discards stale first-page responses when the query/effect changes.
- `searchEpoch` (bumped by the `clipboard-history-invalidated` listener) re-runs the search effect; cancelling the in-flight request alone would drop a search that landed during the event and never retry it.
- The same effect reacts only to a narrow `searchSettingsKey` derived from `display.searchPageSize` and `searchSortRules` (the values themselves are read with `untrack`); either setting changing invalidates first-page and pagination request IDs before re-querying, while an unrelated settings change no longer restarts the search or resets pagination.
- Successful first pages replace the `indexed` view (`replaceViewItems`) and set `indexedQuery`, `searchOffset`, `searchTotalCount`/`searchTruncated`, and `searchHasMore`. An empty first page still switches the view on, so the panel shows "no match" instead of falling back to the history list; `closeSearchResults` is what returns to "no search displayed".
- `loadSearchPage()` uses `searchLoadRequestId`, the current offset, and `display.searchPageSize` for scroll pagination, and drops ids already in `indexedIds` before appending so OFFSET drift cannot produce a duplicate keyed-each key.
- `searchHasMore` is derived from the backend total (`searchOffset < totalCount`); an empty page still ends pagination as a backstop.

When changing query, filter, sort, or mutation behavior, audit both first-page and pagination request IDs. A stale pagination response must never append to a newer query. Keep offset reset and result invalidation together.

## Frontend spare-result cache

The pure maintenance logic lives in `src/lib/utils/item-store.ts` (`mergeSearchCachePage`, `promoteFromCache`, `trimLoadedHistory`) with Vitest coverage in `item-store.test.ts`; the route's `updateSearchCache(results)` stores first and subsequent-page search results whose ids the loaded active-history view does not already hold.

- Capacity is `searchCacheSize` (normalized 200–2000; default 500).
- `cacheIds` records insertion order — the access order lives in the store now, so there is no second id list to keep in step.
- `promoteFromCache(loadedIds)` removes ids once normal history pagination loads them; the record itself stays in `byId` because the history view owns it.
- The cache is separate from the `indexed` view; it is not the source of search ordering.

### FIFO/LRU evidence

`searchCacheEviction` exposes `fifo` and `lru`. FIFO preserves an existing entry's insertion position when it appears in another result page; LRU moves that id to the end of `cacheIds`. Both policies evict from the front at capacity, and eviction only drops the `byId` record once no view references it. Loaded-history promotion removes the promoted ids from the spare cache.

## Loaded-history tolerance trimming

`trimLoadedHistory()` limits ordinary loaded history separately:

- threshold: `pageSizeLimit + loadTolerance`;
- when exceeded, remove up to `loadTolerance` oldest non-deleted, non-favorite items;
- favorites and recycle-bin items are protected from this in-memory trimming, and so is any record the detail pane still displays;
- default `pageSizeLimit` is 500 and default tolerance is 100;
- the window is a cap, not a target: once the history view reaches the threshold, `loadActiveHistoryPage` sets `activeHistoryHasMore = false` because deeper OFFSET pages hold the oldest rows and would be evicted immediately (previously pagination kept fetch-then-evicting on every scroll event at the cap).

Changing this logic requires checking selected/detail items, virtual-scroll height state, active/deleted offsets, and the spare-result cache. In-memory trimming must not be confused with database history cleanup.

Active history is backed by `created_at_ms DESC LIMIT/OFFSET`. A committed insertion, re-copy dedup promotion (the timestamp bump shifts every later offset just like a fresh insert), soft/hard deletion, restore, bulk mutation, clear, or destructive invalidation can shift every later offset. Such paths call `invalidateActiveHistoryPagination()` to invalidate the in-flight request generation and reload page zero; do not merely append/remove locally and keep the old cursor. As a second line of defense, `loadActiveHistoryPage` drops ids that are already loaded when merging an offset>0 page, so a replayed row can never produce a duplicate keyed-each key even if a shift source is missed.

## Mutation invalidation

Record patches MUST go through the item store — `applyItemPatches` writes each patched record once and every view projects it, so a view cannot miss the update (the fan-out funnel this replaced existed because a hand-rolled mapping loop once left the spare cache with stale tags). `updateItem` resolves the original through `findLoadedItem` (the whole store), not the history view alone: a search result can be displayed only by the `indexed` or `cache` view, and resolving against the history view silently no-ops the action. Bulk mutations use the same helpers and roll back only the ids they touched, never a whole-view snapshot: `bulkFavorite` re-patches the batch, and `bulkRestore`, `bulkPermanentDelete`, `clearHistory`, `bulkDelete`, and the single-row permanent/hard delete paths capture the affected ids with `captureAffectedItems` and undo them with `restoreAffectedItems` (patch in place, re-insert what the mutation removed at the index it held in each view, and re-select only the rows that were selected). A whole-view snapshot cannot be rolled back safely because the persist call is async: a capture, search, or favorite toggle that lands in that window is discarded by the restore, and records that arrived during it are absent from the snapshot entirely, so they vanish from the history. Destructive storage-kind operations emit `clipboard-history-invalidated`, which removes IDs from the store, bumps `searchEpoch`, and resets affected deleted-history pagination.

Search index freshness still depends on SQLite outbox synchronization. When adding a mutation:

1. ensure the database trigger/outbox operation is correct;
2. ensure `SearchSynchronizer` applies it;
3. invalidate backend cached IDs;
4. update/remove the record in frontend collections;
5. reset pagination/request IDs when the result set behind an offset changed;
6. add tests for stale results and skipped/duplicated pages.

## Verification checklist

- Backend cache hit for a smaller/equal maximum.
- Backend miss for a larger maximum or different normalized query.
- Repository batch lookup above 500 IDs preserves the complete caller order.
- Cache invalidation after upsert, delete, and full rebuild.
- First-page stale response discarded after query change.
- Pagination does not append a stale prior-query page.
- No duplicates or skipped IDs across pages.
- FIFO behavior at capacity; LRU only when real access refresh exists.
- Promotion removes cached entries that enter normal loaded history.
- Loaded-history trimming protects favorites/deleted items and preserves selection/detail correctness.
- Sort-rule changes invalidate/reload results rather than reusing incompatible ordering.
