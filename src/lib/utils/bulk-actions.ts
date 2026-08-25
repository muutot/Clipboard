// Pure bulk-selection planning/snapshot helpers extracted from the main
// route. They contain no IPC, stores, or DOM — the route applies the returned
// plan through its persistence wrappers and applyItemPatches funnel.

import type { ClipboardItem } from "$lib/types/clipboard";

export interface BulkDeletePlan {
  /** Recycle-bin candidates: soft delete + keep in list as deleted. */
  softIds: string[];
  /** Already-deleted rows: permanent delete. */
  permanentIds: string[];
  /** Active rows with recycle bin off (or favorites excluded): hard delete. */
  hardIds: string[];
}

/**
 * Splits a selection into soft/permanent/hard delete buckets.
 * Favorites are never hard-deleted by capacity-style rules here; an active
 * favorite is simply left out of the plan (matching route behavior where the
 * UI prevents favoriting into deletion).
 */
export function planBulkDelete(
  selectedItems: ClipboardItem[],
  useRecycleBin: boolean,
): BulkDeletePlan {
  const softIds: string[] = [];
  const permanentIds: string[] = [];
  const hardIds: string[] = [];
  for (const item of selectedItems) {
    if (item.deleted) {
      permanentIds.push(item.id);
    } else if (!item.favorite) {
      (useRecycleBin ? softIds : hardIds).push(item.id);
    }
  }
  return { softIds, permanentIds, hardIds };
}

export interface BulkSnapshot {
  items: ClipboardItem[];
  indexedItems: ClipboardItem[] | null;
  selectedIds: Set<string>;
  detailItem: ClipboardItem | null;
}

/** Deep-enough snapshot for rollback after a failed async bulk mutation. */
export function captureBulkSnapshot(state: {
  items: ClipboardItem[];
  indexedItems: ClipboardItem[] | null;
  selectedIds: ReadonlySet<string>;
  detailItem: ClipboardItem | null;
}): BulkSnapshot {
  return {
    items: state.items.map((entry) => ({ ...entry })),
    indexedItems: state.indexedItems?.map((entry) => ({ ...entry })) ?? null,
    selectedIds: new Set(state.selectedIds),
    detailItem: state.detailItem ? { ...state.detailItem } : null,
  };
}

/** Applies a deleted/undeleted flag to the given ids across both lists. */
export function setDeletedFlags(
  items: ClipboardItem[],
  ids: ReadonlySet<string>,
  deleted: boolean,
): ClipboardItem[] {
  return items.map((item) =>
    ids.has(item.id) && item.deleted !== deleted ? { ...item, deleted } : item,
  );
}
