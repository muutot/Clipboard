// Single source of truth for every loaded clipboard record.
//
// The route used to keep one record in up to four parallel arrays (items,
// indexedItems, searchCache, detailItem) and fan every mutation out to each
// copy; a hand-rolled mapping loop once left the spare search cache showing
// stale tags. Here the records live in `byId` exactly once and the four views
// carry only ids — ordering and membership — so two views cannot disagree
// about a record's content by construction.
//
// Pure logic, no IPC, stores, or DOM: every function returns the next store
// instead of mutating the current one, and the route holds the result in
// `$state.raw` so a plain `Map` is never wrapped in a Svelte deep proxy.

import type { ClipboardItem } from "$lib/types/clipboard";

/** The three list views; the detail pane is the single-record fourth view. */
export type ItemView = "history" | "indexed" | "cache";

const VIEWS: readonly ItemView[] = ["history", "indexed", "cache"];

export interface ItemStore {
  /**
   * Every loaded record, keyed by id, and nothing else: `byId` holds exactly
   * the union of the four views. Replaced wholesale on every write, so a
   * `$derived` view recomputes from a fresh map.
   */
  readonly byId: ReadonlyMap<string, ClipboardItem>;
  /** Loaded history page (active list or recycle bin) in display order. */
  readonly historyIds: readonly string[];
  /** Active search page order; `null` while no indexed search is displayed. */
  readonly indexedIds: readonly string[] | null;
  /** Spare search-result cache in insertion/access order. */
  readonly cacheIds: readonly string[];
  /** Record shown in the detail pane. */
  readonly detailId: string | null;
}

export function createItemStore(history: readonly ClipboardItem[] = []): ItemStore {
  if (history.length === 0) {
    return { byId: new Map(), historyIds: [], indexedIds: null, cacheIds: [], detailId: null };
  }
  const byId = new Map<string, ClipboardItem>();
  for (const record of history) byId.set(record.id, record);
  return {
    byId,
    historyIds: history.map((record) => record.id),
    indexedIds: null,
    cacheIds: [],
    detailId: null,
  };
}

function viewIds(store: ItemStore, view: ItemView): readonly string[] {
  return view === "history"
    ? store.historyIds
    : view === "indexed"
      ? (store.indexedIds ?? [])
      : store.cacheIds;
}

/** Materializes one list view; ids missing from `byId` are skipped. */
export function getItems(store: ItemStore, view: ItemView): ClipboardItem[] {
  const items: ClipboardItem[] = [];
  for (const id of viewIds(store, view)) {
    const record = store.byId.get(id);
    if (record) items.push(record);
  }
  return items;
}

/** Materializes the active search view, preserving "no search" as `null`. */
export function getIndexedItems(store: ItemStore): ClipboardItem[] | null {
  return store.indexedIds === null ? null : getItems(store, "indexed");
}

export function getDetailItem(store: ItemStore): ClipboardItem | null {
  return store.detailId === null ? null : (store.byId.get(store.detailId) ?? null);
}

/**
 * Drops records no view displays any more. Without this a record that left the
 * history list, the search views, and the detail pane would stay in `byId`
 * forever, since the map — not the views — now owns its lifetime.
 */
function pruneUnreferenced(
  byId: Map<string, ClipboardItem>,
  historyIds: readonly string[],
  indexedIds: readonly string[] | null,
  cacheIds: readonly string[],
  detailId: string | null,
): boolean {
  const live = new Set<string>(historyIds);
  for (const id of indexedIds ?? []) live.add(id);
  for (const id of cacheIds) live.add(id);
  if (detailId !== null) live.add(detailId);
  let pruned = false;
  for (const id of [...byId.keys()]) {
    if (!live.has(id)) {
      byId.delete(id);
      pruned = true;
    }
  }
  return pruned;
}

/** Resolves a record from any view — the single version, by construction. */
export function findLoadedItem(store: ItemStore, id: string): ClipboardItem | undefined {
  return store.byId.get(id);
}

/**
 * THE funnel for record mutations: writes each patched record once, so every
 * view that displays it changes in the same pass. Ids that are not loaded are
 * skipped rather than resurrected.
 */
