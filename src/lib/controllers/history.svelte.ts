import { onDestroy } from "svelte";
import { loadClipboardHistory, loadDeletedClipboardHistory } from "$lib/services/clipboard";
import { isTauriRuntime } from "$lib/services/runtime";
import type {
  HistoryCursorPayload,
  HistoryFilterArgs,
  GeneralSettings,
} from "$lib/types/clipboard";
import type { ItemStoreView } from "$lib/utils/store/item-store-view.svelte";
import {
  appendItems,
  replaceViewItems,
  mergeDeletedHistoryPage,
  promoteFromCache as promoteCachedEntries,
  trimLoadedHistory,
} from "$lib/utils/store/item-store";
export interface HistoryDependencies {
  itemStore: ItemStoreView;
  readonly settings: GeneralSettings;
  filter(): HistoryFilterArgs;
  onError(key: string): void;
}
/** Owns pagination/cancellation state; records remain exclusively in the supplied item store. */
export function createHistoryController(deps: HistoryDependencies) {
  const DELETED_HISTORY_PAGE_SIZE = 100;

  let deletedHistoryLoaded = $state(false);

  let deletedHistoryLoading = $state(false);

  let deletedHistoryOffset = $state(0);

  let deletedHistoryHasMore = $state(true);

  let deletedHistoryRequestId = 0;

  let activeHistoryLoading = $state(false);

  // Keyset pagination anchor for active history: the (last_used_at_ms, id)
  // of the last row of the most recently fetched page. NULL
  // means the next fetch is a fresh first page. Unlike OFFSET bookkeeping
  // this never replays or skips rows when `set_last_used` promotes entries
  // between page fetches — promoted rows simply land above the cursor.
  let activeHistoryCursor = $state<HistoryCursorPayload | null>(null);

  let activeHistoryExhausted = $state(false);

  const activeHistoryCount = $derived(
    deps.itemStore.history.filter((item) => !item.deleted).length,
  );

  const activeHistoryLimit = $derived(deps.settings.pageSizeLimit + deps.settings.loadTolerance);

  const activeHistoryHasMore = $derived(
    !activeHistoryExhausted && activeHistoryCount < activeHistoryLimit,
  );

  let activeHistoryRequestId = 0;

  // Keep stale in-flight recycle-bin pages from resurrecting rows that were
  // already restored or permanently removed locally.
  const deletedHistorySuppressedIds = new Set<string>();

  const SUPPRESSED_IDS_MAX = 500;

  function addSuppressedId(id: string) {
    if (deletedHistorySuppressedIds.size >= SUPPRESSED_IDS_MAX) {
      const first = deletedHistorySuppressedIds.values().next().value;
      if (first !== undefined) deletedHistorySuppressedIds.delete(first);
    }
    deletedHistorySuppressedIds.add(id);
  }

  // Deleting, restoring, or permanently removing a row changes the result
  // set behind the recycle-bin OFFSET. Reset the cursor before loading again
  // so a mutation in an earlier page cannot cause the next row to be skipped.
  function invalidateDeletedHistoryPagination() {
    deletedHistoryRequestId += 1;
    deletedHistoryLoading = false;
    deletedHistoryOffset = 0;
    deletedHistoryHasMore = true;
    void loadDeletedHistoryPage();
  }

  // Active history uses keyset (cursor) pagination anchored to the last row
  // of the most recent page. Out-of-band promotions (re-copy, reuse, sync
  // apply) raise a row above the cursor, so the rows below it neither shift
  // nor replay; any committed insertion, removal, soft-delete, or restore
  // still rebuilds from page zero instead of compensating with a fragile
  // local increment/decrement.
  function invalidateActiveHistoryPagination() {
    activeHistoryRequestId += 1;
    activeHistoryLoading = false;
    activeHistoryCursor = null;
    activeHistoryExhausted = false;
    void loadActiveHistoryPage();
  }

  function promoteFromCache(loadedIds: Set<string>) {
    deps.itemStore.current = promoteCachedEntries(deps.itemStore.current, loadedIds);
  }

  function trimLoadedItems() {
    deps.itemStore.current = trimLoadedHistory(deps.itemStore.current, {
      limit: deps.settings.pageSizeLimit,
      tolerance: deps.settings.loadTolerance,
    });
  }

  async function loadActiveHistoryPage(): Promise<void> {
    if (activeHistoryLoading || (activeHistoryCursor !== null && !activeHistoryHasMore)) return;

    if (!isTauriRuntime()) {
      activeHistoryExhausted = true;
      return;
    }

    activeHistoryLoading = true;
    const requestId = ++activeHistoryRequestId;
    const cursor = activeHistoryCursor;
    const isFirstPage = cursor === null;
    const requestedPageSize = Math.min(
      deps.settings.display.pageSize,
      isFirstPage ? activeHistoryLimit : activeHistoryLimit - activeHistoryCount,
    );
    try {
      const page = await loadClipboardHistory(requestedPageSize, 0, {
        ...deps.filter(),
        cursor,
      });
      if (requestId !== activeHistoryRequestId) return;
      if (page === null) {
        activeHistoryExhausted = true;
        return;
      }

      if (isFirstPage) {
        const deletedItems = deps.itemStore.history.filter((item) => item.deleted);
        const storedIds = new Set(page.map((item) => item.id));
        deps.itemStore.current = replaceViewItems(
          deps.itemStore.current,
          [...page, ...deletedItems.filter((item) => !storedIds.has(item.id))],
          "history",
        );
      } else {
        // Keyset pagination cannot replay rows the backend already served
        // below the cursor, but keep the guard anyway: an entry promoted and
        // then re-captured could theoretically round-trip its timestamps, and
        // the keyed each below must never see a duplicate key.
        const knownIds = new Set(deps.itemStore.current.historyIds);
        deps.itemStore.current = appendItems(
          deps.itemStore.current,
          page.filter((item) => !knownIds.has(item.id)),
          "history",
        );
      }
      // The anchor is the last row of the backend page — never the tail of
      // `items`, which mixes in recycled deleted entries and deduplicated rows.
      const anchor = page[page.length - 1];
      if (anchor) {
        activeHistoryCursor = {
          // Persisted rows initialize this timestamp at creation (schema v2).
          lastUsedAtMs: anchor.lastUsedAtMs!,
          id: anchor.id,
        };
      }
      activeHistoryExhausted = page.length < requestedPageSize;
      const loadedIds = new Set(page.map((item) => item.id));
      promoteFromCache(loadedIds);
      const beforeTrim = deps.itemStore.current;
      trimLoadedItems();
      if (beforeTrim !== deps.itemStore.current) {
        activeHistoryCursor = null;
        activeHistoryExhausted = false;
      }
    } catch (error) {
      if (requestId !== activeHistoryRequestId) return;
      console.error("Unable to load clipboard history", error);
      deps.onError("app.databaseLoadFailed");
    } finally {
      if (requestId === activeHistoryRequestId) activeHistoryLoading = false;
    }
  }

  async function loadDeletedHistoryPage(): Promise<void> {
    if (deletedHistoryLoading || !deletedHistoryHasMore) return;

    // The browser preview has no persisted recycle bin. Mark the page as
    // exhausted so selecting the filter remains a harmless local operation.
    if (!isTauriRuntime()) {
      deletedHistoryLoaded = true;
      deletedHistoryHasMore = false;
      return;
    }

    deletedHistoryLoading = true;
    const requestId = ++deletedHistoryRequestId;
    const offset = deletedHistoryOffset;
    try {
      const page = await loadDeletedClipboardHistory(DELETED_HISTORY_PAGE_SIZE, offset);
      if (requestId !== deletedHistoryRequestId) return;
      if (page === null) {
        deletedHistoryLoaded = true;
        deletedHistoryHasMore = false;
        return;
      }

      deps.itemStore.current = mergeDeletedHistoryPage(
        deps.itemStore.current,
        page,
        deletedHistorySuppressedIds,
      );
      deletedHistoryOffset += page.length;
      deletedHistoryLoaded = true;
      deletedHistoryHasMore = page.length === DELETED_HISTORY_PAGE_SIZE;
    } catch (error) {
      if (requestId !== deletedHistoryRequestId) return;
      console.error("Unable to load deleted clipboard history", error);
      deps.onError("app.databaseLoadFailed");
    } finally {
      if (requestId === deletedHistoryRequestId) deletedHistoryLoading = false;
    }
  }
  onDestroy(() => {
    activeHistoryRequestId++;
    deletedHistoryRequestId++;
  });
  return {
    addSuppressedId,
    invalidateDeletedHistoryPagination,
    invalidateActiveHistoryPagination,
    promoteFromCache,
    trimLoadedItems,
    loadActiveHistoryPage,
    loadDeletedHistoryPage,
    get deletedHistoryLoaded() {
      return deletedHistoryLoaded;
    },
    get deletedHistoryLoading() {
      return deletedHistoryLoading;
    },
    get deletedHistoryHasMore() {
      return deletedHistoryHasMore;
    },
    get activeHistoryLoading() {
      return activeHistoryLoading;
    },
    get activeHistoryHasMore() {
      return activeHistoryHasMore;
    },
    get deletedHistorySuppressedIds() {
      return deletedHistorySuppressedIds;
    },
  };
}
