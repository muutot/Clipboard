// Folds a backend `clipboard-items-changed` payload into a local item store.
//
// Why a shared module: every window holds its own copy of the loaded rows, so
// the same handful of lines would otherwise be re-implemented per route — and a
// route that handled only the records half would keep showing rows that were
// deleted in the other window, or show a soft-deleted row as still active.
//
// The four lists in the payload map onto three store operations, and the order
// matters: content first, then the flag flips, then removals, then the promotion
// of used rows. A removal has to win, because an id that is gone must not survive
// a later patch — or be pulled back to the top after it was dropped.

import { toClipboardItem } from "$lib/services/clipboard/mapping";
import type { ClipboardItemsChangedPayload, PersistedClipboardItem } from "$lib/types/clipboard";
import {
  applyItemPatches,
  promoteItem,
  removeItems,
  replaceItem,
  type ItemStore,
} from "./item-store";

export interface ApplyItemsChangedOptions {
  /**
   * Whether a stamped usage moves rows to the top of the history.
   *
   * Left to the caller because it is the `Pin Copied to Top` setting, and the
   * setting lives with the route. The database order changes either way — usage
   * is stamped for every copy and paste — so this only decides whether the jump
   * is instant or waits for the next reload, exactly like the same setting does
   * for a copy started in this window.
   */
  promoteUsed?: boolean;
}

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
  options: ApplyItemsChangedOptions = {},
): ItemStore {
  let next = applyItemsChanged(store, payload.items ?? []);
  next = applyItemPatches(
    next,
    new Map((payload.usageUpdates ?? []).map(({ id, lastUsedAtMs }) => [id, { lastUsedAtMs }])),
  );

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

  if (options.promoteUsed) {
    // Back to front, so the first id in the payload ends up on top: promoting
    // pushes to the front, and two copies in one payload would otherwise land in
    // reverse order.
    for (const id of [...(payload.usedIds ?? [])].reverse()) {
      // Search order belongs to the backend's configured sort rules.
      const item = next.byId.get(id);
      if (item) next = promoteItem(next, item, "history");
    }
    if (payload.usageUpdates?.length) next = sortRecentHistory(next);
  }

  return next;
}

/** Match SQLite's timestamp/id ordering, including ties and clock rollback. */
export function sortRecentHistory(store: ItemStore): ItemStore {
  const historyIds = [...store.historyIds].sort((a, b) => {
    const delta = (store.byId.get(b)?.lastUsedAtMs ?? 0) - (store.byId.get(a)?.lastUsedAtMs ?? 0);
    return delta || (a < b ? 1 : a > b ? -1 : 0);
  });
  return { ...store, historyIds };
}