export function applyItemPatches(
  store: ItemStore,
  patches: ReadonlyMap<string, Partial<ClipboardItem>>,
): ItemStore {
  if (patches.size === 0) return store;
  const byId = new Map(store.byId);
  let changed = false;
  for (const [id, patch] of patches) {
    const current = byId.get(id);
    if (!current) continue;
    byId.set(id, { ...current, ...patch });
    changed = true;
  }
  if (!changed) return store;
  return { ...store, byId };
}

/**
 * Removal counterpart: drops ids from the record map and from every view that
 * displayed them. Purging the spare cache here closes the gap where a
 * permanently deleted entry could later resurface from it.
 */
export function removeItems(store: ItemStore, ids: ReadonlySet<string>): ItemStore {
  if (ids.size === 0) return store;
  const historyIds = store.historyIds.filter((id) => !ids.has(id));
  const indexedIds = store.indexedIds?.filter((id) => !ids.has(id)) ?? null;
  const cacheIds = store.cacheIds.filter((id) => !ids.has(id));
  const detailId = store.detailId !== null && ids.has(store.detailId) ? null : store.detailId;
  const byId = new Map(store.byId);
  for (const id of ids) byId.delete(id);
  const pruned = pruneUnreferenced(byId, historyIds, indexedIds, cacheIds, detailId);
  if (
    !pruned &&
    historyIds.length === store.historyIds.length &&
    cacheIds.length === store.cacheIds.length &&
    (indexedIds === null || indexedIds.length === store.indexedIds?.length) &&
    detailId === store.detailId
  ) {
    return store;
  }
  return { byId, historyIds, indexedIds, cacheIds, detailId };
}

/** Swaps a fully materialized record in, for funnels that rebuild whole sets. */
export function replaceItem(store: ItemStore, updated: ClipboardItem): ItemStore {
  if (!store.byId.has(updated.id)) return store;
  const byId = new Map(store.byId);
  byId.set(updated.id, updated);
  return { ...store, byId };
}

/** Points the detail pane at a record, or clears it. */
export function setDetailItem(store: ItemStore, item: ClipboardItem | null): ItemStore {
  if (item === null) {
    if (store.detailId === null) return store;
    const byId = new Map(store.byId);
    pruneUnreferenced(byId, store.historyIds, store.indexedIds, store.cacheIds, null);
    return { ...store, byId, detailId: null };
  }
  const byId = new Map(store.byId);
  byId.set(item.id, item);
  // Re-pointing the pane can leave the previous record displayed by no view at
  // all — a search-cache-only id the user opened, say. The map owns the record
  // lifetime now, so release it here instead of leaking it into every later
  // derived view.
  pruneUnreferenced(byId, store.historyIds, store.indexedIds, store.cacheIds, item.id);
  return { ...store, byId, detailId: item.id };
}

/**
 * Appends freshly loaded records to a view. Records already in that view keep
 * their position; their content is refreshed from the incoming page. Appending
 * to the `indexed` view is what turns "no search displayed" into a displayed
 * search page, so a first result page switches the view on.
 */
export function appendItems(
  store: ItemStore,
  records: readonly ClipboardItem[],
  view: ItemView,
): ItemStore {
  if (records.length === 0) return store;
  const byId = new Map(store.byId);
  const ids = [...viewIds(store, view)];
  const seen = new Set(ids);
  let changed = false;
  for (const record of records) {
    if (byId.get(record.id) !== record) {
      byId.set(record.id, record);
      changed = true;
    }
    if (!seen.has(record.id)) {
      seen.add(record.id);
      ids.push(record.id);
      changed = true;
    }
  }
  if (!changed) return store;
  return withView(store, view, ids, byId);
}

/**
 * Moves a record to the top of a view and refreshes it. Re-copying an existing
 * entry is de-duplicated on (kind, content_hash): the row keeps its id and its
 * original `created_at_ms`, but the list order follows array position rather
 * than the timestamp, so the just-copied entry has to be pinned visibly.
 */
