# Search Pagination and Cache Strategy

Search currently has three distinct pieces of state. Do not collapse them conceptually:

1. backend Tantivy ID cache in `src-tauri/src/search/index.rs`;
2. active paginated search results in `src/routes/+page.svelte::indexedItems`;
3. frontend spare-result cache in `src/routes/+page.svelte::searchCache`.

## Backend Tantivy ID cache

`SearchIndex` stores `cached_ids: Mutex<Option<(String, usize, Vec<String>)>>`.

- Key: normalized query plus requested `max_results`.
- Hit: query matches and cached maximum is at least the new maximum; return only the requested prefix while retaining the cached total.
- Miss: query differs or requested maximum grows; run Tantivy again and replace the cache.
- Empty query stores/returns an empty ID set.
- `apply_changes()` clears cached IDs after index mutations.
- `begin_full_rebuild()` clears cached IDs before rebuild.

## Backend SearchResultCache

`SearchResultCache` in `src-tauri/src/lib.rs` stores fully-sorted, fetched `ClipboardItem` results keyed by `(query, sort_rules, max_results)`.

- Hit: slice `[offset..offset+limit]` directly from the cached vector; no DB or index access needed.
- Miss: re-run the full search pipeline (Tantivy → SQL fetch → sort) and cache the result.
- `rebuild_search_index` clears this cache along with Tantivy's `cached_ids`.
- Cache miss when `max_results` is larger than the cached value ensures `searchPageSizeLimit` changes invalidate stale entries.

`search_clipboard_items` obtains a configured maximum, asks Tantivy for candidate IDs, fetches the complete bounded candidate set from SQLite, applies frontend sort rules globally, caches the full sorted result, and only then slices the requested offset/limit. `ClipboardRepository::get_items_by_ids` must read every requested active ID in safe query chunks and reconstruct caller order; a silent per-query cap truncates later search pages. Sorting after slicing breaks ordering across page boundaries and is forbidden. When sort fields tie, the incoming Tantivy relevance order remains the fallback order.

Any new index mutation path must invalidate `cached_ids`. Add a regression test showing an old query result cannot survive an upsert, delete, or rebuild.

## Frontend search request lifecycle

The main route debounces a first-page indexed search by 300 ms.

- Queries shorter than two characters, empty queries, recycle-bin filtering, and recognized date queries do not use Tantivy.
- `searchRequestId` discards stale first-page responses when the query/effect changes.
- The same effect synchronously tracks `display.searchPageSize` and `searchSortRules`; either setting changing invalidates first-page and pagination request IDs before re-querying.
- Successful first pages set `indexedItems`, `indexedQuery`, `searchOffset`, and `searchHasMore`.
- `loadSearchPage()` uses `searchLoadRequestId`, the current offset, and `display.searchPageSize` for scroll pagination.
- `searchHasMore` is inferred from a full page; an empty/short page ends pagination.

When changing query, filter, sort, or mutation behavior, audit both first-page and pagination request IDs. A stale pagination response must never append to a newer query. Keep offset reset and result invalidation together.

## Frontend spare-result cache

`updateSearchCache(results)` stores first and subsequent-page search results that are not already in the loaded active-history `items` list.

- Capacity is `searchCacheSize` (normalized 200–2000; default 500).
- `searchCacheAccessOrder` records insertion order.
- `promoteFromCache(loadedIds)` removes entries once normal history pagination loads them.
- The cache is separate from `indexedItems`; it is not the source of search ordering.

### FIFO/LRU evidence

`searchCacheEviction` exposes `fifo` and `lru`. FIFO preserves an existing entry's insertion position when it appears in another result page; LRU moves that entry to the end of `searchCacheAccessOrder`. Both policies evict from the front at capacity. Loaded-history promotion removes entries and their order records from the spare cache.

## Loaded-history tolerance trimming

`trimLoadedItems()` limits ordinary loaded history separately:

- threshold: `pageSizeLimit + loadTolerance`;
- when exceeded, remove up to `loadTolerance` oldest non-deleted, non-favorite items;
- favorites and recycle-bin items are protected from this in-memory trimming;
- default `pageSizeLimit` is 500 and default tolerance is 100.

Changing this logic requires checking selected/detail items, virtual-scroll height state, active/deleted offsets, and the spare-result cache. In-memory trimming must not be confused with database history cleanup.

Active history is backed by `created_at_ms DESC LIMIT/OFFSET`. A committed insertion, soft/hard deletion, restore, bulk mutation, clear, or destructive invalidation can shift every later offset. Such paths call `invalidateActiveHistoryPagination()` to invalidate the in-flight request generation and reload page zero; do not merely append/remove locally and keep the old cursor.

## Mutation invalidation

Create/update/favorite/delete/restore/permanent-delete flows update or invalidate `items` and `indexedItems` optimistically, then roll back on failure where implemented. Destructive storage-kind operations emit `clipboard-history-invalidated`, which removes IDs and resets affected deleted-history pagination.

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
