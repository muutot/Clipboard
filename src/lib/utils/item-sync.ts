// Pure four-copy fan-out extracted from the main route. The route keeps one
// item in up to four parallel lists (items, indexedItems, searchCache,
// detailItem); every mutation must reach all copies or the UI shows stale
// rows (the historical drift was a tags update that skipped searchCache).
// These functions contain no IPC, stores, or DOM — the route applies the
// returned copies through reassignment so Svelte reactivity fires.

import type { ClipboardItem } from "$lib/types/clipboard";

export interface ItemCopies {
  items: ClipboardItem[];
  indexedItems: ClipboardItem[] | null;
  searchCache: ClipboardItem[];
  detailItem: ClipboardItem | null;
}

/** Fans one patch set out to every copy in a single pass over each list. */
export function applyItemPatchesToCopies(
  copies: ItemCopies,
  patches: ReadonlyMap<string, Partial<ClipboardItem>>,
): ItemCopies {
  if (patches.size === 0) return copies;
  const apply = (entry: ClipboardItem): ClipboardItem => {
    const patch = patches.get(entry.id);
    return patch ? { ...entry, ...patch } : entry;
  };
  const detailPatch = copies.detailItem ? patches.get(copies.detailItem.id) : undefined;
  return {
    items: copies.items.map(apply),
    indexedItems: copies.indexedItems?.map(apply) ?? null,
    searchCache: copies.searchCache.map(apply),
    detailItem:
      copies.detailItem && detailPatch
        ? { ...copies.detailItem, ...detailPatch }
        : copies.detailItem,
  };
}

/**
 * Removal counterpart: drops ids from every copy. Purging searchCache here
 * closes the gap where a permanently deleted entry could later resurface
 * from the spare-result cache.
 */
export function removeItemsFromCopies(copies: ItemCopies, ids: ReadonlySet<string>): ItemCopies {
  if (ids.size === 0) return copies;
  return {
    items: copies.items.filter((item) => !ids.has(item.id)),
    indexedItems: copies.indexedItems?.filter((item) => !ids.has(item.id)) ?? null,
    searchCache: copies.searchCache.filter((item) => !ids.has(item.id)),
    detailItem: copies.detailItem && ids.has(copies.detailItem.id) ? null : copies.detailItem,
  };
}

/** Swaps a fully materialized record into every copy holding its id. */
export function replaceItemInCopies(copies: ItemCopies, updated: ClipboardItem): ItemCopies {
  const replace = (item: ClipboardItem) => (item.id === updated.id ? updated : item);
  return {
    items: copies.items.map(replace),
    indexedItems: copies.indexedItems?.map(replace) ?? null,
    searchCache: copies.searchCache.map(replace),
    detailItem: copies.detailItem?.id === updated.id ? updated : copies.detailItem,
  };
}

export interface AffectedItemSnapshot {
  /** Previous version of each affected id, taken from the copy that held it. */
  readonly previous: ReadonlyMap<string, ClipboardItem>;
  /** Index each affected id occupied in `items`, so a rollback restores order. */
  readonly positions: ReadonlyMap<string, number>;
  /** The detail item before the mutation, when it was one of the affected ids. */
  readonly detailItem: ClipboardItem | null;
  /** The affected ids that were selected before the mutation. */
  readonly selected: ReadonlySet<string>;
}

/**
 * Records only the ids a bulk mutation is about to touch.
 *
 * A whole-array snapshot cannot be rolled back safely: the persist call is
 * async, and while it is in flight a clipboard capture, a search, or a favorite
 * toggle can change the very arrays the snapshot froze. Restoring the snapshot
 * then silently discards those newer changes — and items that arrived during
 * the window are missing from the snapshot entirely, so they vanish from the
 * history. Capturing per id keeps the rollback scoped to what the user asked
 * to change.
 */