export function promoteItem(store: ItemStore, item: ClipboardItem, view: ItemView): ItemStore {
  const byId = new Map(store.byId);
  byId.set(item.id, item);
  const ids = [item.id, ...viewIds(store, view).filter((id) => id !== item.id)];
  return withView(store, view, ids, byId);
}

/** Empties one view and releases the records it alone displayed. */
export function clearView(store: ItemStore, view: ItemView): ItemStore {
  // `indexed: null` means "no search is displayed" — already empty, and it must
  // stay distinguishable from an active search that matched zero rows.
  if (view === "indexed" && store.indexedIds === null) return store;
  if (viewIds(store, view).length === 0) return store;
  const byId = new Map(store.byId);
  if (view === "history") {
    pruneUnreferenced(byId, [], store.indexedIds, store.cacheIds, store.detailId);
    return { ...store, byId, historyIds: [] };
  }
  if (view === "indexed") {
    pruneUnreferenced(byId, store.historyIds, [], store.cacheIds, store.detailId);
    return { ...store, byId, indexedIds: [] };
  }
  pruneUnreferenced(byId, store.historyIds, store.indexedIds, [], store.detailId);
  return { ...store, byId, cacheIds: [] };
}

function withView(
  store: ItemStore,
  view: ItemView,
  ids: string[],
  byId: Map<string, ClipboardItem>,
): ItemStore {
  return view === "history"
    ? { ...store, byId, historyIds: ids }
    : view === "indexed"
      ? { ...store, byId, indexedIds: ids }
      : { ...store, byId, cacheIds: ids };
}

/** Replaces a view's contents with `records`, releasing what it dropped. */
export function replaceViewItems(
  store: ItemStore,
  records: readonly ClipboardItem[],
  view: ItemView,
): ItemStore {
  const cleared = clearView(store, view);
  // An empty result page is still a *displayed* search: the panel has to show
  // "no match" rather than silently fall back to the history list, so the
  // indexed view must switch on even with nothing in it.
  if (view === "indexed" && records.length === 0 && cleared.indexedIds === null) {
    return { ...cleared, indexedIds: [] };
  }
  return appendItems(cleared, records, view);
}

/**
 * Drops back to "no search displayed" (`indexedIds: null`), which is not the
 * same state as an active search that matched zero rows.
 */
export function closeSearchResults(store: ItemStore): ItemStore {
  if (store.indexedIds === null) return store;
  const byId = new Map(store.byId);
  pruneUnreferenced(byId, store.historyIds, null, store.cacheIds, store.detailId);
  return { ...store, byId, indexedIds: null };
}

export interface AffectedItemSnapshot {
  /** Previous version of each affected id, taken from the record map. */
  readonly previous: ReadonlyMap<string, ClipboardItem>;
  /** Index each affected id occupied per view, so a rollback restores order. */
  readonly positions: ReadonlyMap<string, ReadonlyMap<ItemView, number>>;
  /** The detail id before the mutation, when it was one of the affected ids. */
  readonly detailId: string | null;
  /** The affected ids that were selected before the mutation. */
  readonly selected: ReadonlySet<string>;
}

/**
 * Records only the ids a bulk mutation is about to touch.
 *
 * A whole-array snapshot cannot be rolled back safely: the persist call is
 * async, and while it is in flight a clipboard capture, a search, or a favorite
 * toggle can change the very lists the snapshot froze. Restoring the snapshot
 * then silently discards those newer changes — and records that arrived during
 * the window are missing from it entirely, so they vanish from the history.
 * Capturing per id keeps the rollback scoped to what the user asked to change.
 */
