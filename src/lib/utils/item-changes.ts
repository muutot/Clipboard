// Folds a backend `clipboard-items-changed` payload into a local item store.
//
// Why a shared module: every window holds its own copy of the loaded rows, so
// the same handful of lines would otherwise be re-implemented per route — and a
// route that handled only the records half would keep showing rows that were
// deleted in the other window, or show a soft-deleted row as still active.
//
// The four lists in the payload map onto three store operations, and the order
// matters: content first, then the flag flips, then removals. A removal has to
// win, because an id that is gone must not survive a later patch.

import { toClipboardItem } from "$lib/services/clipboard";
import type { ClipboardItemsChangedPayload, PersistedClipboardItem } from "$lib/types/clipboard";
import { applyItemPatches, removeItems, replaceItem, type ItemStore } from "./item-store";

export function applyItemsChanged(
  store: ItemStore,
  items: readonly PersistedClipboardItem[],
): ItemStore {
  let next = store;
  for (const record of items) {
    const item = toClipboardItem(record);
    // Only rows this window actually displays are replaced. An id it never
    // loaded is not its business, and writing it in would resurrect a row the
    // user cannot see (or one trimmed out of the loaded window).
    if (next.byId.has(item.id)) next = replaceItem(next, item);
  }
  return next;
}

export function applyItemsChangedEvent(
  store: ItemStore,
  payload: ClipboardItemsChangedPayload,
): ItemStore {
  let next = applyItemsChanged(store, payload.items ?? []);

  const deleted = payload.deletedIds ?? [];
  if (deleted.length) {
    // Soft delete keeps the row; only the flag the receiver holds flips, so the
    // recycle-bin filter can still show it.
    next = applyItemPatches(next, new Map(deleted.map((id) => [id, { deleted: true }])));
  }

  const restored = payload.restoredIds ?? [];
  if (restored.length) {
    next = applyItemPatches(next, new Map(restored.map((id) => [id, { deleted: false }])));
  }

  const removed = payload.removedIds ?? [];
  if (removed.length) {
    // Permanent delete leaves every view at once, the spare search cache
    // included — otherwise a deleted row could resurface from it.
    next = removeItems(next, new Set(removed));
  }

  return next;
}
