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