export function captureAffectedItems(
  store: ItemStore,
  ids: ReadonlySet<string>,
  selectedIds: ReadonlySet<string> = new Set(),
): AffectedItemSnapshot {
  const previous = new Map<string, ClipboardItem>();
  const positions = new Map<string, Map<ItemView, number>>();
  for (const view of VIEWS) {
    viewIds(store, view).forEach((id, index) => {
      if (!ids.has(id)) return;
      const record = store.byId.get(id);
      if (record && !previous.has(id)) previous.set(id, { ...record });
      const slot = positions.get(id) ?? new Map<ItemView, number>();
      slot.set(view, index);
      positions.set(id, slot);
    });
  }
  // A record shown only in the detail pane is still displayed, so it needs its
  // previous version and its view recorded too.
  if (store.detailId !== null && ids.has(store.detailId)) {
    const record = store.byId.get(store.detailId);
    if (record && !previous.has(store.detailId)) previous.set(store.detailId, { ...record });
  }

  const selected = new Set<string>();
  for (const id of ids) {
    if (selectedIds.has(id)) selected.add(id);
  }

  return {
    previous,
    positions,
    detailId:
      store.detailId !== null && ids.has(store.detailId) && previous.has(store.detailId)
        ? store.detailId
        : null,
    selected,
  };
}

/**
 * Puts the captured ids back, leaving all other ids alone. An id still loaded
 * is patched in place; one the mutation removed is re-inserted at the index it
 * held in each view, so recency order survives. Records that arrived during the
 * async window are neither patched nor reordered.
 */
export function restoreAffectedItems(store: ItemStore, snapshot: AffectedItemSnapshot): ItemStore {
  if (snapshot.previous.size === 0) return store;

  const byId = new Map(store.byId);
  const views: Record<ItemView, string[]> = {
    history: [...store.historyIds],
    indexed: store.indexedIds === null ? [] : [...store.indexedIds],
    cache: [...store.cacheIds],
  };
  const drift: Record<ItemView, number> = { history: 0, indexed: 0, cache: 0 };
  const missing: [string, ClipboardItem, ReadonlyMap<ItemView, number> | undefined][] = [];

  for (const [id, before] of snapshot.previous) {
    const current = byId.get(id);
    if (current) {
      byId.set(id, { ...current, ...before });
      continue;
    }
    missing.push([id, before, snapshot.positions.get(id)]);
  }

  // Restore in recorded order so each insertion shifts only later positions.
  missing.sort((a, b) => firstPosition(a[2]) - firstPosition(b[2]));
  for (const [id, before, slot] of missing) {
    byId.set(id, before);
    if (!slot) continue;
    for (const view of VIEWS) {
      const at = slot.get(view);
      if (at === undefined) continue;
      const list = views[view];
      list.splice(Math.min(at + drift[view], list.length), 0, id);
      drift[view] += 1;
    }
  }

  // Only reclaim an emptied detail pane. If the user opened a different record
  // during the window, that newer choice wins.
  const detailId =
    store.detailId === null && snapshot.detailId !== null && byId.has(snapshot.detailId)
      ? snapshot.detailId
      : store.detailId;

  return {
    byId,
    historyIds: views.history,
    indexedIds: store.indexedIds === null ? null : views.indexed,
    cacheIds: views.cache,
    detailId,
  };
}

function firstPosition(slot: ReadonlyMap<ItemView, number> | undefined): number {
  if (!slot) return Number.MAX_SAFE_INTEGER;
  let lowest = Number.MAX_SAFE_INTEGER;
  for (const view of VIEWS) {
    const at = slot.get(view);
    if (at !== undefined && at < lowest) lowest = at;
  }
  return lowest;
}

/** Renames one tag on a record, dropping duplicates the rename would create. */
export function rewriteItemTags(
  item: ClipboardItem,
  oldName: string,
  newName: string,
): ClipboardItem {
  const tags = item.tags ?? [];
  if (!tags.includes(oldName)) return item;
  const seen = new Set<string>();
  const next: string[] = [];
  for (const tag of tags) {
    const value = tag === oldName ? newName : tag;
    if (!seen.has(value)) {
      seen.add(value);
      next.push(value);
    }
  }
  return { ...item, tags: next };
}

/** Removes one tag from a record. */
export function removeItemTag(item: ClipboardItem, name: string): ClipboardItem {
  const tags = item.tags ?? [];
  if (!tags.includes(name)) return item;
  return { ...item, tags: tags.filter((tag) => tag !== name) };
}

/**
 * Folds a recycle-bin page into the loaded history view. Suppressed ids
 * (locally restored/permanently deleted while the request was in flight) are
 * dropped, and a persisted row never overwrites a local restore — a stale
 * response must not resurrect a row the user already acted on.
 */
