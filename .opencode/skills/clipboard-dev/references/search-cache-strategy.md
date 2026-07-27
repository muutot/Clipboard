# Search Result Cache (SearchIndex)

`SearchIndex` caches Tantivy search results as `(query, max_results, Vec<item_id>)` in `cached_ids: Mutex`. Subsequent pagination calls read from this cache instead of re-running Tantivy.

**Cache key**: `(query: String, max_results: usize)` — exact match required for cache hit.

**Invalidation strategy** (all must be covered when modifying search):

| Scenario | Behavior |
|---|---|
| Query changes | Key mismatch → re-search |
| `max_results` increases | `cached_max < max_results` → cache miss → re-search |
| `max_results` decreases | `cached_max >= max_results` → cache hit → return subset |
| Index rebuild (`begin_full_rebuild`) | `clear_cached_ids()` |
| Index content change (`apply_changes`) | `clear_cached_ids()` — new/deleted items must invalidate |

**Callers must not cache stale state**: the frontend uses `searchRequestId` (incremented on query change) to discard in-flight responses.
