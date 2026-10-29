import { onDestroy, untrack, tick } from "svelte";
import {
  createSearchPaintTracker,
  type SearchInteraction,
} from "$lib/utils/search/search-paint-latency";
import { recordSearchInteractionLatency } from "$lib/services/storage";
import { searchClipboardHistory } from "$lib/services/clipboard";
import { isTauriRuntime } from "$lib/services/runtime";
import { filterDemoSearchResults } from "$lib/utils/search/demo-search";
import type { ClipboardItem, HistoryFilterArgs, GeneralSettings } from "$lib/types/clipboard";
import type { ItemStoreView } from "$lib/utils/store/item-store-view.svelte";
import {
  appendItems,
  closeSearchResults,
  getItems,
  replaceViewItems,
  mergeSearchCachePage,
} from "$lib/utils/store/item-store";
export interface SearchDependencies {
  itemStore: ItemStoreView;
  readonly settings: GeneralSettings;
  readonly query: string;
  readonly isDeleted: boolean;
  filter(): HistoryFilterArgs;
  status: string;
  translate(key: string, params?: Record<string, string | number>): string;
  flushSettings(): Promise<void>;
  rememberSearchTerm(query: string): void;
  acceptRecords?(records: readonly ClipboardItem[]): void;
}
/** Owns debouncing, request generations and search paging without duplicating records. */
export function createSearchController(deps: SearchDependencies) {
  // The interaction is started by the search effect, not by a DOM input
  // handler: the query state update and this effect run before the search
  // box's `input` event is delivered, so a handler-started interaction would
  // never belong to the search it is supposed to measure.
  let activeInteraction: SearchInteraction | null = null;
  const paintTracker = createSearchPaintTracker((ms) => {
    // One sample per interaction: a re-run of the same query (history
    // invalidation) must not report the same measurement twice.
    activeInteraction = null;
    void recordSearchInteractionLatency(ms).catch(() => {});
  });
  let indexedQuery = $state("");

  let searchPending = $state(false);

  let searchRequestId = 0;

  // Bumped by `clipboard-history-invalidated` to re-run the search effect;
  // cancelling the in-flight request alone left the panel with no results.
  let searchEpoch = $state(0);

  let searchHasMore = $state(false);

  let searchLoading = $state(false);

  let searchOffset = $state(0);

  let searchLoadRequestId = 0;

  let pendingSearchHistoryQuery = "";

  // The query the current measurement belongs to.
  let measuredQuery = "";

  // --- Effects ---

  // Only an actual page-size or sort-rule change should restart the search.
  // Reading the whole settings store in the effect below would re-run
  // it (and reset pagination) on every unrelated settings change, because the
  // store emits a fresh object each time. This key is a primitive that only
  // changes when those two settings do.
  function invalidateSearchResults() {
    searchRequestId += 1;
    searchLoadRequestId += 1;
    searchHasMore = false;
    searchLoading = false;
    searchEpoch += 1;
  }

  const searchSettingsKey = $derived(
    `${deps.settings.display.searchPageSize}\u0000${deps.settings.searchPageSizeLimit}\u0000${JSON.stringify(deps.settings.searchSortRules)}`,
  );

  $effect(() => {
    const requestedQuery = deps.query.trim();
    // A changed query starts a new measurement. Re-runs for the same query
    // (history invalidation, settings change) keep the pending interaction so
    // the restarted search reports the same user-visible latency once.
    if (requestedQuery !== measuredQuery) {
      measuredQuery = requestedQuery;
      activeInteraction = paintTracker.input();
    }
    const interaction = activeInteraction;
    const requestedFilter = deps.filter();
    void searchSettingsKey; // dependency: narrow key, not the whole store
    const requestedMaxResults = untrack(() => deps.settings.searchPageSizeLimit);
    const requestedPageSize = untrack(() => deps.settings.display.searchPageSize);
    const requestedSortRules = untrack(() => deps.settings.searchSortRules);
    const requestedEpoch = searchEpoch;
    const requestId = ++searchRequestId;
    searchLoadRequestId += 1;
    searchLoading = false;
    searchHasMore = false;
    searchOffset = 0;

    if (!requestedQuery || deps.isDeleted) {
      deps.itemStore.current = closeSearchResults(deps.itemStore.current);
      indexedQuery = "";
      searchPending = false;
      return;
    }

    if (!isTauriRuntime()) {
      // Browser preview: there is no backend to query, so filter the demo
      // records client-side into the indexed view the list renders during a
      // search. The write is deferred like the desktop path because the effect
      // also reads the item store; writing it synchronously inside the
      // effect body self-invalidates the effect and freezes the query.
      searchPending = true;
      const timer = window.setTimeout(() => {
        if (requestId !== searchRequestId || requestedEpoch !== searchEpoch) return;
        const results = filterDemoSearchResults(
          getItems(deps.itemStore.current, "history"),
          requestedQuery,
          requestedFilter,
        );
        deps.itemStore.current = replaceViewItems(deps.itemStore.current, results, "indexed");
        indexedQuery = requestedQuery;
        searchOffset = results.length;
        searchHasMore = false;
        searchPending = false;
        deps.status = deps.translate("app.searchHitSummary", { count: results.length });
      }, 0);
      return () => window.clearTimeout(timer);
    }

    searchPending = true;
    const timer = window.setTimeout(() => {
      void deps
        .flushSettings()
        .then(() => {
          if (requestId !== searchRequestId || requestedEpoch !== searchEpoch) return null;
          return searchClipboardHistory(
            requestedQuery,
            requestedPageSize,
            0,
            requestedSortRules,
            requestedFilter,
          );
        })
        .then((page) => {
          if (requestId !== searchRequestId || requestedEpoch !== searchEpoch || page === null)
            return;
          // A first page replaces the previous query's results outright; an
          // empty page is still a displayed search, so the view stays on.
          deps.acceptRecords?.(page.items);
          deps.itemStore.current = replaceViewItems(deps.itemStore.current, page.items, "indexed");
          indexedQuery = requestedQuery;
          searchOffset = page.items.length;
          searchHasMore = searchOffset < Math.min(page.totalCount, requestedMaxResults);
          updateSearchCache(page.items);
          void tick().then(() => {
            paintTracker.painted(interaction, () => requestId === searchRequestId);
          });
          deps.status = page.truncated
            ? deps.translate("app.searchTruncated", {
                shown: page.items.length,
                total: page.totalCount,
              })
            : deps.translate("app.searchHitSummary", { count: page.items.length });
          if (deps.settings.searchHistoryEnabled && pendingSearchHistoryQuery === requestedQuery) {
            deps.rememberSearchTerm(requestedQuery);
            pendingSearchHistoryQuery = "";
          }
        })
        .catch((error) => {
          if (requestId !== searchRequestId) return;
          console.error("Unable to search clipboard history", error);
          deps.status = deps.translate("app.searchFailed");
        })
        .finally(() => {
          if (requestId === searchRequestId) searchPending = false;
        });
    }, 300);

    return () => window.clearTimeout(timer);
  });

  function updateSearchCache(results: ClipboardItem[]) {
    deps.itemStore.current = mergeSearchCachePage(deps.itemStore.current, {
      results,
      loadedIds: new Set(deps.itemStore.current.historyIds),
      policy: deps.settings.searchCacheEviction,
      max: deps.settings.searchCacheSize,
    });
  }

  async function loadSearchPage(): Promise<void> {
    if (searchLoading || !searchHasMore || !indexedQuery) return;

    if (!isTauriRuntime()) {
      searchHasMore = false;
      return;
    }

    searchLoading = true;
    const requestId = ++searchLoadRequestId;
    const offset = searchOffset;
    const maxResults = deps.settings.searchPageSizeLimit;
    try {
      const page = await searchClipboardHistory(
        indexedQuery,
        deps.settings.display.searchPageSize,
        offset,
        deps.settings.searchSortRules,
        deps.filter(),
      );
      if (requestId !== searchLoadRequestId) return;
      if (page === null || page.items.length === 0) {
        searchHasMore = false;
        return;
      }

      // OFFSET pagination can replay a row after an out-of-band insertion;
      // drop ids already loaded so the keyed each never sees a duplicate key.
      deps.acceptRecords?.(page.items);
      const knownIds = new Set(deps.itemStore.current.indexedIds ?? []);
      deps.itemStore.current = appendItems(
        deps.itemStore.current,
        page.items.filter((item) => !knownIds.has(item.id)),
        "indexed",
      );
      searchOffset += page.items.length;
      searchHasMore = searchOffset < Math.min(page.totalCount, maxResults);
      updateSearchCache(page.items);
    } catch (error) {
      if (requestId !== searchLoadRequestId) return;
      console.error("Unable to load more search results", error);
      deps.status = deps.translate("app.searchFailed");
    } finally {
      if (requestId === searchLoadRequestId) searchLoading = false;
    }
  }
  onDestroy(() => {
    paintTracker.dispose();
    searchRequestId++;
    searchLoadRequestId++;
  });
  return {
    invalidateSearchResults,
    loadSearchPage,
    updateSearchCache,
    get indexedQuery() {
      return indexedQuery;
    },
    set indexedQuery(value: typeof indexedQuery) {
      indexedQuery = value;
    },
    get searchPending() {
      return searchPending;
    },
    set searchPending(value: typeof searchPending) {
      searchPending = value;
    },
    get searchEpoch() {
      return searchEpoch;
    },
    set searchEpoch(value: typeof searchEpoch) {
      searchEpoch = value;
    },
    get searchHasMore() {
      return searchHasMore;
    },
    set searchHasMore(value: typeof searchHasMore) {
      searchHasMore = value;
    },
    get searchLoading() {
      return searchLoading;
    },
    set searchLoading(value: typeof searchLoading) {
      searchLoading = value;
    },
    get pendingSearchHistoryQuery() {
      return pendingSearchHistoryQuery;
    },
    set pendingSearchHistoryQuery(value: typeof pendingSearchHistoryQuery) {
      pendingSearchHistoryQuery = value;
    },
  };
}
