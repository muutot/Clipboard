// Pure bulk-selection planning helper extracted from the main route. It
// contains no IPC, stores, or DOM — the route applies the returned plan
// through its persistence wrappers and the item-store mutators.

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