export function mergeDeletedHistoryPage(
  store: ItemStore,
  page: readonly ClipboardItem[],
  suppressedIds: ReadonlySet<string>,
): ItemStore {
  if (page.length === 0) return store;
  const incoming = new Map(page.map((record) => [record.id, record]));
  const byId = new Map(store.byId);
  const historyIds: string[] = [];
  const kept = new Set<string>();

  for (const id of store.historyIds) {
    const current = byId.get(id);
    if (!current) continue;
    if (current.deleted && suppressedIds.has(id)) continue;
    const persisted = incoming.get(id);
    if (persisted && current.deleted) byId.set(id, { ...current, ...persisted, deleted: true });
    historyIds.push(id);
    kept.add(id);
  }

  for (const record of page) {
    if (suppressedIds.has(record.id) || kept.has(record.id)) continue;
    byId.set(record.id, { ...record, deleted: true });
    historyIds.push(record.id);
    kept.add(record.id);
  }

  pruneUnreferenced(byId, historyIds, store.indexedIds, store.cacheIds, store.detailId);
  return { ...store, byId, historyIds };
}

/**
 * Merges a freshly fetched backend search page into the spare cache. Records
 * already displayed by the loaded history are dropped from the cache (that
 * view owns them now); re-encountering a cached id refreshes its content and,
 * under LRU, its recency. The cache is then trimmed to `max` entries by
 * evicting the least recently used/inserted ids first.
 */
export function mergeSearchCachePage(
  store: ItemStore,
  options: {
    results: readonly ClipboardItem[];
    loadedIds: ReadonlySet<string>;
    policy: "fifo" | "lru";
    max: number;
  },
): ItemStore {
  if (options.results.length === 0 && store.cacheIds.length <= options.max) return store;
  const byId = new Map(store.byId);
  // A record can leave the map entirely while an id lingers in the order list,
  // so reconcile the order against the map before merging.
  let cacheIds = store.cacheIds.filter((id) => byId.has(id));

  for (const record of options.results) {
    if (options.loadedIds.has(record.id)) {
      // Only the cache membership goes: the loaded history view still displays
      // the record, so it must stay in the map.
      cacheIds = cacheIds.filter((id) => id !== record.id);
      continue;
    }
    const cached = cacheIds.includes(record.id);
    byId.set(record.id, record);
    if (!cached) {
      cacheIds.push(record.id);
    } else if (options.policy === "lru") {
      cacheIds = [...cacheIds.filter((id) => id !== record.id), record.id];
    }
  }

  while (cacheIds.length > options.max) cacheIds.shift();
  pruneUnreferenced(byId, store.historyIds, store.indexedIds, cacheIds, store.detailId);
  return { ...store, byId, cacheIds };
}

/** Removes promoted ids from the spare cache once the live list holds them. */
export function promoteFromCache(store: ItemStore, loadedIds: ReadonlySet<string>): ItemStore {
  if (!loadedIds.size) return store;
  const promoted = store.cacheIds.filter((id) => loadedIds.has(id));
  if (!promoted.length) return store;
  return { ...store, cacheIds: store.cacheIds.filter((id) => !loadedIds.has(id)) };
}

/**
 * Keeps the in-memory history view bounded: when it exceeds `limit +
 * tolerance`, up to `tolerance` oldest non-favorite non-deleted records are
 * released (favorites and recycle-bin rows are never touched).
 */
export function trimLoadedHistory(
  store: ItemStore,
  options: { limit: number; tolerance: number },
): ItemStore {
  const max = options.limit + options.tolerance;
  if (store.historyIds.length <= max) return store;

  const evictable = store.historyIds
    .map((id, index) => ({ id, index, record: store.byId.get(id) }))
    .filter((entry) => entry.record && !entry.record.deleted && !entry.record.favorite)
    .sort((a, b) => a.record!.createdAt - b.record!.createdAt || a.index - b.index);

  const toEvict = new Set(evictable.slice(0, options.tolerance).map((entry) => entry.id));
  if (toEvict.size === 0) return store;
  return removeItems(store, toEvict);
}
