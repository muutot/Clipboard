<script lang="ts">
  import { flushSync, onMount, tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import BulkBar from "$lib/components/BulkBar.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import HistoryList from "$lib/components/HistoryList.svelte";
  import SearchHeader from "$lib/components/SearchHeader.svelte";
  import DetailPanel from "$lib/components/DetailPanel.svelte";
  import ImageFullscreenOverlay from "$lib/components/ImageFullscreenOverlay.svelte";
  import TagEditDialog from "$lib/components/TagEditDialog.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import { demoClipboardItems } from "$lib/data/demo-items";
  import {
    loadClipboardHistory,
    loadDeletedClipboardHistory,
    persistDelete,
    persistHardDelete,
    persistRestore,
    persistBatchRestore,
    persistPermanentDelete,
    persistBatchPermanentDelete,
    persistFavorite,
    persistBatchFavorite,
    persistBatchDelete,
    persistTags,
    listAllTags,
    searchClipboardHistory,
    listSourceApplications,
    materializeClipboardItem,
    toClipboardItem,
    copyClipboardItem,
    copyClipboardPath,
    pasteClipboardItem,
    deriveTextEditPatch,
    writeClipboardText,
    getDisplayTitle,
    getDisplayRemainingLines,
  } from "$lib/services/clipboard";
  import { getRuntimeInfo, isTauriRuntime } from "$lib/services/runtime";
  import { showToast } from "$lib/services/toast";
  import { getKeyboardConfig } from "$lib/services/keyboard";
  import { defaultShortcutsFor } from "$lib/keyboard-defaults";
  import type { ClipboardFilter, ClipboardItem, WindowPosition } from "$lib/types/clipboard";
  import type { IconName } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    createVirtualList,
    editHeight,
    buildPositions,
    type VirtualScrollConfig,
  } from "$lib/utils/virtual-scroll";
  import { estimateCardHeight } from "$lib/utils/card-height";
  import { parseDateQuery } from "$lib/utils/date-query";
  import { buildHistoryFilterArgs, filterHistoryItems } from "$lib/utils/history-filter";
  import {
    mergeSearchCachePage,
    promoteFromCache as promoteCachedEntries,
    trimLoadedItems as trimLoadedHistory,
  } from "$lib/utils/search-cache";
  import {
    resolveActionBindings,
    resolveFilterShortcutBindings,
    resolveNavigationBindings,
  } from "$lib/utils/shortcut-bindings";
  import { captureBulkSnapshot, planBulkDelete, setDeletedFlags } from "$lib/utils/bulk-actions";
  import { resolveKeyAction, type KeyAction } from "$lib/utils/keyboard-actions";
  import { resolveSearchInputAction } from "$lib/utils/search-input-actions";
  import {
    applyItemPatchesToCopies,
    findLoadedItemInCopies,
    mergeDeletedHistoryPage,
    removeItemTag,
    removeItemsFromCopies,
    replaceItemInCopies,
    rewriteItemTags,
  } from "$lib/utils/item-sync";
  import { isEditableKeyboardTarget } from "$lib/utils/keyboard";
  import {
    SEARCH_HISTORY_LIMIT,
    SEARCH_HISTORY_STORAGE_KEY,
    SEARCH_SUGGESTION_LIMIT,
    SEARCH_TERM_MAX_LENGTH,
    loadSearchHistory as loadStoredSearchHistory,
    nextSearchHistory,
    normalizeSearchTerm,
    persistSearchHistory as persistStoredSearchHistory,
    suggestionCandidate,
  } from "$lib/utils/search-history";
  import { createWindowBoundsController } from "$lib/utils/window-bounds";
  import {
    applyGeneralSettingsToDocument,
    applyFontSizesToDocument,
  } from "$lib/services/settings-bootstrap";
  import { listen } from "@tauri-apps/api/event";
  import type { PersistedClipboardItem, TagsChangedPayload } from "$lib/types/clipboard";
  import {
    generalSettings,
    restoreWindowPosition,
    saveWindowPosition,
  } from "$lib/services/settings";
  import { iconsDir } from "$lib/services/paths";
  import { getStorageStatus } from "$lib/services/storage";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  const dateFilterOptions = $derived([
    { id: "all" as const, label: _t("dateFilter.all") },
    { id: "today" as const, label: _t("dateFilter.today") },
    { id: "yesterday" as const, label: _t("dateFilter.yesterday") },
    { id: "week" as const, label: _t("dateFilter.week") },
    { id: "month" as const, label: _t("dateFilter.month") },
  ]);

  const VIRTUAL_SCROLL_CONFIG: VirtualScrollConfig = { itemHeight: 150, overscan: 5 };
  const VIRTUAL_SCROLL_THRESHOLD = 50;
  const DELETED_HISTORY_PAGE_SIZE = 100;

  type SearchOption = {
    value: string;
    kind: "history" | "suggestion";
  };

  type ClipboardHistoryInvalidation = {
    deletedIds: string[];
  };

  // Browser preview shows demo data; the Tauri runtime starts empty so real
  // history load failures can never be masked by fake entries.
  let items = $state<ClipboardItem[]>(
    isTauriRuntime() ? [] : demoClipboardItems.map((item) => ({ ...item })),
  );

  function updateItem(id: string, mutator: (item: ClipboardItem) => Partial<ClipboardItem>) {
    // Resolve across every copy: a search result can live only in
    // `indexedItems`/`searchCache` while the loaded history page is `items`.
    // Looking in `items` alone silently no-ops every action on such a row.
    const original = findLoadedItem(id);
    if (!original) return false;
    applyItemPatches(new Map([[id, mutator(original)]]));
    return true;
  }

  /// Fans one patch set out to every copy (items, indexedItems, searchCache,
  /// detailItem) in a single pass over each list. THE funnel for item
  /// mutations — direct per-copy mapping loops are how the searchCache drift
  /// happened. Pure logic lives in `utils/item-sync.ts` (covered by
  /// `item-sync.test.ts`); this wrapper only reassigns route state.
  function applyItemPatches(patches: ReadonlyMap<string, Partial<ClipboardItem>>) {
    if (patches.size === 0) return;
    const next = applyItemPatchesToCopies(
      { items, indexedItems, searchCache, detailItem },
      patches,
    );
    items = next.items;
    indexedItems = next.indexedItems;
    searchCache = next.searchCache;
    detailItem = next.detailItem;
  }

  /// Removal counterpart of [`applyItemPatches`]: drops ids from every copy
  /// (items, indexedItems, searchCache, detailItem) in one pass per list.
  /// Purging searchCache here closes the gap where a permanently deleted
  /// entry could later resurface from the spare-result cache.
  function removeItems(ids: ReadonlySet<string>) {
    if (ids.size === 0) return;
    const next = removeItemsFromCopies({ items, indexedItems, searchCache, detailItem }, ids);
    items = next.items;
    indexedItems = next.indexedItems;
    searchCache = next.searchCache;
    detailItem = next.detailItem;
  }

  function revertItem(id: string, fields: Partial<ClipboardItem>) {
    applyItemPatches(new Map([[id, fields]]));
  }

  function replaceMaterializedItem(updated: ClipboardItem): ClipboardItem {
    const next = replaceItemInCopies({ items, indexedItems, searchCache, detailItem }, updated);
    items = next.items;
    indexedItems = next.indexedItems;
    searchCache = next.searchCache;
    detailItem = next.detailItem;
    return updated;
  }

  function findLoadedItem(id: string): ClipboardItem | undefined {
    return findLoadedItemInCopies({ items, indexedItems, searchCache, detailItem }, id);
  }

  async function ensureItemMaterialized(item: ClipboardItem): Promise<ClipboardItem> {
    if (item.kind !== "image" && item.kind !== "file") return item;
    return replaceMaterializedItem(await materializeClipboardItem(item));
  }

  function prepareItemMaterialization(id: string) {
    const item = findLoadedItem(id);
    if (!item || (item.kind !== "image" && item.kind !== "file")) return;
    void ensureItemMaterialized(item).catch((error) => {
      console.error("Unable to prefetch remote clipboard resource", error);
    });
  }

  let deletedHistoryLoaded = $state(false);
  let deletedHistoryLoading = $state(false);
  let deletedHistoryOffset = $state(0);
  let deletedHistoryHasMore = $state(true);
  let deletedHistoryRequestId = 0;
  let activeHistoryLoading = $state(false);
  let activeHistoryOffset = $state(0);
  let activeHistoryHasMore = $state(true);
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
  let query = $state("");
  let activeFilter = $state<ClipboardFilter>("all");
  let selectedId = $state(isTauriRuntime() ? "" : (demoClipboardItems[0]?.id ?? ""));
  let currentTime = $state(Date.now());
  let runtimeLabel = $state(_t("app.browserPreview"));
  let statusMessage = $state(_t("app.activateHint"));
  let lastBackspaceAt = $state(0);
  let indexedItems = $state<ClipboardItem[] | null>(null);
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
  let searchHistory = $state<string[]>([]);
  let searchSuggestionsOpen = $state(false);
  let searchSuggestionIndex = $state(-1);
  let searchBlurTimer: number | undefined;
  let pendingSearchHistoryQuery = "";
  let searchCache = $state<ClipboardItem[]>([]);
  let searchCacheAccessOrder = $state<string[]>([]);

  let dateFilter = $state<string>("all");
  let sourceAppFilter = $state("");
  let tagFilter = $state<string | null>(null);
  let tagColors = $state<Record<string, string>>({});
  let tagEditDialog = $state<string | null>(null);
  let sourceApps = $state<string[]>([]);

  let detailItem = $state<ClipboardItem | null>(null);
  // Guards the async materialization in `openDetail`: a late result must not
  // reopen a panel the user already closed or replaced with another item.
  let detailRequestId = 0;

  let fullscreenFilePath = $state<string | null>(null);
  let fullscreenOpacity = $state(0.92);
  let fullscreenMode = $state<"overlay" | "desktop">("overlay");

  let selectedIds = $state<Set<string>>(new Set());
  let lastClickedIndex = $state(-1);

  let searchInputEl = $state<HTMLInputElement | null>(null);
  let historyListEl = $state<HTMLElement | null>(null);
  let appShellEl = $state<HTMLElement | null>(null);
  let scrollTop = $state(0);
  let containerHeight = $state(0);
  let containerWidth = $state(
    typeof window !== "undefined" ? Math.max(700, window.innerWidth - 20) : 700,
  );

  type MeasuredCardHeight = { height: number; signature: string };
  let measuredCardHeights = $state<Record<string, MeasuredCardHeight>>({});
  let heightRafId = 0;
  let pendingHeights = new Map<string, MeasuredCardHeight>();

  $effect(() => {
    // Prune measured heights against the displayed list, not `items` alone:
    // during a search the rendered cards come from `indexedItems`, and keying
    // off `items` deleted their measurements on any history change.
    const activeIds = new Set(filteredItems.map((i) => i.id));
    const current = untrack(() => measuredCardHeights);
    let changed = false;
    for (const key of Object.keys(current)) {
      if (!activeIds.has(key)) {
        delete current[key];
        changed = true;
      }
    }
    if (changed) measuredCardHeights = { ...current };
  });

  const hasDeletedItems = $derived(
    items.some((item) => !!item.deleted) ||
      (activeFilter === "deleted" && deletedHistoryLoaded && deletedHistoryHasMore),
  );

  const filters = $derived([
    { id: "all" as ClipboardFilter, label: _t("filter.all"), icon: "grid" as IconName },
    { id: "text" as ClipboardFilter, label: _t("filter.text"), icon: "text" as IconName },
    { id: "link" as ClipboardFilter, label: _t("filter.link"), icon: "link" as IconName },
    { id: "image" as ClipboardFilter, label: _t("filter.image"), icon: "image" as IconName },
    { id: "file" as ClipboardFilter, label: _t("filter.file"), icon: "file" as IconName },
    { id: "favorite" as ClipboardFilter, label: _t("filter.favorite"), icon: "star" as IconName },
    ...($generalSettings.useRecycleBin && hasDeletedItems
      ? [
          {
            id: "deleted" as ClipboardFilter,
            label: _t("filter.deleted"),
            icon: "trash" as IconName,
          },
        ]
      : []),
  ]);

  // Configured group-switch shortcuts (conf/keyboard.json), keyed by filter id.
  // Resolution lives in $lib/utils/shortcut-bindings: an absent action falls
  // back to its default (Alt+<N>); an action explicitly configured to empty
  // disables the shortcut.
  let keyboardShortcuts = $state<Record<string, string[]>>({});

  const filterShortcutBindings = $derived(
    resolveFilterShortcutBindings(
      keyboardShortcuts,
      filters.map((filter) => filter.id),
    ),
  );

  // Configured navigation shortcuts (conf/keyboard.json); same fallback and
  // disable rules as the group-switch bindings above.
  const navigationBindings = $derived(resolveNavigationBindings(keyboardShortcuts));

  // Float-panel shortcut; absent actions fall back to the canonical default,
  // empty disables it.
  const floatPanelBindings = $derived(
    resolveActionBindings(
      keyboardShortcuts,
      "toggleFloatPanel",
      defaultShortcutsFor("toggleFloatPanel"),
    ),
  );

  // Quick-paste shortcut; unbound by default, so it only fires when the
  // user explicitly binds it in the keyboard settings.
  const quickPasteBindings = $derived(
    resolveActionBindings(keyboardShortcuts, "quickPaste", defaultShortcutsFor("quickPaste")),
  );

  // Hide-window shortcut; absent actions fall back to the canonical default
  // (Escape), empty disables hiding from the main window.
  const hideWindowBindings = $derived(
    resolveActionBindings(keyboardShortcuts, "hideWindow", defaultShortcutsFor("hideWindow")),
  );

  // Item-action chords (copy/delete/favorite/tag/detail/save/select-all);
  // absent actions fall back to the canonical defaults, empty disables.
  // These must stay wired here: the keydown decision table matches against
  // them, and a binding edited in settings would otherwise silently do
  // nothing in the main window.
  const itemShortcutBindings = $derived({
    copyItem: resolveActionBindings(keyboardShortcuts, "copyItem", defaultShortcutsFor("copyItem")),
    deleteItem: resolveActionBindings(
      keyboardShortcuts,
      "deleteItem",
      defaultShortcutsFor("deleteItem"),
    ),
    favoriteItem: resolveActionBindings(
      keyboardShortcuts,
      "favoriteItem",
      defaultShortcutsFor("favoriteItem"),
    ),
    addTag: resolveActionBindings(keyboardShortcuts, "addTag", defaultShortcutsFor("addTag")),
    openDetail: resolveActionBindings(
      keyboardShortcuts,
      "openDetail",
      defaultShortcutsFor("openDetail"),
    ),
    downloadItem: resolveActionBindings(
      keyboardShortcuts,
      "downloadItem",
      defaultShortcutsFor("downloadItem"),
    ),
    selectAll: resolveActionBindings(
      keyboardShortcuts,
      "selectAll",
      defaultShortcutsFor("selectAll"),
    ),
  });

  // `quickCopy1..9` chords in position order for the decision table.
  const quickCopyShortcutBindings = $derived(
    Array.from({ length: 9 }, (_, index) =>
      resolveActionBindings(
        keyboardShortcuts,
        `quickCopy${index + 1}`,
        defaultShortcutsFor(`quickCopy${index + 1}`),
      ),
    ),
  );

  const focusSearchShortcutBindings = $derived(
    resolveActionBindings(keyboardShortcuts, "focusSearch", defaultShortcutsFor("focusSearch")),
  );

  // Main-toggle hint for the status bar; null hides the kbd chips when unbound.
  const toggleWindowHint = $derived(
    resolveActionBindings(
      keyboardShortcuts,
      "toggleWindow",
      defaultShortcutsFor("toggleWindow"),
    )[0] ?? null,
  );

  async function loadKeyboardShortcuts() {
    try {
      const config = await getKeyboardConfig();
      keyboardShortcuts = config?.shortcuts ?? {};
    } catch {
      keyboardShortcuts = {};
    }
  }

  // --- Date range resolution ---
  // resolveDateRange lives in $lib/utils/history-filter; the route only maps
  // the dropdown id onto backend filter args below.

  function loadSearchHistory(): string[] {
    return loadStoredSearchHistory(typeof window !== "undefined" ? window.localStorage : null);
  }

  function persistSearchHistory(history: string[]) {
    persistStoredSearchHistory(typeof window !== "undefined" ? window.localStorage : null, history);
  }

  function rememberSearchTerm(value: string) {
    if (!$generalSettings.searchHistoryEnabled) return;
    const next = nextSearchHistory(searchHistory, value);
    if (next.length === searchHistory.length && next.every((v, i) => v === searchHistory[i]))
      return;
    searchHistory = next;
    persistSearchHistory(next);
  }

  // --- Filtering ---

  const filteredItems = $derived(
    filterHistoryItems(
      { items, indexedItems, indexedQuery },
      {
        query,
        activeFilter,
        tagFilter,
        sourceAppFilter,
        dateFilter,
      },
    ),
  );

  const selectedIndex = $derived(filteredItems.findIndex((item) => item.id === selectedId));
  // Resolve every selected id across all four copies: a search result can live
  // only in `indexedItems`/`searchCache`, so counting against `items` alone
  // undercounts and makes bulk operations skip search-only rows.
  const selectedLoadedItems = $derived(
    [...selectedIds]
      .map((id) => findLoadedItem(id))
      .filter((item): item is ClipboardItem => item !== undefined),
  );
  const selectedDeletedCount = $derived(
    selectedLoadedItems.filter((item) => !!item.deleted).length,
  );
  const selectedActiveCount = $derived(selectedLoadedItems.filter((item) => !item.deleted).length);
  const allSelectedFavorites = $derived(
    selectedLoadedItems.length > 0 && selectedLoadedItems.every((item) => item.favorite),
  );
  const resultSummary = $derived(
    searchPending
      ? _t("status.searching")
      : _t("status.recordCount", { count: filteredItems.length }),
  );

  // --- Virtual scrolling ---

  const effectiveContainerWidth = $derived(Math.max(680, containerWidth));
  const cardTextHeight = $derived($generalSettings.cardTextHeight);
  const cardTallTextHeight = $derived($generalSettings.cardTallTextHeight);
  const cardImageHeight = $derived($generalSettings.cardImageHeight);
  const cardGap = $derived($generalSettings.cardGap);
  const cardPaddingTop = $derived($generalSettings.cardPaddingTop);
  const cardPaddingBottom = $derived($generalSettings.cardPaddingBottom);
  const searchHeight = $derived($generalSettings.searchHeight);
  const searchFontSize = $derived($generalSettings.searchFontSize);
  const cardBorderRadius = $derived($generalSettings.cardBorderRadius);
  const showSecondaryText = $derived($generalSettings.display.showSecondaryText);
  const maxTextLines = $derived($generalSettings.display.maxTextLines);
  const alwaysShowActions = $derived($generalSettings.cardActionsDisplay === "always");
  const quickCopyBadgeAlwaysVisible = $derived($generalSettings.quickCopyBadgeAlwaysVisible);
  const detailDisplayMode = $derived($generalSettings.detailDisplayMode);
  const doubleClickPaste = $derived($generalSettings.doubleClickPaste);

  function estimatedCardHeight(item: ClipboardItem): number {
    return estimateCardHeight(
      item,
      {
        imageHeight: cardImageHeight,
        textHeight: cardTextHeight,
        tallTextHeight: cardTallTextHeight,
        cardGap,
        cardPaddingTop,
        cardPaddingBottom,
        showSecondaryText,
        maxTextLines,
        previewFontSize: $generalSettings.fontSizes.cardPreview,
        contentWidth: effectiveContainerWidth,
      },
      detailDisplayMode === "split" && detailItem?.id === item.id,
    );
  }

  function cardHeightFor(item: ClipboardItem): number {
    return Math.max(0, estimatedCardHeight(item) - cardGap);
  }

  function cardLayoutSignaturePrefix(): string {
    return [
      containerWidth,
      cardTextHeight,
      cardTallTextHeight,
      cardImageHeight,
      cardGap,
      cardPaddingTop,
      cardPaddingBottom,
      showSecondaryText,
      maxTextLines,
      detailDisplayMode,
      $generalSettings.fontSizes.cardTitle,
      $generalSettings.fontSizes.cardPreview,
      $generalSettings.fontSizes.secondary,
    ].join(":");
  }

  const cardLayoutSignaturePrefixValue = $derived(cardLayoutSignaturePrefix());

  function cardLayoutSignature(item: ClipboardItem): string {
    const text = item.textContent || item.title;
    const logicalLineCount = text.replace(/\r\n?/g, "\n").split("\n").length;
    // Only the selected card's layout differs in split mode, so the detail
    // selection participates in THIS card's signature instead of the global
    // prefix — opening/closing the detail panel must not invalidate every
    // measured height and cause a list-wide layout jitter.
    const detailSelected =
      detailDisplayMode === "split" && detailItem?.id === item.id ? "detail" : "";
    return `${cardLayoutSignaturePrefixValue}:${editingId === item.id}:${item.id}:${item.kind}:${item.customTitle}:${text.length}:${logicalLineCount}:${item.title.length}:${item.preview.length}:${detailSelected}`;
  }

  function recordCardHeight(id: string, height: number) {
    const item = filteredItems.find((candidate) => candidate.id === id);
    if (!item || !Number.isFinite(height) || height <= 0) return;
    const signature = cardLayoutSignature(item);
    const previous = measuredCardHeights[id];
    if (previous?.signature === signature && previous.height === height) return;
    pendingHeights.set(id, { height, signature });
    if (heightRafId === 0) {
      heightRafId = requestAnimationFrame(() => {
        heightRafId = 0;
        const updates = pendingHeights;
        pendingHeights = new Map();
        let changed = false;
        for (const [updateId, { height: h, signature: s }] of updates) {
          const prev = measuredCardHeights[updateId];
          if (prev?.signature === s && prev.height === h) continue;
          measuredCardHeights[updateId] = { height: h, signature: s };
          changed = true;
        }
        if (changed) {
          measuredCardHeights = { ...measuredCardHeights };
        }
      });
    }
  }

  function virtualHeightFor(item: ClipboardItem): number {
    // Only reuse a measured height recorded under the same layout
    // signature; otherwise an edit-mode measurement keeps polluting the
    // normal layout after editing ends (and vice versa).
    const measured = measuredCardHeights[item.id];
    if (measured && measured.signature === cardLayoutSignature(item)) return measured.height;
    if (editingId === item.id) {
      return editHeight(
        (item.textContent || "").split("\n").length,
        !!item.customTitle,
        cardGap,
        cardPaddingTop,
        cardPaddingBottom,
      );
    }
    return estimatedCardHeight(item);
  }

  const virtualHeights = $derived(filteredItems.map(virtualHeightFor));

  const virtualPositions = $derived(
    buildPositions(virtualHeights, VIRTUAL_SCROLL_CONFIG.itemHeight),
  );

  const filteredItemIndexById = $derived.by(() => {
    const map = new Map<string, number>();
    for (let i = 0; i < filteredItems.length; i++) {
      map.set(filteredItems[i].id, i);
    }
    return map;
  });

  const virtualList = $derived(
    createVirtualList(
      filteredItems.length,
      containerHeight,
      scrollTop,
      VIRTUAL_SCROLL_CONFIG,
      virtualHeights,
      virtualPositions,
    ),
  );

  const useVirtualScroll = $derived(filteredItems.length > VIRTUAL_SCROLL_THRESHOLD);

  const visiblePageItems = $derived.by(() => {
    if (!useVirtualScroll) return filteredItems;
    return virtualList.visibleItems
      .map((v) => filteredItems[v.index])
      .filter((item): item is ClipboardItem => item !== undefined);
  });

  // --- Effects ---

  $effect(() => {
    const requestedQuery = query.trim();
    const requestedPageSize = $generalSettings.display.searchPageSize;
    const requestedSortRules = $generalSettings.searchSortRules;
    const requestedEpoch = searchEpoch;
    const requestId = ++searchRequestId;
    searchLoadRequestId += 1;
    searchLoading = false;
    searchHasMore = false;
    searchOffset = 0;

    if (!requestedQuery || activeFilter === "deleted") {
      indexedItems = null;
      indexedQuery = "";
      searchPending = false;
      return;
    }

    if (requestedQuery.length < 2) {
      indexedItems = null;
      indexedQuery = "";
      searchPending = false;
      return;
    }

    if (parseDateQuery(requestedQuery)) {
      indexedItems = null;
      indexedQuery = "";
      searchPending = false;
      return;
    }

    searchPending = true;
    const timer = window.setTimeout(() => {
      void searchClipboardHistory(requestedQuery, requestedPageSize, 0, requestedSortRules)
        .then((results) => {
          if (requestId !== searchRequestId || requestedEpoch !== searchEpoch || results === null)
            return;
          indexedItems = results;
          indexedQuery = requestedQuery;
          searchOffset = results.length;
          searchHasMore = results.length === requestedPageSize;
          updateSearchCache(results);
          statusMessage = _t("app.searchHitSummary", { count: results.length });
          if (
            $generalSettings.searchHistoryEnabled &&
            pendingSearchHistoryQuery === requestedQuery
          ) {
            rememberSearchTerm(requestedQuery);
            pendingSearchHistoryQuery = "";
          }
        })
        .catch((error) => {
          if (requestId !== searchRequestId) return;
          console.error("Unable to search clipboard history", error);
          statusMessage = _t("app.searchFailed");
        })
        .finally(() => {
          if (requestId === searchRequestId) searchPending = false;
        });
    }, 300);

    return () => window.clearTimeout(timer);
  });

  $effect(() => {
    if (filteredItems.length > 0 && selectedIndex === -1) {
      selectedId = filteredItems[0].id;
    }
  });

  // After filteredItems changes, prune invalid selectedIds. Rebuild the set
  // in one pass: removing ids one at a time would re-trigger this effect
  // once per invalid id and can exceed Svelte's effect update depth on
  // large bulk selections.
  $effect(() => {
    const idSet = new Set(filteredItems.map((i) => i.id));
    const pruned = new Set([...selectedIds].filter((id) => idSet.has(id)));
    if (pruned.size !== selectedIds.size) {
      selectedIds = pruned;
    }
  });

  $effect(() => {
    if (activeFilter === "deleted" && (!$generalSettings.useRecycleBin || !hasDeletedItems)) {
      activeFilter = "all";
      selectedIds = new Set();
    }
  });

  onMount(() => {
    searchHistory = loadSearchHistory();

    const clock = window.setInterval(() => {
      currentTime = Date.now();
    }, 30_000);

    void getStorageStatus()
      .then((status) => {
        if (status) {
          iconsDir.set(status.iconsDir);
        }
      })
      .catch((error) => {
        console.error("Unable to load storage status", error);
      });

    // Load history independently of the storage-status probe: a transient
    // status failure must not leave the whole list empty.
    void loadActiveHistoryPage()
      .then(() => {
        if (items.length === 0) return;
        selectedId = items[0]?.id ?? "";
      })
      .catch((error) => {
        console.error("Unable to load clipboard history", error);
        statusMessage = _t("app.databaseLoadFailed");
      });

    void getRuntimeInfo().then((runtime) => {
      if (runtime) {
        runtimeLabel = `${runtime.operatingSystem} / ${runtime.architecture} \u00b7 ${_t("app.coreConnected")}`;
      }
    });

    void loadKeyboardShortcuts();

    // Load one recycle-bin page during startup so the filter reflects the
    // persisted desktop state even before the user opens the deleted view.
    void loadDeletedHistoryPage();

    void listSourceApplications().then((apps) => {
      if (apps) sourceApps = apps;
    });

    refreshTagColors();

    const unlisten = listen<PersistedClipboardItem>("clipboard-item-added", (event) => {
      const record = event.payload;
      const newItem = toClipboardItem(record);
      const existingIdx = items.findIndex((i) => i.id === newItem.id);
      if (existingIdx >= 0) {
        // Re-copying an existing entry is de-duplicated on (kind, content_hash)
        // and keeps the same row id with a refreshed last_used_at_ms (the
        // original created_at_ms stays frozen), but the list order is driven
        // by array position rather than the timestamp. Promote it to the top
        // so the just-copied entry is visibly pinned.
        items.splice(existingIdx, 1);
        items = [newItem, ...items];
        selectedId = newItem.id;
        // The timestamp bump shifts every row behind the OFFSET cursor just
        // like a fresh insertion does. Rebuild the cursor so a later
        // scroll-load cannot replay an already-loaded row as a duplicate.
        invalidateActiveHistoryPagination();
      } else {
        items = [newItem, ...items];
        selectedId = newItem.id;
        invalidateActiveHistoryPagination();
      }
      if (indexedItems) {
        const indexedIdx = indexedItems.findIndex((i) => i.id === newItem.id);
        if (indexedIdx >= 0) {
          indexedItems.splice(indexedIdx, 1);
          indexedItems = [newItem, ...indexedItems];
        }
      }
      // A promoted entry may also sit in the spare search cache with its old
      // created_at_ms; patch it so the search panel does not show a stale
      // relative time until the next pagination-driven promoteFromCache.
      if (searchCache.some((i) => i.id === newItem.id)) {
        applyItemPatches(new Map([[newItem.id, newItem]]));
      }
      // The open detail panel holds its own object reference; refresh it so
      // a re-copied entry does not keep showing the previous timestamp or
      // resource paths.
      if (detailItem?.id === newItem.id) detailItem = newItem;
    });

    const unlistenHistoryInvalidated = listen<ClipboardHistoryInvalidation>(
      "clipboard-history-invalidated",
      (event) => {
        const removedIds = new Set(event.payload.deletedIds);

        for (const item of items) {
          if (item.deleted && removedIds.has(item.id)) addSuppressedId(item.id);
        }
        items = items.filter((item) => !removedIds.has(item.id));
        if (indexedItems) indexedItems = indexedItems.filter((item) => !removedIds.has(item.id));
        // The spare search cache is also a copy of live rows; evict removed
        // ids so a later promoteFromCache cannot resurrect deleted entries.
        if (searchCache.some((item) => removedIds.has(item.id))) {
          searchCache = searchCache.filter((item) => !removedIds.has(item.id));
        }
        // Re-run the search effect instead of only cancelling the in-flight
        // request; otherwise a search that lands during this event is dropped
        // and never retried.
        searchEpoch += 1;
        searchPending = false;
        selectedIds = new Set([...selectedIds].filter((id) => !removedIds.has(id)));
        if (removedIds.has(selectedId)) selectedId = items[0]?.id ?? "";
        if (detailItem && removedIds.has(detailItem.id)) detailItem = null;
        invalidateActiveHistoryPagination();
        invalidateDeletedHistoryPagination();
      },
    );

    const unlistenTrayOpenSettings = listen("tray-open-settings", () => {
      openSettings();
    });

    const unlistenTrayRestartBlocked = listen("tray-restart-blocked-in-dev", () => {
      showToast(_t("app.restartBlockedInDev"), "info");
    });

    const appWindow = isTauriRuntime() ? getCurrentWindow() : null;
    let previousRememberWindowPosition = false;

    const windowBounds = createWindowBoundsController({
      appWindow,
      isRemembered: () => $generalSettings.rememberWindowPosition,
      savePosition: saveWindowPosition,
      restorePosition: restoreWindowPosition,
      storage: typeof window !== "undefined" ? window.localStorage : null,
    });

    function applySettings(s: typeof $generalSettings) {
      applyGeneralSettingsToDocument(s);
      if (appWindow) {
        appWindow.setAlwaysOnTop(s.alwaysOnTop).catch(() => {});
        appWindow.setDecorations(s.useSystemTitleBar).catch(() => {});
        if (!s.rememberWindowPosition) {
          windowBounds.resetRestoreAttempt();
        } else if (!previousRememberWindowPosition) {
          void windowBounds.restore();
        }
      }
      previousRememberWindowPosition = s.rememberWindowPosition;
    }
    applySettings($generalSettings);
    const unsubSettings = generalSettings.subscribe((s) => applySettings(s));
    const unsubFontEvent = listen<{
      fontSizes: {
        base: number;
        secondary: number;
        tiny: number;
        cardTitle: number;
        cardPreview: number;
      };
      display: { showSecondaryText: boolean; maxTextLines: number };
    }>("settings-font-changed", (event) => {
      const { fontSizes, display } = event.payload;
      if (fontSizes) {
        applyFontSizesToDocument(fontSizes, display);
      }
    });

    const unsubTagsChanged = listen<TagsChangedPayload>("tags-changed", (event) => {
      const { renamed, deleted } = event.payload;
      if (!renamed && !deleted) return;
      // Compute per-entry patches across every copy's members, then fan them
      // out through the single funnel — including searchCache, which a
      // hand-rolled loop once missed.
      const transform = (entry: ClipboardItem) =>
        renamed ? rewriteItemTags(entry, renamed.old, renamed.new) : removeItemTag(entry, deleted!);
      const patches = new Map<string, Partial<ClipboardItem>>();
      for (const entry of [
        ...items,
        ...(indexedItems ?? []),
        ...searchCache,
        ...(detailItem ? [detailItem] : []),
      ]) {
        if (patches.has(entry.id)) continue;
        const rewritten = transform(entry);
        if (rewritten !== entry) patches.set(entry.id, { tags: rewritten.tags });
      }
      applyItemPatches(patches);
      if (renamed) {
        if (tagFilter === renamed.old) tagFilter = renamed.new;
      } else if (tagFilter === deleted) {
        tagFilter = null;
      }
      void refreshTagColors();
    });

    let listenersDisposed = false;
    let unlistenMove: (() => void) | undefined;
    let unlistenResize: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    if (appWindow) {
      appWindow
        .onFocusChanged(() => {
          void loadKeyboardShortcuts();
        })
        .then((fn) => {
          if (listenersDisposed) fn();
          else unlistenFocus = fn;
        })
        .catch(() => {});
      appWindow
        .onMoved(() => {
          windowBounds.scheduleSave();
        })
        .then((fn) => {
          if (listenersDisposed) fn();
          else unlistenMove = fn;
        })
        .catch(() => {});
      appWindow
        .onResized(() => {
          windowBounds.scheduleSave();
        })
        .then((fn) => {
          if (listenersDisposed) fn();
          else unlistenResize = fn;
        })
        .catch(() => {});
    }

    return () => {
      listenersDisposed = true;
      window.clearInterval(clock);
      void unlisten.then((fn) => fn()).catch(() => {});
      void unlistenHistoryInvalidated.then((fn) => fn()).catch(() => {});
      void unlistenTrayOpenSettings.then((fn) => fn()).catch(() => {});
      void unlistenTrayRestartBlocked.then((fn) => fn()).catch(() => {});
      void unsubFontEvent.then((fn) => fn()).catch(() => {});
      void unsubTagsChanged.then((fn) => fn()).catch(() => {});
      unsubSettings();
      if (unlistenMove) unlistenMove();
      if (unlistenResize) unlistenResize();
      if (unlistenFocus) unlistenFocus();
      if (heightRafId) cancelAnimationFrame(heightRafId);
      if (scrollRaf) cancelAnimationFrame(scrollRaf);
      if (searchBlurTimer !== undefined) window.clearTimeout(searchBlurTimer);
      pendingHeights.clear();
      void windowBounds.flush();
    };
  });

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

  // Active history uses SQLite OFFSET pagination. Any committed insertion,
  // removal, soft-delete, or restore shifts the rows behind the current
  // cursor, so rebuild the cursor from page zero instead of compensating with
  // a fragile local increment/decrement.
  function invalidateActiveHistoryPagination() {
    activeHistoryRequestId += 1;
    activeHistoryLoading = false;
    activeHistoryOffset = 0;
    activeHistoryHasMore = true;
    void loadActiveHistoryPage();
  }

  function updateSearchCache(results: ClipboardItem[]) {
    const next = mergeSearchCachePage(
      { cache: searchCache, accessOrder: searchCacheAccessOrder },
      {
        results,
        loadedIds: new Set(items.map((i) => i.id)),
        policy: $generalSettings.searchCacheEviction,
        max: $generalSettings.searchCacheSize,
      },
    );
    searchCacheAccessOrder = next.accessOrder;
    searchCache = next.cache;
  }

  function promoteFromCache(loadedIds: Set<string>) {
    const next = promoteCachedEntries(
      { cache: searchCache, accessOrder: searchCacheAccessOrder },
      loadedIds,
    );
    searchCache = next.cache;
    searchCacheAccessOrder = next.accessOrder;
  }

  function trimLoadedItems() {
    items = trimLoadedHistory(
      items,
      $generalSettings.pageSizeLimit,
      $generalSettings.loadTolerance,
    );
  }

  async function loadActiveHistoryPage(): Promise<void> {
    if (activeHistoryLoading || !activeHistoryHasMore) return;

    if (!isTauriRuntime()) {
      activeHistoryHasMore = false;
      return;
    }

    activeHistoryLoading = true;
    const requestId = ++activeHistoryRequestId;
    const offset = activeHistoryOffset;
    try {
      const page = await loadClipboardHistory(
        $generalSettings.display.pageSize,
        offset,
        buildHistoryFilterArgs({ activeFilter, tagFilter, sourceAppFilter, dateFilter }),
      );
      if (requestId !== activeHistoryRequestId) return;
      if (page === null) {
        activeHistoryHasMore = false;
        return;
      }

      if (offset === 0) {
        const deletedItems = items.filter((item) => item.deleted);
        const storedIds = new Set(page.map((item) => item.id));
        items = [...page, ...deletedItems.filter((item) => !storedIds.has(item.id))];
      } else {
        // OFFSET pagination can replay rows after any out-of-band insertion
        // (re-copy promotion, sync apply). Drop ids that are already loaded so
        // the keyed each below never sees a duplicate key.
        const knownIds = new Set(items.map((item) => item.id));
        const freshPage = page.filter((item) => !knownIds.has(item.id));
        items = [...items, ...freshPage];
      }
      activeHistoryOffset += page.length;
      activeHistoryHasMore = page.length === $generalSettings.display.pageSize;
      const loadedIds = new Set(page.map((item) => item.id));
      promoteFromCache(loadedIds);
      trimLoadedItems();
      // The trim cap is a sliding window over the newest rows: deeper OFFSET
      // pages contain the oldest rows and would be evicted right after
      // loading, so a full window must end pagination instead of fetching
      // pages that can never stay loaded (this previously looped
      // fetch-then-evict on every scroll event once the cap was reached).
      if (items.length >= $generalSettings.pageSizeLimit + $generalSettings.loadTolerance) {
        activeHistoryHasMore = false;
      }
    } catch (error) {
      if (requestId !== activeHistoryRequestId) return;
      console.error("Unable to load clipboard history", error);
      statusMessage = _t("app.databaseLoadFailed");
    } finally {
      if (requestId === activeHistoryRequestId) activeHistoryLoading = false;
    }
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
    try {
      const results = await searchClipboardHistory(
        indexedQuery,
        $generalSettings.display.searchPageSize,
        offset,
        $generalSettings.searchSortRules,
      );
      if (requestId !== searchLoadRequestId) return;
      if (results === null || results.length === 0) {
        searchHasMore = false;
        return;
      }

      // OFFSET pagination can replay a row after an out-of-band insertion;
      // drop ids already loaded so the keyed each never sees a duplicate key.
      const knownIds = new Set((indexedItems ?? []).map((item) => item.id));
      const freshResults = results.filter((item) => !knownIds.has(item.id));
      indexedItems = [...(indexedItems ?? []), ...freshResults];
      searchOffset += results.length;
      searchHasMore = results.length === $generalSettings.display.searchPageSize;
      updateSearchCache(results);
    } catch (error) {
      if (requestId !== searchLoadRequestId) return;
      console.error("Unable to load more search results", error);
      statusMessage = _t("app.searchFailed");
    } finally {
      if (requestId === searchLoadRequestId) searchLoading = false;
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

      items = mergeDeletedHistoryPage(items, page, deletedHistorySuppressedIds);
      deletedHistoryOffset += page.length;
      deletedHistoryLoaded = true;
      deletedHistoryHasMore = page.length === DELETED_HISTORY_PAGE_SIZE;
    } catch (error) {
      if (requestId !== deletedHistoryRequestId) return;
      console.error("Unable to load deleted clipboard history", error);
      statusMessage = _t("app.databaseLoadFailed");
    } finally {
      if (requestId === deletedHistoryRequestId) deletedHistoryLoading = false;
    }
  }

  // --- Handlers ---

  function commitSearchQuery(value = query) {
    const term = normalizeSearchTerm(value);
    searchSuggestionsOpen = false;
    searchSuggestionIndex = -1;

    if (!term) {
      pendingSearchHistoryQuery = "";
      return;
    }

    pendingSearchHistoryQuery = $generalSettings.searchHistoryEnabled ? term : "";

    if (
      !isTauriRuntime() ||
      activeFilter === "deleted" ||
      parseDateQuery(term) ||
      (indexedItems !== null && indexedQuery === term)
    ) {
      if ($generalSettings.searchHistoryEnabled) rememberSearchTerm(term);
      pendingSearchHistoryQuery = "";
    }
  }

  function chooseSearchOption(value: string) {
    const term = normalizeSearchTerm(value);
    if (!term) return;
    query = term;
    searchSuggestionIndex = -1;
    searchSuggestionsOpen = false;
    commitSearchQuery(term);
    searchInputEl?.focus();
  }

  function clearSearchQuery() {
    query = "";
    pendingSearchHistoryQuery = "";
    searchSuggestionIndex = -1;
    searchSuggestionsOpen = true;
    searchInputEl?.focus();
  }

  function handleSearchInputKeydown(event: KeyboardEvent) {
    const input = searchInputEl;
    const resolved = resolveSearchInputAction(event, {
      now: Date.now(),
      lastBackspaceAt,
      query,
      suggestionsOpen: searchSuggestionsOpen,
      optionCount: searchOptions.length,
      activeOption: activeSearchOption?.value ?? null,
      inlineSuggestion: inlineSearchSuggestion?.value ?? null,
      caretAtEnd:
        input !== null &&
        input.selectionStart === input.selectionEnd &&
        input.selectionEnd === query.length,
    });
    lastBackspaceAt = resolved.backspaceAt;
    if (resolved.prevent) event.preventDefault();
    if (resolved.stop) event.stopPropagation();
    switch (resolved.action.type) {
      case "none":
        break;
      case "clear-query":
        query = "";
        pendingSearchHistoryQuery = "";
        searchSuggestionIndex = -1;
        break;
      case "accept-inline":
        query = resolved.action.value;
        pendingSearchHistoryQuery = "";
        searchSuggestionsOpen = false;
        searchSuggestionIndex = -1;
        void tick().then(() => {
          const el = searchInputEl;
          if (!el) return;
          el.focus();
          el.setSelectionRange(query.length, query.length);
        });
        break;
      case "move-index": {
        const count = searchOptions.length;
        if (count === 0) {
          searchSuggestionIndex = -1;
          break;
        }
        // From "nothing selected" (-1), ArrowUp must land on the last option
        // and ArrowDown on the first; the plain modulo skipped the last one.
        searchSuggestionIndex =
          searchSuggestionIndex < 0
            ? resolved.action.delta < 0
              ? count - 1
              : 0
            : (searchSuggestionIndex + resolved.action.delta + count) % count;
        break;
      }
      case "commit-query":
        commitSearchQuery();
        break;
      case "choose-option":
        chooseSearchOption(resolved.action.value);
        break;
      case "close-suggestions":
        searchSuggestionsOpen = false;
        searchSuggestionIndex = -1;
        query = "";
        pendingSearchHistoryQuery = "";
        break;
    }
  }

  function handleSearchInputBlur() {
    if (searchBlurTimer !== undefined) window.clearTimeout(searchBlurTimer);
    searchBlurTimer = window.setTimeout(() => {
      searchBlurTimer = undefined;
      if (document.activeElement !== searchInputEl) {
        searchSuggestionsOpen = false;
        searchSuggestionIndex = -1;
      }
    }, 0);
  }
  let settingsWindowOpening = $state(false);
  let floatWindowOpening = $state(false);

  async function toggleFloatPanel() {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (floatWindowOpening) return;
    floatWindowOpening = true;
    try {
      await invoke("toggle_float_panel");
    } catch (error) {
      console.error("Unable to toggle float panel", error);
      showToast(_t("float.toggleFailed"), "error");
    } finally {
      floatWindowOpening = false;
    }
  }
  async function openSettings() {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (settingsWindowOpening) return;
    settingsWindowOpening = true;
    try {
      const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
      const existing = await WebviewWindow.getByLabel("settings");
      if (existing) {
        existing.setFocus();
        return;
      }
      const settingsBg =
        getComputedStyle(document.documentElement).getPropertyValue("--bg-settings").trim() ||
        "#1b1b1b";
      const settingsWindow = new WebviewWindow("settings", {
        url: "/settings",
        title: "Settings",
        width: 760,
        height: 640,
        minWidth: 730,
        minHeight: 480,
        center: true,
        resizable: true,
        decorations: false,
        focus: true,
        backgroundColor: settingsBg,
      });
    } finally {
      settingsWindowOpening = false;
    }
  }

  function setFilter(filter: ClipboardFilter) {
    if (filter === "deleted" && (!$generalSettings.useRecycleBin || !hasDeletedItems)) return;
    const enteringDeleted = filter === "deleted";
    if (activeFilter !== filter) resetHistoryScroll();
    activeFilter = filter;
    selectedIds = new Set();
    indexedItems = null;
    indexedQuery = "";
    if (enteringDeleted) {
      if (!deletedHistoryLoaded) {
        void loadDeletedHistoryPage();
      }
    } else {
      void invalidateActiveHistoryPagination();
    }
  }

  function selectItem(id: string, event?: MouseEvent) {
    if (event && (event.ctrlKey || event.metaKey)) {
      toggleSelectItem(id);
      return;
    }

    if (event && event.shiftKey && lastClickedIndex >= 0) {
      const currentIdx = filteredItems.findIndex((i) => i.id === id);
      if (currentIdx < 0) {
        // The card left the filtered set between paint and click; indexing -1
        // would throw, so fall back to a plain selection.
        selectedId = id;
        lastClickedIndex = -1;
        return;
      }
      const start = Math.min(lastClickedIndex, currentIdx);
      const end = Math.max(lastClickedIndex, currentIdx);
      const rangeIds = new Set<string>();
      for (let i = start; i <= end; i++) {
        rangeIds.add(filteredItems[i].id);
      }
      selectedIds = new Set(rangeIds);
      return;
    }

    selectedId = id;
    lastClickedIndex = filteredItems.findIndex((i) => i.id === id);
  }

  function toggleSelectItem(id: string) {
    const next = new Set(selectedIds);
    if (next.has(id)) {
      next.delete(id);
      if (selectedId === id) selectedId = "";
    } else {
      next.add(id);
    }
    selectedIds = next;
    lastClickedIndex = filteredItems.findIndex((i) => i.id === id);
  }

  function toggleFavorite(id: string) {
    const original = filteredItems.find((item) => item.id === id);
    if (!original) return;

    const nextFavorite = !original.favorite;
    updateItem(id, () => ({ favorite: nextFavorite }));

    void persistFavorite(id, nextFavorite)
      .then((updated) => {
        if (updated === false) throw new Error("record not found");
        showToast(
          nextFavorite ? _t("toast.favoriteSuccess") : _t("toast.unfavoriteSuccess"),
          "success",
        );
      })
      .catch((error) => {
        console.error("Unable to update favorite", error);
        revertItem(id, { favorite: original.favorite });
        statusMessage = _t("app.favoriteFailed");
        showToast(_t("app.favoriteFailed"), "error");
      });
  }

  /// Re-adds an optimistically removed row after a failed hard/permanent
  /// delete without replacing the whole array, so rows captured during the
  /// request survive.
  function reinsertItem(item: ClipboardItem, wasSelected: boolean) {
    if (!items.some((entry) => entry.id === item.id)) {
      items = [item, ...items];
    }
    if (indexedItems && !indexedItems.some((entry) => entry.id === item.id)) {
      indexedItems = [item, ...indexedItems];
    }
    if (wasSelected) selectedIds = new Set([...selectedIds, item.id]);
  }

  function deleteItem(id: string) {
    const item = findLoadedItem(id);
    if (item?.deleted) {
      permanentlyDeleteItem(id);
      return;
    }
    if (!$generalSettings.useRecycleBin) {
      hardDeleteItem(id);
      return;
    }

    const wasSelected = selectedIds.has(id);
    deletedHistorySuppressedIds.delete(id);
    updateItem(id, () => ({ deleted: true }));
    selectedIds = new Set([...selectedIds].filter((x) => x !== id));

    void persistDelete(id)
      .then((removed) => {
        if (removed === false) throw new Error("record not found");
        invalidateActiveHistoryPagination();
        invalidateDeletedHistoryPagination();
        showToast(_t("toast.deleteSuccess"), "success");
      })
      .catch((error) => {
        console.error("Unable to delete clipboard item", error);
        // Undo only this row; restoring a whole-array snapshot would discard
        // items captured while the delete was in flight.
        revertItem(id, { deleted: false });
        if (wasSelected) selectedIds = new Set([...selectedIds, id]);
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function permanentlyDeleteItem(id: string) {
    const target = findLoadedItem(id);
    if (!target) return;

    const wasSelected = selectedIds.has(id);
    const previousDetailItem = detailItem;
    const previousSearchCache = searchCache;
    addSuppressedId(id);
    // Route through the shared funnel so `detailItem`/`searchCache` cannot keep
    // rendering a row that no longer exists.
    removeItems(new Set([id]));
    selectedIds = new Set([...selectedIds].filter((x) => x !== id));

    void persistPermanentDelete(id)
      .then((removed) => {
        if (removed === false) throw new Error("record not found");
        invalidateDeletedHistoryPagination();
        showToast(_t("toast.deleteSuccess"), "success");
      })
      .catch((error) => {
        console.error("Unable to permanently delete clipboard item", error);
        deletedHistorySuppressedIds.delete(id);
        reinsertItem(target, wasSelected);
        detailItem = previousDetailItem;
        searchCache = previousSearchCache;
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  // Direct deletion remains a separate backend path: the recycle-bin
  // permanent-delete command intentionally accepts only already deleted
  // rows, while this path handles active rows when the feature is disabled.
  function hardDeleteItem(id: string) {
    const target = findLoadedItem(id);
    if (!target) return;

    const wasSelected = selectedIds.has(id);
    const previousDetailItem = detailItem;
    const previousSearchCache = searchCache;
    // Route through the shared funnel so `detailItem`/`searchCache` cannot keep
    // rendering a row that no longer exists.
    removeItems(new Set([id]));
    selectedIds = new Set([...selectedIds].filter((x) => x !== id));

    void persistHardDelete(id)
      .then((removed) => {
        if (removed === false) throw new Error("record not found");
        invalidateActiveHistoryPagination();
        showToast(_t("toast.deleteSuccess"), "success");
      })
      .catch((error) => {
        console.error("Unable to delete clipboard item", error);
        reinsertItem(target, wasSelected);
        detailItem = previousDetailItem;
        searchCache = previousSearchCache;
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function restoreItem(id: string) {
    const target = findLoadedItem(id);
    if (!target?.deleted) return;

    addSuppressedId(id);
    updateItem(id, () => ({ deleted: false }));
    void persistRestore(id)
      .then((restored) => {
        if (restored === false) throw new Error("record not found");
        invalidateActiveHistoryPagination();
        invalidateDeletedHistoryPagination();
        showToast(_t("toast.resumed"), "success");
      })
      .catch((error) => {
        console.error("Unable to restore clipboard item", error);
        deletedHistorySuppressedIds.delete(id);
        revertItem(id, { deleted: true });
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function moveToTop(id: string) {
    const promote = (list: ClipboardItem[]): ClipboardItem[] => {
      const idx = list.findIndex((i) => i.id === id);
      if (idx <= 0) return list;
      const next = [...list];
      const [item] = next.splice(idx, 1);
      return [item, ...next];
    };
    items = promote(items);
    // During a search the visible list is `indexedItems`, so the copied row
    // must be promoted there too or "pin copied to top" silently no-ops.
    if (indexedItems) indexedItems = promote(indexedItems);
    // The spare search cache keeps its own order; promote there as well so
    // a later promoteFromCache does not restore the pre-copy position.
    searchCache = promote(searchCache);
  }

  async function copyItem(id: string) {
    const item = findLoadedItem(id);
    if (!item) return;
    await copyClipboardItem(item, {
      moveToTop: $generalSettings.pinCopiedToTop ? (mid) => moveToTop(mid) : undefined,
      onstatus: (message) => (statusMessage = message),
    });
  }

  async function copyPath(id: string) {
    const item = findLoadedItem(id);
    if (!item) return;
    await copyClipboardPath(item, {
      onstatus: (message) => (statusMessage = message),
    });
  }

  async function openDetail(id: string) {
    const item = findLoadedItem(id);
    if (!item) return;
    const requestId = ++detailRequestId;
    detailItem = item;
    if (item.kind === "image" || item.kind === "file") {
      try {
        const materialized = await ensureItemMaterialized(item);
        if (requestId === detailRequestId && detailItem?.id === id) {
          detailItem = materialized;
        }
      } catch (error) {
        console.error("Unable to materialize clipboard item for detail", error);
      }
    }
  }

  async function handleImageFullscreen(id: string) {
    let item = findLoadedItem(id);
    if (!item) return;
    const needsMaterialization = !item.resourcePath && !item.previewPath;
    if (needsMaterialization) {
      try {
        item = await ensureItemMaterialized(item);
      } catch (error) {
        console.error("Unable to materialize image for fullscreen", error);
        showToast(_t("toast.copyFailed"), "error");
        return;
      }
    }
    const filePath = item?.resourcePath || item?.previewPath;
    if (!filePath) return;

    if ($generalSettings.imageFullscreenMode === "desktop") {
      fullscreenMode = "desktop";
      fullscreenFilePath = filePath;
      flushSync();
      return;
    }

    fullscreenMode = "overlay";
    fullscreenFilePath = filePath;
    fullscreenOpacity = $generalSettings.viewerBackdropOpacity / 100;
  }

  function closeFullscreen() {
    fullscreenFilePath = null;
  }

  function closeDetail() {
    detailRequestId += 1;
    detailItem = null;
    void tick().then(() => {
      const el = document.querySelector(`[data-id="${selectedId}"]`);
      if (el instanceof HTMLElement) {
        el.focus();
      }
    });
  }

  let editingId = $state<string | null>(null);

  function startEdit(id: string) {
    editingId = id;
  }

  async function saveEdit(id: string, content: string): Promise<boolean> {
    const item = findLoadedItem(id);
    if (!item) return false;

    const { isMedia, isText, newTitle, newTextContent, newPreview, newSizeBytes, newSizeLabel } =
      deriveTextEditPatch(item, content);

    if (isMedia && content) {
      try {
        const updated = await invoke<ClipboardItem>("rename_item", { id, newName: content });
        updateItem(id, () => ({
          title: updated.title,
          resourcePath: updated.resourcePath,
          previewPath: updated.previewPath,
        }));
        editingId = null;
        showToast(_t("toast.editSaved"), "success");
        return true;
      } catch (e) {
        statusMessage = _t("toast.saveFailed");
        showToast(String(e), "error");
        return false;
      }
    }

    if (!isText) return false;

    if (isTauriRuntime()) {
      try {
        const saved = await invoke<boolean>("update_clipboard_text", {
          id,
          newTitle,
          newTextContent,
        });
        if (saved === false) throw new Error("record not found");
      } catch (error) {
        statusMessage = _t("toast.saveFailed");
        showToast(_t("toast.saveFailed"), "error");
        console.error("Unable to save clipboard text", error);
        return false;
      }
    }

    updateItem(id, (item) => ({
      textContent: newTextContent,
      preview: newPreview,
      sizeBytes: newSizeBytes,
      sizeLabel: newSizeLabel,
      ...(item.customTitle ? {} : { title: newTitle }),
    }));
    editingId = null;
    showToast(_t("toast.editSaved"), "success");
    return true;
  }

  function cancelEdit(_id: string) {
    editingId = null;
  }

  function renameTitle(id: string, title: string) {
    const item = findLoadedItem(id);
    if (!item) return;
    const previousTitle = item.title;
    const previousCustomTitle = item.customTitle;
    updateItem(id, () => ({ title, customTitle: true }));
    invoke("rename_item", { id, newName: title }).catch((err) => {
      console.error("Rename item failed:", err);
      revertItem(id, { title: previousTitle, customTitle: previousCustomTitle });
      showToast(_t("toast.saveFailed"), "error");
    });
  }

  function toggleTagFilter(tag: string) {
    tagFilter = tagFilter === tag ? null : tag;
    resetHistoryScroll();
    void invalidateActiveHistoryPagination();
  }

  function openTagEdit(tag: string) {
    tagEditDialog = tag;
  }

  function resetHistoryScroll() {
    scrollTop = 0;
    if (historyListEl) historyListEl.scrollTop = 0;
  }

  async function refreshTagColors() {
    const tags = await listAllTags();
    if (!tags) return;
    const map: Record<string, string> = {};
    for (const tag of tags) {
      if (tag.color) map[tag.name] = tag.color;
    }
    tagColors = map;
  }

  async function saveTags(id: string, tags: string[]) {
    const item = findLoadedItem(id);
    if (!item) return;
    const previousTags = item.tags ?? [];
    const deduped = [...new Set(tags.map((t) => t.trim()).filter(Boolean))];
    // updateItem fans the patch out to items, indexedItems, searchCache, and
    // detailItem so no copy of the entry keeps stale tags.
    updateItem(id, () => ({ tags: deduped }));
    try {
      const ok = await persistTags(id, deduped);
      if (ok === false) throw new Error("record not found");
    } catch (error) {
      console.error("Unable to save tags", error);
      revertItem(id, { tags: previousTags });
      showToast(_t("toast.saveFailed"), "error");
      return;
    }
    refreshTagColors();
  }

  async function plainPaste(_id: string) {
    const item = findLoadedItem(_id);
    if (!item) return;
    await pasteClipboardItem(item, "plain", {
      moveToTop: (mid) => moveToTop(mid),
    });
  }

  async function formatPaste(_id: string) {
    const item = findLoadedItem(_id);
    if (!item || !item.htmlContent) return;
    await pasteClipboardItem(item, "format", {
      moveToTop: (mid) => moveToTop(mid),
    });
  }

  async function cleanPaste(_id: string) {
    const item = findLoadedItem(_id);
    if (!item) return;
    await pasteClipboardItem(item, "clean", {
      moveToTop: (mid) => moveToTop(mid),
    });
  }

  async function doubleClickPasteItem(id: string) {
    const item = findLoadedItem(id);
    if (!item) return;
    await pasteClipboardItem(item, "auto", {
      moveToTop: (mid) => moveToTop(mid),
    });
  }

  function duplicateItem(id: string) {
    invoke("duplicate_clipboard_item", { id })
      .then(() => {
        showToast(_t("toast.duplicateSuccess"), "success");
      })
      .catch(() => {
        showToast(_t("toast.saveFailed"), "error");
      });
  }

  async function saveAsNew(id: string, title: string, content: string) {
    editingId = null;
    try {
      const newId: string = await invoke("duplicate_clipboard_item", { id });
      await invoke("update_clipboard_text", {
        id: newId,
        newTitle: title,
        newTextContent: content,
      });
      showToast(_t("toast.duplicateSuccess"), "success");
    } catch {
      showToast(_t("toast.saveFailed"), "error");
    }
  }

  function copyFilename(_id: string) {
    const item = findLoadedItem(_id);
    if (!item) return;
    const name = item.fileName ?? item.title;
    void writeClipboardText(name)
      .then(() => {
        showToast(_t("toast.copySuccess"), "success");
      })
      .catch(() => {
        showToast(_t("toast.copyFailed"), "error");
      });
  }

  async function saveItem(id: string) {
    let item = findLoadedItem(id);
    if (!item) return;
    try {
      item = await ensureItemMaterialized(item);
    } catch (error) {
      console.error("Unable to materialize file for save", error);
      showToast(_t("toast.saveFailed"), "error");
      return;
    }
    if (!item.resourcePath) return;
    if (!isTauriRuntime()) return;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const defaultName = item.fileName || item.title.split(/[\\/]/).pop() || "file";
      const ext = defaultName.includes(".") ? defaultName.split(".").pop() : "";
      const filters = ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : [];
      const filePath = await save({ defaultPath: defaultName, filters });
      if (filePath) {
        await invoke("save_clipboard_item_file", { id, dst: filePath });
        showToast(_t("card.saveAs"), "success");
      }
    } catch (error) {
      console.error("Unable to save file", error);
      statusMessage = _t("toast.saveFailed");
      showToast(_t("toast.saveFailed"), "error");
    }
  }

  // --- Bulk operations ---

  function bulkCopy() {
    const selectedItems = selectedLoadedItems;
    // Text/link rows carry the full content in `textContent` while `title`
    // is only the first line; copying titles silently drops content.
    // Media rows keep `title` (their `textContent` is null or an internal
    // multi-file JSON list, never user-facing text).
    const text = selectedItems
      .map((i) => (i.kind === "text" || i.kind === "link" ? i.textContent || i.title : i.title))
      .join("\n");
    void writeClipboardText(text)
      .then(() => {
        showToast(_t("toast.bulkCopySuccess", { count: selectedItems.length }), "success");
      })
      .catch(() => {
        showToast(_t("toast.copyFailed"), "error");
      });
  }

  function bulkFavorite() {
    const ids = [...selectedIds];
    const unfavorite = allSelectedFavorites;

    const patch = new Map<string, Partial<ClipboardItem>>();
    for (const id of ids) patch.set(id, { favorite: !unfavorite });
    applyItemPatches(patch);

    void persistBatchFavorite(ids, !unfavorite)
      .then((updated) => {
        if (updated === false) throw new Error("batch favorite failed");
        showToast(
          unfavorite
            ? _t("toast.bulkUnfavoriteSuccess", { count: ids.length })
            : _t("toast.bulkFavoriteSuccess", { count: ids.length }),
          "success",
        );
        selectedIds = new Set();
      })
      .catch((error) => {
        console.error("Bulk favorite failed", error);
        // Revert per-item through the shared funnel instead of restoring a
        // whole-array snapshot: entries spliced in by clipboard-item-added
        // during the async window must survive the rollback.
        applyItemPatches(new Map(ids.map((id) => [id, { favorite: unfavorite }])));
        statusMessage = _t("app.favoriteFailed");
        showToast(_t("app.favoriteFailed"), "error");
      });
  }

  function bulkRestore() {
    const ids = selectedLoadedItems.filter((item) => item.deleted).map((item) => item.id);
    if (ids.length === 0) return;

    const previousItems = captureBulkSnapshot({
      items,
      indexedItems,
      searchCache,
      selectedIds,
      detailItem,
    });
    const idSet = new Set(ids);
    for (const id of ids) addSuppressedId(id);
    items = setDeletedFlags(items, idSet, false);
    if (indexedItems) {
      indexedItems = setDeletedFlags(indexedItems, idSet, false);
    }
    searchCache = setDeletedFlags(searchCache, idSet, false);
    if (detailItem && idSet.has(detailItem.id)) {
      detailItem = { ...detailItem, deleted: false };
    }
    selectedIds = new Set([...selectedIds].filter((id) => !idSet.has(id)));

    void persistBatchRestore(ids)
      .then((restored) => {
        if (restored === false) throw new Error("batch restore failed");
        invalidateActiveHistoryPagination();
        invalidateDeletedHistoryPagination();
        showToast(_t("toast.restoreSuccess", { count: ids.length }), "success");
      })
      .catch((error) => {
        console.error("Bulk restore failed", error);
        for (const id of ids) deletedHistorySuppressedIds.delete(id);
        items = previousItems.items;
        indexedItems = previousItems.indexedItems;
        searchCache = previousItems.searchCache;
        selectedIds = previousItems.selectedIds;
        detailItem = previousItems.detailItem;
        statusMessage = _t("app.deleteFailed");
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function bulkPermanentDelete() {
    const ids = selectedLoadedItems.filter((item) => item.deleted).map((item) => item.id);
    if (ids.length === 0) return;

    const previous = captureBulkSnapshot({
      items,
      indexedItems,
      searchCache,
      selectedIds,
      detailItem,
    });
    const idSet = new Set(ids);
    for (const id of ids) addSuppressedId(id);
    removeItems(idSet);
    selectedIds = new Set([...selectedIds].filter((id) => !idSet.has(id)));

    void persistBatchPermanentDelete(ids)
      .then((removed) => {
        if (removed === false) throw new Error("batch permanent delete failed");
        invalidateDeletedHistoryPagination();
        showToast(_t("toast.bulkDeleteSuccess", { count: ids.length }), "success");
      })
      .catch((error) => {
        console.error("Bulk permanent delete failed", error);
        for (const id of ids) deletedHistorySuppressedIds.delete(id);
        items = previous.items;
        indexedItems = previous.indexedItems;
        searchCache = previous.searchCache;
        selectedIds = previous.selectedIds;
        detailItem = previous.detailItem;
        statusMessage = _t("app.deleteFailed");
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function bulkDelete() {
    const selectedItems = selectedLoadedItems;
    if (selectedItems.length === 0) return;

    const useRecycleBin = $generalSettings.useRecycleBin;
    const { softIds, permanentIds, hardIds } = planBulkDelete(selectedItems, useRecycleBin);
    const operationIds = new Set([...softIds, ...permanentIds, ...hardIds]);
    if (operationIds.size === 0) return;

    const previous = captureBulkSnapshot({
      items,
      indexedItems,
      searchCache,
      selectedIds,
      detailItem,
    });
    const softSet = new Set(softIds);

    for (const id of softIds) deletedHistorySuppressedIds.delete(id);
    for (const id of permanentIds) addSuppressedId(id);

    const hardSet = new Set(hardIds);
    const permanentSet = new Set(permanentIds);
    const removedOptimistic = new Set([...permanentSet, ...hardSet]);
    items = items
      .filter((item) => !removedOptimistic.has(item.id))
      .map((item) => (softSet.has(item.id) ? { ...item, deleted: true } : item));
    if (indexedItems) {
      indexedItems = indexedItems
        .filter((item) => !removedOptimistic.has(item.id))
        .map((item) => (softSet.has(item.id) ? { ...item, deleted: true } : item));
    }
    searchCache = searchCache
      .filter((item) => !removedOptimistic.has(item.id))
      .map((item) => (softSet.has(item.id) ? { ...item, deleted: true } : item));
    selectedIds = new Set();
    if (detailItem && removedOptimistic.has(detailItem.id)) {
      detailItem = null;
    } else if (detailItem && softSet.has(detailItem.id)) {
      detailItem = { ...detailItem, deleted: true };
    }

    const operations: {
      ids: string[];
      mode: "soft" | "permanent" | "hard";
      run: () => Promise<boolean | null>;
    }[] = [];
    if (softIds.length > 0) {
      operations.push({ ids: softIds, mode: "soft", run: () => persistBatchDelete(softIds) });
    }
    if (permanentIds.length > 0) {
      operations.push({
        ids: permanentIds,
        mode: "permanent",
        run: () => persistBatchPermanentDelete(permanentIds),
      });
    }
    for (const id of hardIds) {
      operations.push({ ids: [id], mode: "hard", run: () => persistHardDelete(id) });
    }

    void Promise.all(
      operations.map(async (operation) => {
        try {
          const result = await operation.run();
          return { ...operation, ok: result !== false };
        } catch (error) {
          console.error(
            operation.mode === "permanent"
              ? "Bulk permanent delete failed"
              : operation.mode === "hard"
                ? "Bulk hard delete failed"
                : "Bulk delete failed",
            error,
          );
          return { ...operation, ok: false };
        }
      }),
    ).then((outcomes) => {
      const successfulSoft = new Set(
        outcomes.filter((outcome) => outcome.ok && outcome.mode === "soft").flatMap((o) => o.ids),
      );
      const successfulPermanent = new Set(
        outcomes
          .filter((outcome) => outcome.ok && outcome.mode === "permanent")
          .flatMap((o) => o.ids),
      );
      const successfulHard = new Set(
        outcomes.filter((outcome) => outcome.ok && outcome.mode === "hard").flatMap((o) => o.ids),
      );
      const failedIds = new Set(outcomes.filter((outcome) => !outcome.ok).flatMap((o) => o.ids));
      const removedIds = new Set([...successfulPermanent, ...successfulHard]);
      const succeededIds = new Set([...successfulSoft, ...removedIds]);

      for (const id of successfulPermanent) addSuppressedId(id);
      for (const id of permanentIds) {
        if (!successfulPermanent.has(id)) deletedHistorySuppressedIds.delete(id);
      }
      for (const id of softIds) deletedHistorySuppressedIds.delete(id);

      // Rebuild from the snapshot so a partially failed mixed batch mirrors
      // exactly which backend transaction succeeded.
      items = previous.items
        .filter((item) => !removedIds.has(item.id))
        .map((item) => (successfulSoft.has(item.id) ? { ...item, deleted: true } : item));
      if (previous.indexedItems) {
        indexedItems = previous.indexedItems
          .filter((item) => !removedIds.has(item.id))
          .map((item) => (successfulSoft.has(item.id) ? { ...item, deleted: true } : item));
      } else {
        indexedItems = null;
      }
      searchCache = previous.searchCache
        .filter((item) => !removedIds.has(item.id))
        .map((item) => (successfulSoft.has(item.id) ? { ...item, deleted: true } : item));
      selectedIds = new Set([...previous.selectedIds].filter((id) => !succeededIds.has(id)));
      if (previous.detailItem && removedIds.has(previous.detailItem.id)) {
        detailItem = null;
      } else if (previous.detailItem && successfulSoft.has(previous.detailItem.id)) {
        detailItem = { ...previous.detailItem, deleted: true };
      } else {
        detailItem = previous.detailItem;
      }

      // The snapshot rebuild above mirrors backend success, but any items
      // that arrived via clipboard events during the async window are absent
      // from the snapshot. Failed (and partially failed) batches skip the
      // success-path invalidations, so resync from the backend explicitly.
      if (successfulSoft.size > 0 || successfulPermanent.size > 0 || failedIds.size > 0) {
        invalidateDeletedHistoryPagination();
      }
      if (successfulSoft.size > 0 || successfulHard.size > 0 || failedIds.size > 0) {
        invalidateActiveHistoryPagination();
      }
      if (failedIds.size > 0) {
        statusMessage = _t("app.deleteFailed");
        showToast(_t("app.deleteFailed"), "error");
      } else {
        showToast(_t("toast.bulkDeleteSuccess", { count: succeededIds.size }), "success");
      }
    });
  }

  function activateSelected() {
    if (!selectedId) return;
    copyItem(selectedId);
  }

  function moveSelection(offset: number) {
    if (filteredItems.length === 0) return;

    // With no current selection, step onto the first (Down) or last (Up)
    // row instead of skipping one via the clamped index.
    const next =
      selectedIndex === -1
        ? offset > 0
          ? 0
          : filteredItems.length - 1
        : Math.min(filteredItems.length - 1, Math.max(0, selectedIndex + offset));
    selectedId = filteredItems[next].id;
    const el = document.querySelector(`[data-id="${selectedId}"]`);
    if (el instanceof HTMLElement) {
      el.scrollIntoView({ block: "nearest" });
      el.focus();
      return;
    }

    // Virtual scroll: the target row may live outside the rendered window,
    // so there is no card element to scrollIntoView. Scroll the window so
    // the row becomes visible, then focus it after Svelte renders it.
    if (!useVirtualScroll || !historyListEl) return;
    const start = virtualPositions[next] ?? 0;
    const height = virtualHeights[next] ?? VIRTUAL_SCROLL_CONFIG.itemHeight;
    const viewport = historyListEl.clientHeight;
    const bottom = start + height;
    const target =
      start < historyListEl.scrollTop
        ? start
        : bottom > historyListEl.scrollTop + viewport
          ? bottom - viewport
          : historyListEl.scrollTop;
    scrollTop = Math.max(0, Math.min(target, Math.max(0, virtualList.totalHeight - viewport)));
    historyListEl.scrollTop = scrollTop;
    void tick().then(() => {
      const rendered = document.querySelector(`[data-id="${selectedId}"]`);
      if (rendered instanceof HTMLElement) rendered.focus();
    });
  }

  function clearHistory() {
    // The clear-history command operates on active records only. Keep rows
    // already in the recycle bin visible until they are restored or removed.
    const nonFavorites = items.filter((item) => !item.favorite && !item.deleted);
    if (nonFavorites.length === 0) {
      showToast(_t("toast.noRecordsToClear"), "info");
      return;
    }

    const ids = nonFavorites.map((item) => item.id);
    const idSet = new Set(ids);
    for (const id of ids) deletedHistorySuppressedIds.delete(id);
    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSearchCache = searchCache.map((entry) => ({ ...entry }));
    const previousDetailItem = detailItem ? { ...detailItem } : null;
    const previousSelectedIds = new Set(selectedIds);

    if ($generalSettings.useRecycleBin) {
      // Soft clear: retain rows locally so they immediately appear in the
      // recycle-bin filter and can be restored without a reload.
      items = items.map((item) => (idSet.has(item.id) ? { ...item, deleted: true } : item));
      if (indexedItems) {
        indexedItems = indexedItems.map((item) =>
          idSet.has(item.id) ? { ...item, deleted: true } : item,
        );
      }
      searchCache = searchCache.map((item) =>
        idSet.has(item.id) ? { ...item, deleted: true } : item,
      );
      if (detailItem && idSet.has(detailItem.id)) {
        detailItem = { ...detailItem, deleted: true };
      }
      selectedIds = new Set([...selectedIds].filter((id) => !idSet.has(id)));

      void invoke<number>("clear_all_non_favorite_items")
        .then((count) => {
          invalidateActiveHistoryPagination();
          invalidateDeletedHistoryPagination();
          showToast(_t("toast.clearHistorySuccess", { count }), "success");
        })
        .catch((error) => {
          console.error("Unable to clear history", error);
          items = previousItems;
          indexedItems = previousIndexedItems;
          searchCache = previousSearchCache;
          detailItem = previousDetailItem;
          selectedIds = previousSelectedIds;
          showToast(_t("app.deleteFailed"), "error");
        });
      return;
    }

    // Direct clear when the recycle bin is disabled. The backend's compact
    // clear command is intentionally soft-delete-only, so use the existing
    // direct-delete command for each active record instead.
    items = items.filter((item) => !idSet.has(item.id));
    if (indexedItems) indexedItems = indexedItems.filter((item) => !idSet.has(item.id));
    searchCache = searchCache.filter((item) => !idSet.has(item.id));
    if (detailItem && idSet.has(detailItem.id)) detailItem = null;
    selectedIds = new Set([...selectedIds].filter((id) => !idSet.has(id)));

    void Promise.all(
      ids.map(async (id) => {
        try {
          const removed = await persistHardDelete(id);
          return { id, ok: removed !== false };
        } catch (error) {
          console.error("Unable to directly delete history item", id, error);
          return { id, ok: false };
        }
      }),
    ).then((outcomes) => {
      const failedIds = new Set(outcomes.filter((outcome) => !outcome.ok).map((o) => o.id));
      const successfulIds = new Set(
        outcomes.filter((outcome) => outcome.ok).map((outcome) => outcome.id),
      );
      if (successfulIds.size > 0) invalidateActiveHistoryPagination();
      if (failedIds.size > 0) {
        items = previousItems.filter((item) => !successfulIds.has(item.id));
        if (previousIndexedItems) {
          indexedItems = previousIndexedItems.filter((item) => !successfulIds.has(item.id));
        } else {
          indexedItems = null;
        }
        searchCache = previousSearchCache.filter((item) => !successfulIds.has(item.id));
        detailItem =
          previousDetailItem && !successfulIds.has(previousDetailItem.id)
            ? previousDetailItem
            : null;
        selectedIds = new Set([...previousSelectedIds].filter((id) => !successfulIds.has(id)));
        statusMessage = _t("app.deleteFailed");
        showToast(_t("app.deleteFailed"), "error");
        return;
      }
      showToast(_t("toast.clearHistorySuccess", { count: ids.length }), "success");
    });
  }

  function handleEscapePriority(event: KeyboardEvent) {
    if (
      event.key !== "Escape" ||
      selectedIds.size === 0 ||
      editingId ||
      isEditableKeyboardTarget(event.target)
    ) {
      return;
    }

    selectedIds = new Set();
    // Don't preventDefault — let the event continue so a single Esc press
    // can clear bulk selection, close detail panel, or hide the window.
  }

  // Carries the pressed card id so the signal is consumed as an edge by the
  // targeted card only; a bare counter would re-fire on every other card the
  // moment it becomes selected.
  let tagAddSignal = $state<{ seq: number; itemId: string } | null>(null);

  function focusActiveFilterTab() {
    void tick().then(() => {
      const btn = document.querySelector<HTMLElement>(
        `.filters [role="tab"][aria-selected="true"]`,
      );
      btn?.focus();
    });
  }

  function executeKeyAction(event: KeyboardEvent, action: KeyAction) {
    if (action.prevent) event.preventDefault();
    switch (action.type) {
      case "none":
        break;
      case "focus-search":
        searchInputEl?.focus();
        break;
      case "quick-copy": {
        const item = filteredItems[action.index];
        if (item) {
          selectedId = item.id;
          activateSelected();
        }
        break;
      }
      case "set-filter":
        setFilter(action.filterId as ClipboardFilter);
        if (action.focusTab) focusActiveFilterTab();
        break;
      case "move-selection":
        moveSelection(action.delta);
        break;
      case "cycle-filter": {
        const idx = filters.findIndex((f) => f.id === activeFilter);
        const next = (idx + action.delta + filters.length) % filters.length;
        setFilter(filters[next].id);
        focusActiveFilterTab();
        break;
      }
      case "activate-selected":
        activateSelected();
        break;
      case "open-detail":
        openDetail(action.id);
        break;
      case "clear-selection":
        if (selectedIds.size > 0) selectedIds = new Set();
        break;
      case "select-all":
        selectedIds = new Set(filteredItems.map((i) => i.id));
        break;
      case "escape-hide-window":
        getCurrentWindow()
          .hide()
          .catch(() => {});
        break;
      case "escape-toggle-tag":
        toggleTagFilter(action.tag);
        break;
      case "bulk-copy":
        bulkCopy();
        break;
      case "copy-item":
        copyItem(action.id);
        break;
      case "bulk-delete":
        bulkDelete();
        break;
      case "delete-item":
        deleteItem(action.id);
        break;
      case "bulk-favorite":
        bulkFavorite();
        break;
      case "toggle-favorite":
        toggleFavorite(action.id);
        break;
      case "tag-add":
        tagAddSignal = { seq: (tagAddSignal?.seq ?? 0) + 1, itemId: selectedId ?? "" };
        break;
      case "save-item":
        saveItem(action.id);
        break;
      case "toggle-float":
        void toggleFloatPanel();
        break;
      case "quick-paste": {
        const item = findLoadedItem(action.id);
        if (item) {
          void pasteClipboardItem(item, "auto", {
            moveToTop: (mid) => moveToTop(mid),
          });
        }
        break;
      }
    }
  }

  function handleGlobalKeydown(event: KeyboardEvent) {
    executeKeyAction(
      event,
      resolveKeyAction(event, {
        hasEditing: !!editingId,
        hasFullscreen: !!fullscreenFilePath,
        hasTagDialog: !!tagEditDialog,
        hasDetail: !!detailItem,
        tagFilter,
        isTauri: "__TAURI_INTERNALS__" in window,
        detailEditor:
          isEditableKeyboardTarget(event.target) &&
          event.target instanceof Element &&
          event.target.closest(".detail-panel") !== null,
        isSearchInput: event.target === searchInputEl,
        selectedId,
        selectedCount: selectedIds.size,
        filteredItems,
        filters,
        activeFilter,
        filterShortcutBindings,
        moveSelectionDown: navigationBindings.moveSelectionDown,
        moveSelectionUp: navigationBindings.moveSelectionUp,
        switchFilterNext: navigationBindings.switchFilterNext,
        switchFilterPrev: navigationBindings.switchFilterPrev,
        toggleFloatBindings: floatPanelBindings,
        hideWindowBindings,
        quickPasteBindings,
        itemBindings: itemShortcutBindings,
        quickCopyBindings: quickCopyShortcutBindings,
        focusSearchBindings: focusSearchShortcutBindings,
      }),
    );
  }

  let scrollRaf = 0;

  function handleHistoryScroll() {
    if (!historyListEl) return;
    if (scrollRaf) return;
    scrollRaf = requestAnimationFrame(() => {
      scrollRaf = 0;
      if (!historyListEl) return;
      scrollTop = historyListEl.scrollTop;
      const nearBottom =
        historyListEl.scrollTop + historyListEl.clientHeight >= historyListEl.scrollHeight - 180;
      if (
        activeFilter === "deleted" &&
        deletedHistoryHasMore &&
        !deletedHistoryLoading &&
        nearBottom
      ) {
        void loadDeletedHistoryPage();
      }
      if (indexedItems !== null) {
        if (searchHasMore && !searchLoading && nearBottom) {
          void loadSearchPage();
        }
      } else if (
        activeFilter !== "deleted" &&
        activeHistoryHasMore &&
        !activeHistoryLoading &&
        nearBottom
      ) {
        void loadActiveHistoryPage();
      }
    });
  }

  async function measureContainer() {
    if (historyListEl) {
      containerHeight = historyListEl.clientHeight;
      containerWidth = historyListEl.clientWidth;
    }
  }

  $effect(() => {
    const el = historyListEl;
    if (!el) return;

    measureContainer();

    const ro = new ResizeObserver(() => {
      measureContainer();
    });
    ro.observe(el);

    return () => ro.disconnect();
  });

  const matchingSearchHistory = $derived.by<SearchOption[]>(() => {
    if (!$generalSettings.searchHistoryEnabled) return [];
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if ($generalSettings.searchSuggestionMode !== "panel" && normalizedQuery) return [];
    return searchHistory
      .filter((term) => !normalizedQuery || term.toLocaleLowerCase().includes(normalizedQuery))
      .map((value) => ({ value, kind: "history" as const }));
  });

  const matchingSearchSuggestions = $derived.by<SearchOption[]>(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (
      !normalizedQuery ||
      parseDateQuery(query) ||
      $generalSettings.searchSuggestionMode === "off"
    ) {
      return [];
    }

    const seen = new Set<string>();
    const candidates: SearchOption[] = [];
    const alignToQuery = $generalSettings.searchSuggestionMode === "inline";
    const values: Array<string | null | undefined> = [...sourceApps];
    const scanLimit = Math.min(items.length, 200);
    for (let i = 0; i < scanLimit; i++) {
      const item = items[i];
      if (item.deleted) continue;
      values.push(item.title, item.textContent, item.preview, item.sourceApp);
    }

    for (const rawValue of values) {
      const value = suggestionCandidate(rawValue, normalizedQuery, alignToQuery);
      if (!value) continue;
      const key = value.toLocaleLowerCase();
      if (seen.has(key) || key === normalizedQuery || !key.includes(normalizedQuery)) continue;
      seen.add(key);
      candidates.push({ value, kind: "suggestion" });
      if (candidates.length >= SEARCH_SUGGESTION_LIMIT) break;
    }
    return candidates;
  });

  const visibleSearchHistory = $derived(matchingSearchHistory.slice(0, SEARCH_SUGGESTION_LIMIT));
  const visibleSearchSuggestions = $derived(
    $generalSettings.searchSuggestionMode === "panel"
      ? matchingSearchSuggestions.slice(
          0,
          Math.max(SEARCH_SUGGESTION_LIMIT - visibleSearchHistory.length, 0),
        )
      : [],
  );
  const searchOptions = $derived([...visibleSearchHistory, ...visibleSearchSuggestions]);
  const inlineSearchSuggestion = $derived.by<SearchOption | null>(() => {
    if ($generalSettings.searchSuggestionMode !== "inline") return null;
    const normalizedQuery = normalizeSearchTerm(query);
    if (!normalizedQuery || query !== normalizedQuery) return null;
    const normalizedLower = normalizedQuery.toLocaleLowerCase();
    return (
      matchingSearchSuggestions.find((option) => {
        const valueLower = option.value.toLocaleLowerCase();
        return (
          valueLower.startsWith(normalizedLower) && option.value.length > normalizedQuery.length
        );
      }) ?? null
    );
  });
  const inlineSearchSuggestionSuffix = $derived.by(() => {
    if (!inlineSearchSuggestion) return "";
    const queryLength = normalizeSearchTerm(query).length;
    return inlineSearchSuggestion.value.slice(queryLength);
  });
  const showSearchSuggestions = $derived(searchSuggestionsOpen && searchOptions.length > 0);
  const searchAutocomplete = $derived(
    $generalSettings.searchSuggestionMode === "inline"
      ? showSearchSuggestions
        ? "both"
        : "inline"
      : showSearchSuggestions
        ? "list"
        : "none",
  );
  const activeSearchOption = $derived(searchOptions[searchSuggestionIndex] ?? null);

  $effect(() => {
    if (searchSuggestionIndex >= searchOptions.length) searchSuggestionIndex = -1;
  });

  $effect(() => {
    if (!$generalSettings.searchHistoryEnabled) pendingSearchHistoryQuery = "";
  });
</script>

<svelte:window onkeydowncapture={handleEscapePriority} onkeydown={handleGlobalKeydown} />

<main
  class="app-shell"
  class:split-detail={detailDisplayMode === "split" && detailItem != null}
  bind:this={appShellEl}
>
  <SearchHeader
    bind:query
    bind:inputEl={searchInputEl}
    placeholder={$generalSettings.searchPlaceholder?.trim() || _t("app.searchPlaceholder")}
    autocomplete={searchAutocomplete}
    height={searchHeight}
    fontSize={searchFontSize}
    showSuggestions={showSearchSuggestions}
    activeOptionIndex={activeSearchOption ? searchOptions.indexOf(activeSearchOption) : -1}
    historyOptions={visibleSearchHistory}
    suggestionOptions={visibleSearchSuggestions}
    inlineSuggestionSuffix={inlineSearchSuggestion ? inlineSearchSuggestionSuffix : null}
    onfocus={() => (searchSuggestionsOpen = true)}
    oninput={() => {
      searchSuggestionsOpen = true;
      searchSuggestionIndex = -1;
      if (pendingSearchHistoryQuery && query.trim() !== pendingSearchHistoryQuery) {
        pendingSearchHistoryQuery = "";
      }
    }}
    onblur={handleSearchInputBlur}
    onkeydown={handleSearchInputKeydown}
    onchoose={chooseSearchOption}
    onclear={clearSearchQuery}
  />

  <Toolbar
    {filters}
    {activeFilter}
    {filterShortcutBindings}
    onselectfilter={(id) => setFilter(id)}
    {sourceApps}
    {sourceAppFilter}
    onsourceapp={(app) => {
      sourceAppFilter = app;
      resetHistoryScroll();
      void invalidateActiveHistoryPagination();
    }}
    {dateFilter}
    {dateFilterOptions}
    ondatefilter={(id) => {
      dateFilter = id;
      resetHistoryScroll();
      void invalidateActiveHistoryPagination();
    }}
    onsettings={openSettings}
  />

  <div
    class="main-content"
    class:split-detail={detailDisplayMode === "split" && detailItem != null}
  >
    <HistoryList
      items={visiblePageItems}
      hasItems={filteredItems.length > 0}
      {useVirtualScroll}
      {virtualList}
      indexById={filteredItemIndexById}
      {currentTime}
      {selectedIds}
      {selectedId}
      splitDetail={detailDisplayMode === "split" && detailItem != null}
      {cardPaddingTop}
      {cardPaddingBottom}
      {cardGap}
      {cardBorderRadius}
      {maxTextLines}
      {showSecondaryText}
      {alwaysShowActions}
      {quickCopyBadgeAlwaysVisible}
      {doubleClickPaste}
      {tagColors}
      {tagAddSignal}
      panelLabel={_t("app.recentRecords")}
      emptyTitle={items.length === 0 ? _t("app.noRecords") : _t("app.noMatchRecords")}
      emptyHint={items.length === 0 ? _t("app.noRecordsHint") : _t("app.noMatchRecordsHint")}
      {cardHeightFor}
      {cardLayoutSignature}
      bind:listEl={historyListEl}
      onscroll={handleHistoryScroll}
      onheightchange={recordCardHeight}
      onselect={selectItem}
      ontoggleSelect={toggleSelectItem}
      ontoggleFavorite={toggleFavorite}
      ondelete={deleteItem}
      oncopy={copyItem}
      oncopyPath={copyPath}
      onsave={saveItem}
      ondetail={openDetail}
      onimagefullscreen={handleImageFullscreen}
      onmaterialize={prepareItemMaterialization}
      onedit={startEdit}
      onsaveedit={saveEdit}
      onsaveasnew={saveAsNew}
      oncanceledit={cancelEdit}
      onplainpaste={plainPaste}
      onformatpaste={formatPaste}
      oncleanpaste={cleanPaste}
      ondblclickpaste={doubleClickPasteItem}
      onrestore={restoreItem}
      onsavetags={saveTags}
      ontoggleTagFilter={toggleTagFilter}
      oneditTag={openTagEdit}
    />

    <BulkBar
      selectedCount={selectedIds.size}
      {selectedActiveCount}
      {selectedDeletedCount}
      {activeFilter}
      {allSelectedFavorites}
      ondeselect={() => (selectedIds = new Set())}
      oncopy={bulkCopy}
      ondelete={bulkDelete}
      onfavorite={bulkFavorite}
      onrestore={bulkRestore}
      onpermanentdelete={bulkPermanentDelete}
    />

    {#if detailDisplayMode === "split" && detailItem}
      <DetailPanel
        mode="split"
        item={detailItem}
        onclose={closeDetail}
        oncopy={copyItem}
        oncopyPath={copyPath}
        onedit={startEdit}
        onsaveedit={saveEdit}
        onrenametitle={renameTitle}
        onplainpaste={plainPaste}
        onformatpaste={formatPaste}
        oncleanpaste={cleanPaste}
        onduplicate={duplicateItem}
        onsaveasnew={saveAsNew}
        onocrupdate={(id, patch) => updateItem(id, () => patch)}
        oncopyfilename={copyFilename}
        onimagefullscreen={handleImageFullscreen}
        onsavetags={saveTags}
        {tagColors}
      />
    {/if}
  </div>

  <StatusBar {resultSummary} {runtimeLabel} {statusMessage} toggleHint={toggleWindowHint} />
</main>

<Toast />
{#if tagEditDialog}
  <TagEditDialog
    tag={tagEditDialog}
    color={tagColors[tagEditDialog] ?? ""}
    onclose={() => (tagEditDialog = null)}
  />
{/if}
{#if detailDisplayMode !== "split" || !detailItem}
  <DetailPanel
    item={detailItem}
    onclose={closeDetail}
    oncopy={copyItem}
    oncopyPath={copyPath}
    onedit={startEdit}
    onsaveedit={saveEdit}
    onrenametitle={renameTitle}
    onplainpaste={plainPaste}
    onformatpaste={formatPaste}
    oncleanpaste={cleanPaste}
    onduplicate={duplicateItem}
    onsaveasnew={saveAsNew}
    onocrupdate={(id, patch) => updateItem(id, () => patch)}
    oncopyfilename={copyFilename}
    onimagefullscreen={handleImageFullscreen}
    onsavetags={saveTags}
    {tagColors}
  />
{/if}

{#if fullscreenFilePath}
  <ImageFullscreenOverlay
    filePath={fullscreenFilePath}
    opacity={fullscreenOpacity}
    mode={fullscreenMode}
    onclose={closeFullscreen}
  />
{/if}

<style>
  .app-shell {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr) auto;
    grid-template-columns: 1fr;
    width: 100%;
    min-width: 710px;
    height: 100vh;
    min-height: 480px;
    overflow: hidden;
    border: 1px solid var(--border-color);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--bg-app) var(--window-opacity-shell, 98.5%), transparent);
  }

  .app-shell.split-detail {
    grid-template-columns: minmax(0, 1fr) minmax(360px, 520px);
  }

  .app-shell.split-detail > .main-content {
    grid-column: 1 / -1;
  }

  .main-content {
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .main-content.split-detail {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(360px, 520px);
  }

  .main-content.split-detail > :global(*:last-child) {
    grid-column: 2;
    grid-row: 1 / -1;
  }
</style>