export function captureAffectedItems(
  copies: ItemCopies,
  ids: ReadonlySet<string>,
  selectedIds: ReadonlySet<string> = new Set(),
): AffectedItemSnapshot {
  const previous = new Map<string, ClipboardItem>();
  const positions = new Map<string, number>();
  copies.items.forEach((item, index) => {
    if (!ids.has(item.id) || previous.has(item.id)) return;
    previous.set(item.id, { ...item });
    positions.set(item.id, index);
  });
  // An id loaded only into a search/cache/detail copy still needs its previous
  // version, or a rollback would patch it to nothing.
  for (const item of copies.indexedItems ?? []) {
    if (ids.has(item.id) && !previous.has(item.id)) previous.set(item.id, { ...item });
  }
  for (const item of copies.searchCache) {
    if (ids.has(item.id) && !previous.has(item.id)) previous.set(item.id, { ...item });
  }
  if (copies.detailItem && ids.has(copies.detailItem.id) && !previous.has(copies.detailItem.id)) {
    previous.set(copies.detailItem.id, { ...copies.detailItem });
  }

  const selected = new Set<string>();
  for (const id of ids) {
    if (selectedIds.has(id)) selected.add(id);
  }

  return {
    previous,
    positions,
    detailItem:
      copies.detailItem && ids.has(copies.detailItem.id) ? { ...copies.detailItem } : null,
    selected,
  };
}

/**
 * Puts the captured ids back into every copy, leaving all other ids alone.
 *
 * An id still present in a copy is patched in place; one the mutation removed
 * is re-inserted at its recorded index so the recency order survives. Records
 * that arrived during the async window are neither patched nor reordered.
 */
export function restoreAffectedItemsToCopies(
  copies: ItemCopies,
  snapshot: AffectedItemSnapshot,
): ItemCopies {
  if (snapshot.previous.size === 0) return copies;

  const restoreList = (list: ClipboardItem[]): ClipboardItem[] => {
    const present = new Set<string>();
    const restored = list.map((item) => {
      const before = snapshot.previous.get(item.id);
      if (!before) return item;
      present.add(item.id);
      return { ...item, ...before };
    });

    const missing = [...snapshot.previous]
      .filter(([id]) => !present.has(id))
      .sort((a, b) => (snapshot.positions.get(a[0]) ?? 0) - (snapshot.positions.get(b[0]) ?? 0));
    // Each insertion shifts every later position by one, so track the drift.
    let drift = 0;
    for (const [id, before] of missing) {
      const at = Math.min((snapshot.positions.get(id) ?? restored.length) + drift, restored.length);
      restored.splice(at, 0, before);
      drift += 1;
    }
    return restored;
  };

  return {
    items: restoreList(copies.items),
    indexedItems: copies.indexedItems ? restoreList(copies.indexedItems) : null,
    searchCache: restoreList(copies.searchCache),
    // Only reclaim an emptied detail pane. If the user opened a different record
    // during the window, that newer choice wins.
    detailItem: copies.detailItem
      ? copies.detailItem
      : snapshot.detailItem
        ? { ...snapshot.detailItem }
        : null,
  };
}

/** Fallback lookup across every copy, in route display-priority order. */
export function findLoadedItemInCopies(copies: ItemCopies, id: string): ClipboardItem | undefined {
  return (
    copies.items.find((item) => item.id === id) ??
    copies.indexedItems?.find((item) => item.id === id) ??
    copies.searchCache.find((item) => item.id === id) ??
    (copies.detailItem?.id === id ? copies.detailItem : undefined)
  );
}

/** Renames one tag on an item, dropping duplicates the rename would create. */
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

/** Removes one tag from an item. */
export function removeItemTag(item: ClipboardItem, name: string): ClipboardItem {
  const tags = item.tags ?? [];
  if (!tags.includes(name)) return item;
  return { ...item, tags: tags.filter((tag) => tag !== name) };
}

/**
 * Folds a recycle-bin page into the loaded list. Suppressed ids (locally
 * restored/permanently deleted while the request was in flight) are dropped,
 * and a persisted row never overwrites a local restore — a stale response must
 * not resurrect a row the user already acted on.
 */
export function mergeDeletedHistoryPage(
  items: ClipboardItem[],
  page: ClipboardItem[],
  suppressedIds: ReadonlySet<string>,
): ClipboardItem[] {
  const incoming = new Map(page.map((item) => [item.id, item]));
  const merged = items
    .filter((item) => !item.deleted || !suppressedIds.has(item.id))
    .map((item) => {
      const persisted = incoming.get(item.id);
      if (!persisted || !item.deleted) return item;
      return { ...item, ...persisted, deleted: true };
    });
  const existingIds = new Set(merged.map((item) => item.id));
  for (const item of page) {
    if (suppressedIds.has(item.id)) continue;
    if (!existingIds.has(item.id)) merged.push({ ...item, deleted: true });
  }
  return merged;
}
