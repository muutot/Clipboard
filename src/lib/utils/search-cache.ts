// Search-result cache and loaded-list maintenance extracted from the main
// route. All functions are pure: they take the current state plus policy
// inputs and return the next state so they can be unit tested without stores.

import type { ClipboardItem } from "$lib/types/clipboard";

export interface SearchCacheState {
  cache: ClipboardItem[];
  /** Insertion/access order of ids currently held in `cache`. */
  accessOrder: string[];
}

/**
 * Merges a freshly fetched backend search page into the spare cache. Items
 * already present in the loaded history list are dropped from the cache (the
 * authoritative copy lives in `items`); re-encountering a cached id refreshes
 * its content and, under LRU, its recency. The cache is then trimmed to
 * `max` entries by evicting the least recently used/inserted ids first.
 */
export function mergeSearchCachePage(
  state: SearchCacheState,
  options: {
    results: ClipboardItem[];
    loadedIds: ReadonlySet<string>;
    policy: "fifo" | "lru";
    max: number;
  },
): SearchCacheState {
  const cacheById = new Map(state.cache.map((item) => [item.id, item]));
  let accessOrder = state.accessOrder.filter((id) => cacheById.has(id));

  for (const item of options.results) {
    if (options.loadedIds.has(item.id)) {
      cacheById.delete(item.id);
      accessOrder = accessOrder.filter((id) => id !== item.id);
      continue;
    }

    const cached = cacheById.has(item.id);
    cacheById.set(item.id, item);
    if (!cached) {
      accessOrder.push(item.id);
    } else if (options.policy === "lru") {
      accessOrder = accessOrder.filter((id) => id !== item.id);
      accessOrder.push(item.id);
    }
  }

  while (accessOrder.length > options.max) {
    const id = accessOrder.shift();
    if (id) cacheById.delete(id);
  }

  return {
    accessOrder,
    cache: accessOrder.flatMap((id) => {
      const item = cacheById.get(id);
      return item ? [item] : [];
    }),
  };
}

/** Removes promoted ids from the spare cache once the live list holds them. */
export function promoteFromCache(
  state: SearchCacheState,
  loadedIds: ReadonlySet<string>,
): SearchCacheState {
  if (!loadedIds.size) return state;
  let changed = false;
  const promoted = new Set<string>();
  for (const id of loadedIds) {
    if (state.cache.some((entry) => entry.id === id)) promoted.add(id);
  }
  if (!promoted.size) return state;

  const cache = state.cache.filter((entry) => {
    if (promoted.has(entry.id)) {
      changed = true;
      return false;
    }
    return true;
  });
  const accessOrder = state.accessOrder.filter((id) => !promoted.has(id));
  return changed ? { cache, accessOrder } : state;
}

/**
 * Keeps the in-memory history list bounded: when it exceeds `limit +
 * tolerance`, up to `tolerance` oldest non-favorite non-deleted entries are
 * evicted (favorites and recycle-bin rows are never touched).
 */
export function trimLoadedItems(
  items: ClipboardItem[],
  limit: number,
  tolerance: number,
): ClipboardItem[] {
  const max = limit + tolerance;
  if (items.length <= max) return items;

  const evictable = items
    .filter((entry) => !entry.deleted && !entry.favorite)
    .sort((a, b) => a.createdAt - b.createdAt);

  const toEvict = new Set(evictable.slice(0, tolerance).map((entry) => entry.id));
  return items.filter((entry) => !toEvict.has(entry.id));
}
