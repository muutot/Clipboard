<script lang="ts">
  import { flushSync, onMount, tick, untrack } from "svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import ClipboardCard from "$lib/components/ClipboardCard.svelte";
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
    persistLastUsed,
    listAllTags,
    searchClipboardHistory,
    listSourceApplications,
    formatTextLength,
    generatedClipboardTitle,
    materializeClipboardItem,
    toClipboardItem,
    writeClipboardImage,
    writeClipboardText,
    writeClipboardHtml,
    getDisplayTitle,
    getDisplayRemainingLines,
    type HistoryFilterArgs,
  } from "$lib/services/clipboard";
  import { getRuntimeInfo, isTauriRuntime } from "$lib/services/runtime";
  import { showToast } from "$lib/services/toast";
  import { getKeyboardConfig } from "$lib/services/keyboard";
  import type { ClipboardFilter, ClipboardItem, WindowPosition } from "$lib/types/clipboard";
  import type { IconName } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { assets } from "$app/paths";
  import {
    createVirtualList,
    editHeight,
    buildPositions,
    type VirtualScrollConfig,
  } from "$lib/utils/virtual-scroll";
  import { estimateCardHeight } from "$lib/utils/card-height";
  import { parseDateQuery, startOfDay, endOfDay, startOfWeek } from "$lib/utils/date-query";
  import { isEditableKeyboardTarget, shortcutMatchesEvent } from "$lib/utils/keyboard";
  import { alignDropdownOptionText } from "$lib/utils/dropdown";
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

  interface TextTransformResult {
    input: string;
    operation: string;
    result: string;
  }

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
    const original = items.find((i) => i.id === id);
    if (!original) return false;
    applyItemPatches(new Map([[id, mutator(original)]]));
    return true;
  }

  /// Fans one patch set out to every copy (items, indexedItems, searchCache,
  /// detailItem) in a single pass over each list. THE funnel for item
  /// mutations 鈥?direct per-copy mapping loops are how the searchCache drift
  /// happened.
  function applyItemPatches(patches: ReadonlyMap<string, Partial<ClipboardItem>>) {
    if (patches.size === 0) return;
    const apply = (entry: ClipboardItem): ClipboardItem => {
      const patch = patches.get(entry.id);
      return patch ? { ...entry, ...patch } : entry;
    };
    items = items.map(apply);
    if (indexedItems) indexedItems = indexedItems.map(apply);
    searchCache = searchCache.map(apply);
    if (detailItem) {
      const patch = patches.get(detailItem.id);
      if (patch) detailItem = { ...detailItem, ...patch };
    }
  }

  /// Removal counterpart of [`applyItemPatches`]: drops ids from every copy
  /// (items, indexedItems, searchCache, detailItem) in one pass per list.
  /// Purging searchCache here closes the gap where a permanently deleted
  /// entry could later resurface from the spare-result cache.
  function removeItems(ids: ReadonlySet<string>) {
    if (ids.size === 0) return;
    items = items.filter((item) => !ids.has(item.id));
    if (indexedItems) indexedItems = indexedItems.filter((item) => !ids.has(item.id));
    searchCache = searchCache.filter((item) => !ids.has(item.id));
    if (detailItem && ids.has(detailItem.id)) detailItem = null;
  }

  function revertItem(id: string, fields: Partial<ClipboardItem>) {
    applyItemPatches(new Map([[id, fields]]));
  }

  function replaceMaterializedItem(updated: ClipboardItem): ClipboardItem {
    const replace = (item: ClipboardItem) => (item.id === updated.id ? updated : item);
    items = items.map(replace);
    if (indexedItems) indexedItems = indexedItems.map(replace);
    searchCache = searchCache.map(replace);
    if (detailItem?.id === updated.id) detailItem = updated;
    return updated;
  }

  function findLoadedItem(id: string): ClipboardItem | undefined {
    return (
      items.find((item) => item.id === id) ??
      indexedItems?.find((item) => item.id === id) ??
      searchCache.find((item) => item.id === id) ??
      (detailItem?.id === id ? detailItem : undefined)
    );
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
  let sourceAppSearch = $state("");
  let sourceAppDropdownOpen = $state(false);
  let dateDropdownOpen = $state(false);
  let sourceAppDropdownEl: HTMLDivElement | undefined = $state();
  let dateDropdownEl: HTMLDivElement | undefined = $state();

  let detailItem = $state<ClipboardItem | null>(null);

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
    const activeIds = new Set(items.map((i) => i.id));
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
  // An absent action falls back to its default (Alt+<N>, one per filter
  // position); an action explicitly configured to empty disables the shortcut.
  let keyboardShortcuts = $state<Record<string, string[]>>({});

  const filterShortcutBindings = $derived.by(() => {
    const map: Record<string, string[]> = {};
    filters.forEach((filter, index) => {
      const actionKey = `switchFilter${index + 1}`;
      const hasAction = Object.prototype.hasOwnProperty.call(keyboardShortcuts, actionKey);
      map[filter.id] = hasAction ? (keyboardShortcuts[actionKey] ?? []) : [`Alt+${index + 1}`];
    });
    return map;
  });

  // Configured navigation shortcuts (conf/keyboard.json), reusing the same
  // action keys the settings panel exposes under the "閸掑洦宕? group. An absent
  // action falls back to its default; an action explicitly configured to empty
  // disables that navigation binding.
  const navigationBindings = $derived.by(() => {
    const read = (action: string, fallback: string[]): string[] => {
      const hasAction = Object.prototype.hasOwnProperty.call(keyboardShortcuts, action);
      return hasAction ? (keyboardShortcuts[action] ?? []) : fallback;
    };
    return {
      moveSelectionUp: read("moveSelectionUp", ["ArrowUp"]),
      moveSelectionDown: read("moveSelectionDown", ["ArrowDown"]),
      switchFilterNext: read("switchFilterNext", ["ArrowRight", "Tab"]),
      switchFilterPrev: read("switchFilterPrev", ["ArrowLeft", "Shift+Tab"]),
    };
  });

  async function loadKeyboardShortcuts() {
    try {
      const config = await getKeyboardConfig();
      keyboardShortcuts = config?.shortcuts ?? {};
    } catch {
      keyboardShortcuts = {};
    }
  }

  // --- Date range resolution ---

  function resolveDateRange(filter: string): { from: number; to: number } | null {
    const now = Date.now();
    const dayMs = 24 * 60 * 60 * 1_000;

    switch (filter) {
      case "today":
        return { from: startOfDay(now), to: endOfDay(now) };
      case "yesterday":
        return { from: startOfDay(now - dayMs), to: endOfDay(now - dayMs) };
      case "week":
        return { from: startOfWeek(now), to: endOfDay(now) };
      case "month": {
        const d = new Date(now);
        d.setDate(1);
        d.setHours(0, 0, 0, 0);
        return { from: d.getTime(), to: endOfDay(now) };
      }
      default:
        return null;
    }
  }

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

  const filteredItems = $derived.by(() => {
    const normalizedQuery = query.trim();
    const usesIndexedResults =
      activeFilter !== "deleted" && indexedItems !== null && indexedQuery === normalizedQuery;
    const candidates = usesIndexedResults ? (indexedItems ?? []) : items;

    const dateRange = resolveDateRange(dateFilter);
    const dateRangeFromNl = !dateRange ? parseDateQuery(normalizedQuery) : null;
    const effectiveDateRange = dateRange ?? dateRangeFromNl;
    // A natural-language date token is a filter, not content that must occur
    // in the record text (e.g. "閺勩劌銇? should not be required in the title).
    const keywords = dateRangeFromNl
      ? []
      : normalizedQuery.toLocaleLowerCase().split(/\s+/).filter(Boolean);

    return candidates.filter((item) => {
      const isDeleted = !!item.deleted;
      const matchesFilter =
        activeFilter === "all"
          ? !isDeleted
          : activeFilter === "deleted"
            ? isDeleted
            : activeFilter === "favorite"
              ? !isDeleted && item.favorite
              : !isDeleted && item.kind === activeFilter;

      if (!matchesFilter) return false;

      if (tagFilter && !(item.tags ?? []).includes(tagFilter)) return false;

      if (
        sourceAppFilter &&
        !item.sourceApp.toLowerCase().includes(sourceAppFilter.toLowerCase())
      ) {
        return false;
      }

      if (effectiveDateRange) {
        if (item.createdAt < effectiveDateRange.from || item.createdAt > effectiveDateRange.to) {
          return false;
        }
      }

      if (keywords.length === 0 || usesIndexedResults) {
        return true;
      }

      return keywords.every((keyword) => (item.searchableText ?? "").includes(keyword));
    });
  });

  const selectedIndex = $derived(filteredItems.findIndex((item) => item.id === selectedId));
  const selectedDeletedCount = $derived(
    selectedIds.size === 0
      ? 0
      : items.filter((item) => selectedIds.has(item.id) && !!item.deleted).length,
  );
  const selectedActiveCount = $derived(
    selectedIds.size === 0
      ? 0
      : items.filter((item) => selectedIds.has(item.id) && !item.deleted).length,
  );
  const allSelectedFavorites = $derived(
    selectedIds.size > 0 &&
      items.filter((item) => selectedIds.has(item.id)).every((item) => item.favorite),
  );
  const resultSummary = $derived(
    searchPending
      ? _t("status.searching")
      : _t("status.recordCount", { count: filteredItems.length }),
  );

  // --- Virtual scrolling ---

  const compactMode = $derived($generalSettings.compactMode);
  const effectiveContainerWidth = $derived(Math.max(680, containerWidth));
  const compactText = $derived($generalSettings.compactTextHeight);
  const compactTallText = $derived($generalSettings.compactTallTextHeight);
  const compactImage = $derived($generalSettings.compactImageHeight);
  const compactCustomTitle = $derived($generalSettings.compactCustomTitleHeight);
  const compactCardGap = $derived($generalSettings.compactCardGap);
  const compactPaddingTop = $derived($generalSettings.compactPaddingTop);
  const compactPaddingBottom = $derived($generalSettings.compactPaddingBottom);
  const compactSearchHeight = $derived($generalSettings.compactSearchHeight);
  const compactSearchFontSize = $derived($generalSettings.compactSearchFontSize);
  const compactCardBorderRadius = $derived($generalSettings.compactCardBorderRadius);
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
        compactMode,
        compactImage,
        compactText,
        compactTallText,
        compactCustomTitle,
        compactCardGap,
        compactPaddingTop,
        compactPaddingBottom,
        showSecondaryText,
        maxTextLines,
        previewFontSize: $generalSettings.fontSizes.cardPreview,
        contentWidth: effectiveContainerWidth,
      },
      detailDisplayMode === "split" && detailItem?.id === item.id,
    );
  }

  function compactCardHeightFor(item: ClipboardItem): number {
    if (!compactMode) return 0;
    return Math.max(0, estimatedCardHeight(item) - compactCardGap);
  }

  function cardLayoutSignaturePrefix(): string {
    return [
      containerWidth,
      compactMode,
      compactText,
      compactTallText,
      compactImage,
      compactCustomTitle,
      compactCardGap,
      compactPaddingTop,
      compactPaddingBottom,
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
    // prefix 鈥?opening/closing the detail panel must not invalidate every
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
    const measured = measuredCardHeights[item.id];
    if (measured) return measured.height;
    if (editingId === item.id) {
      return editHeight(
        (item.textContent || "").split("\n").length,
        !!item.customTitle,
        compactCardGap,
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
          if (requestId !== searchRequestId || results === null) return;
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

  // After filteredItems changes, prune invalid selectedIds
  $effect(() => {
    const idSet = new Set(filteredItems.map((i) => i.id));
    let changed = false;
    for (const id of selectedIds) {
      if (!idSet.has(id)) {
        selectedIds = new Set([...selectedIds].filter((x) => x !== id));
        changed = true;
        break;
      }
    }
    if (!changed && selectedIds.size > 0 && filteredItems.length === 0) {
      selectedIds = new Set();
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
        return loadActiveHistoryPage();
      })
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
        // and keeps the same row id with a bumped created_at_ms, but the list
        // order is driven by array position rather than the timestamp. Promote
        // it to the top so the just-copied entry is visibly pinned.
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
        searchRequestId += 1;
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
      const shell = appShellEl;
      if (shell) {
        shell.classList.toggle("compact", s.compactMode);
      }
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
      // out through the single funnel 鈥?including searchCache, which a
      // hand-rolled loop once missed.
      const transform = (entry: ClipboardItem) =>
        renamed ? rewriteTags(entry, renamed.old, renamed.new) : removeTag(entry, deleted!);
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

  function mergeDeletedHistoryPage(page: ClipboardItem[]) {
    const incoming = new Map(page.map((item) => [item.id, item]));
    const merged = items
      .filter((item) => !item.deleted || !deletedHistorySuppressedIds.has(item.id))
      .map((item) => {
        const persisted = incoming.get(item.id);
        // A local restore/delete mutation may have landed while the page was
        // in flight. Do not let a stale recycle-bin response undo a restore.
        if (!persisted || !item.deleted) return item;
        return { ...item, ...persisted, deleted: true };
      });
    const existingIds = new Set(merged.map((item) => item.id));
    for (const item of page) {
      if (deletedHistorySuppressedIds.has(item.id)) continue;
      if (!existingIds.has(item.id)) merged.push({ ...item, deleted: true });
    }
    items = merged;
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

  function buildActiveHistoryFilter(): HistoryFilterArgs {
    if (activeFilter === "deleted") return {};
    const dateRange = resolveDateRange(dateFilter);
    return {
      kind: activeFilter === "all" || activeFilter === "favorite" ? null : activeFilter,
      favorite: activeFilter === "favorite",
      tag: tagFilter,
      sourceApp: sourceAppFilter || null,
      dateFromMs: dateRange?.from ?? null,
      dateToMs: dateRange?.to ?? null,
    };
  }

  function updateSearchCache(results: ClipboardItem[]) {
    const loadedIds = new Set(items.map((i) => i.id));
    const policy = $generalSettings.searchCacheEviction;
    const cacheById = new Map(searchCache.map((item) => [item.id, item]));
    let accessOrder = searchCacheAccessOrder.filter((id) => cacheById.has(id));

    for (const item of results) {
      if (loadedIds.has(item.id)) {
        cacheById.delete(item.id);
        accessOrder = accessOrder.filter((id) => id !== item.id);
        continue;
      }

      const cached = cacheById.has(item.id);
      cacheById.set(item.id, item);
      if (!cached) {
        accessOrder.push(item.id);
      } else if (policy === "lru") {
        accessOrder = accessOrder.filter((id) => id !== item.id);
        accessOrder.push(item.id);
      }
    }

    const max = $generalSettings.searchCacheSize;
    while (accessOrder.length > max) {
      const id = accessOrder.shift();
      if (id) cacheById.delete(id);
    }

    searchCacheAccessOrder = accessOrder;
    searchCache = accessOrder.flatMap((id) => {
      const item = cacheById.get(id);
      return item ? [item] : [];
    });
  }

  function promoteFromCache(loadedIds: Set<string>) {
    if (!loadedIds.size) return;
    const promoted = new Set<string>();
    for (const id of loadedIds) {
      if (searchCache.some((c) => c.id === id)) promoted.add(id);
    }
    if (!promoted.size) return;
    searchCache = searchCache.filter((c) => !promoted.has(c.id));
    searchCacheAccessOrder = searchCacheAccessOrder.filter((id) => !promoted.has(id));
  }

  function trimLoadedItems() {
    const limit = $generalSettings.pageSizeLimit;
    const tolerance = $generalSettings.loadTolerance;
    const max = limit + tolerance;
    if (items.length <= max) return;

    const evictable = items
      .filter((i) => !i.deleted && !i.favorite)
      .sort((a, b) => a.createdAt - b.createdAt);

    const toEvict = evictable.slice(0, tolerance);
    const evictIds = new Set(toEvict.map((i) => i.id));
    items = items.filter((i) => !evictIds.has(i.id));
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
        buildActiveHistoryFilter(),
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

      indexedItems = [...(indexedItems ?? []), ...results];
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

      mergeDeletedHistoryPage(page);
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

  function canAcceptInlineSuggestion(): boolean {
    const input = searchInputEl;
    return Boolean(
      inlineSearchSuggestion &&
      input &&
      input.selectionStart === input.selectionEnd &&
      input.selectionEnd === query.length,
    );
  }

  function acceptInlineSuggestion(): boolean {
    const suggestion = inlineSearchSuggestion;
    if (!suggestion || !canAcceptInlineSuggestion()) return false;

    query = suggestion.value;
    pendingSearchHistoryQuery = "";
    searchSuggestionsOpen = false;
    searchSuggestionIndex = -1;
    void tick().then(() => {
      const input = searchInputEl;
      if (!input) return;
      input.focus();
      input.setSelectionRange(query.length, query.length);
    });
    return true;
  }

  function handleSearchInputKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;

    if (event.key === "Backspace") {
      const now = Date.now();
      if (now - lastBackspaceAt < 400 && query) {
        event.preventDefault();
        query = "";
        pendingSearchHistoryQuery = "";
        searchSuggestionIndex = -1;
        lastBackspaceAt = 0;
      } else {
        lastBackspaceAt = now;
      }
      return;
    }
    lastBackspaceAt = 0;

    if (
      (event.key === "Tab" || event.key === "ArrowRight") &&
      !event.shiftKey &&
      canAcceptInlineSuggestion()
    ) {
      event.preventDefault();
      event.stopPropagation();
      acceptInlineSuggestion();
      return;
    }

    if (
      (event.key === "ArrowDown" || event.key === "ArrowUp") &&
      searchSuggestionsOpen &&
      searchOptions.length > 0
    ) {
      event.preventDefault();
      event.stopPropagation();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      searchSuggestionIndex =
        (searchSuggestionIndex + direction + searchOptions.length) % searchOptions.length;
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      if (activeSearchOption) {
        chooseSearchOption(activeSearchOption.value);
      } else {
        commitSearchQuery();
      }
      return;
    }

    if (event.key === "Escape") {
      if (searchSuggestionsOpen) {
        event.preventDefault();
        event.stopPropagation();
        searchSuggestionsOpen = false;
        searchSuggestionIndex = -1;
        if (query) {
          query = "";
          pendingSearchHistoryQuery = "";
        }
        return;
      }
      if (query) {
        event.preventDefault();
        event.stopPropagation();
        query = "";
        pendingSearchHistoryQuery = "";
        return;
      }
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

  function deleteItem(id: string) {
    const item = items.find((i) => i.id === id);
    if (item?.deleted) {
      permanentlyDeleteItem(id);
      return;
    }
    if (!$generalSettings.useRecycleBin) {
      hardDeleteItem(id);
      return;
    }

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);

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
        items = previousItems;
        indexedItems = previousIndexedItems;
        selectedIds = previousSelectedIds;
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function permanentlyDeleteItem(id: string) {
    const target = items.find((item) => item.id === id);
    if (!target) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);

    addSuppressedId(id);
    items = items.filter((item) => item.id !== id);
    if (indexedItems) indexedItems = indexedItems.filter((item) => item.id !== id);
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
        items = previousItems;
        indexedItems = previousIndexedItems;
        selectedIds = previousSelectedIds;
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  // Direct deletion remains a separate backend path: the recycle-bin
  // permanent-delete command intentionally accepts only already deleted
  // rows, while this path handles active rows when the feature is disabled.
  function hardDeleteItem(id: string) {
    const target = items.find((item) => item.id === id);
    if (!target) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);

    items = items.filter((item) => item.id !== id);
    if (indexedItems) indexedItems = indexedItems.filter((item) => item.id !== id);
    selectedIds = new Set([...selectedIds].filter((x) => x !== id));

    void persistHardDelete(id)
      .then((removed) => {
        if (removed === false) throw new Error("record not found");
        invalidateActiveHistoryPagination();
        showToast(_t("toast.deleteSuccess"), "success");
      })
      .catch((error) => {
        console.error("Unable to delete clipboard item", error);
        items = previousItems;
        indexedItems = previousIndexedItems;
        selectedIds = previousSelectedIds;
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function restoreItem(id: string) {
    const target = items.find((item) => item.id === id);
    if (!target?.deleted) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;

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
        items = previousItems;
        indexedItems = previousIndexedItems;
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function moveToTop(id: string) {
    const idx = items.findIndex((i) => i.id === id);
    if (idx > 0) {
      const [item] = items.splice(idx, 1);
      items = [item, ...items];
    }
  }

  async function copyItem(id: string) {
    let item = findLoadedItem(id);
    if (!item) return;

    if (item.kind === "image" || item.kind === "file") {
      try {
        item = await ensureItemMaterialized(item);
      } catch (error) {
        console.error("Unable to materialize clipboard item for copy", error);
        showToast(_t("toast.copyFailed"), "error");
        return;
      }
    }

    if ($generalSettings.pinCopiedToTop) moveToTop(id);

    void persistLastUsed(id);

    if (item.kind === "image" && item.resourcePath) {
      try {
        const src = convertFileSrc(item.resourcePath.replace(/\\/g, "/"));
        const response = await fetch(src);
        const blob = await response.blob();
        await writeClipboardImage(blob, item.resourcePath, item.contentHash);
        statusMessage = _t("app.copiedItem", { title: getDisplayTitle(item.title) });
        showToast(_t("toast.copySuccess"), "success");
      } catch {
        showToast(_t("toast.copyFailed"), "error");
      }
      return;
    }

    if (item.kind === "file") {
      if (item.textContent && item.textContent.startsWith("[")) {
        try {
          const paths = JSON.parse(item.textContent) as string[];
          if (paths.length > 1) {
            await writeClipboardText(paths.join("\n"));
            statusMessage = _t("app.copiedItem", { title: getDisplayTitle(item.title) });
            showToast(_t("toast.copySuccess"), "success");
            return;
          }
        } catch {
          /* ignore */
        }
      }
      if (item.resourcePath) {
        try {
          await writeClipboardText(item.resourcePath);
          statusMessage = _t("app.copiedItem", {
            title: item.fileName || getDisplayTitle(item.title),
          });
          showToast(_t("toast.copySuccess"), "success");
        } catch {
          showToast(_t("toast.copyFailed"), "error");
        }
      }
      return;
    }

    void writeClipboardText(item.textContent || item.title)
      .then(() => {
        statusMessage = _t("app.copiedItem", { title: getDisplayTitle(item.title) });
        showToast(_t("toast.copySuccess"), "success");
      })
      .catch(() => {
        showToast(_t("toast.copyFailed"), "error");
      });
  }

  async function openDetail(id: string) {
    const item = findLoadedItem(id);
    if (!item) return;
    detailItem = item;
    if (item.kind === "image" || item.kind === "file") {
      try {
        detailItem = await ensureItemMaterialized(item);
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
    const item = items.find((i) => i.id === id);
    if (!item) return false;

    const isMedia = item?.kind === "image" || item?.kind === "file";
    const isText = item?.kind === "text" || item?.kind === "link";
    const newTitle = isText
      ? item.customTitle
        ? item.title
        : generatedClipboardTitle(content)
      : content;
    const newTextContent = isText ? content : (item?.textContent ?? null);
    const newPreview = isText && content.length > 200 ? content.slice(200) : (item?.preview ?? "");
    const newSizeBytes = new TextEncoder().encode(content).byteLength;
    const newSizeLabel = formatTextLength(content.length);

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
    updateItem(id, () => ({ title, customTitle: true }));
    invoke("rename_item", { id, newName: title }).catch((err) =>
      console.error("Rename item failed:", err),
    );
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

  function rewriteTags(item: ClipboardItem, oldName: string, newName: string): ClipboardItem {
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

  function removeTag(item: ClipboardItem, name: string): ClipboardItem {
    const tags = item.tags ?? [];
    if (!tags.includes(name)) return item;
    return { ...item, tags: tags.filter((tag) => tag !== name) };
  }

  async function saveTags(id: string, tags: string[]) {
    const deduped = [...new Set(tags.map((t) => t.trim()).filter(Boolean))];
    // updateItem fans the patch out to items, indexedItems, searchCache, and
    // detailItem so no copy of the entry keeps stale tags.
    updateItem(id, () => ({ tags: deduped }));
    const ok = await persistTags(id, deduped);
    if (ok === false) {
      showToast(_t("toast.saveFailed"), "error");
    }
    refreshTagColors();
  }

  async function cleanTextIfEnabled(text: string): Promise<string> {
    if (!$generalSettings.pasteCleaningEnabled) return text;
    try {
      const transform = await invoke<TextTransformResult>("transform_text", {
        operation: "cleanPaste",
        input: text,
      });
      return transform.result;
    } catch (error) {
      console.error("Unable to clean text before paste", error);
      return text;
    }
  }

  async function pasteToPreviousApplication(
    item: ClipboardItem,
    keys: { paste: string; copy: string; failed: string },
    write: () => Promise<void>,
  ): Promise<void> {
    if ($generalSettings.pinCopiedToTop) moveToTop(item.id);
    try {
      await write();
    } catch (error) {
      console.error("Unable to prepare clipboard content for paste", error);
      showToast(_t(keys.failed), "error");
      return;
    }
    void persistLastUsed(item.id);

    if (!isTauriRuntime()) {
      showToast(_t(keys.copy), "success");
      return;
    }

    try {
      const pasted = await invoke<boolean>("paste_to_previous_application");
      showToast(_t(pasted ? keys.paste : keys.copy), pasted ? "success" : "info");
    } catch (error) {
      console.error("Unable to restore the previous application and paste", error);
      showToast(_t(keys.failed), "error");
    }
  }

  async function plainPaste(_id: string) {
    const item = items.find((i) => i.id === _id);
    if (!item) return;
    const text = item.textContent || item.title;
    await pasteToPreviousApplication(
      item,
      {
        paste: "toast.plainPasteSuccess",
        copy: "toast.plainCopySuccess",
        failed: "toast.plainPasteFailed",
      },
      async () => {
        const cleaned = await cleanTextIfEnabled(text);
        await writeClipboardText(cleaned);
      },
    );
  }

  async function formatPaste(_id: string) {
    const item = items.find((i) => i.id === _id);
    if (!item || !item.htmlContent) return;
    const htmlContent = item.htmlContent;
    const plainText = item.textContent || undefined;
    await pasteToPreviousApplication(
      item,
      {
        paste: "toast.formatPasteSuccess",
        copy: "toast.formatCopySuccess",
        failed: "toast.formatPasteFailed",
      },
      async () => {
        if (plainText && $generalSettings.pasteCleaningEnabled) {
          const cleaned = await cleanTextIfEnabled(plainText);
          if (cleaned !== plainText) {
            await writeClipboardText(cleaned);
            return;
          }
        }
        await writeClipboardHtml(htmlContent, plainText, item.rtfContent);
      },
    );
  }

  async function cleanPaste(_id: string) {
    const item = items.find((i) => i.id === _id);
    if (!item) return;
    const text = item.textContent || item.title;
    await pasteToPreviousApplication(
      item,
      {
        paste: "toast.cleanPasteSuccess",
        copy: "toast.cleanCopySuccess",
        failed: "toast.cleanPasteFailed",
      },
      async () => {
        const transform = await invoke<TextTransformResult>("transform_text", {
          operation: "cleanPaste",
          input: text,
        });
        await writeClipboardText(transform.result);
      },
    );
  }

  async function doubleClickPasteItem(id: string) {
    let item = findLoadedItem(id);
    if (!item) return;

    if (item.kind === "text" || item.kind === "link") {
      if (item.htmlContent) {
        await formatPaste(id);
      } else {
        await plainPaste(id);
      }
      return;
    }

    if (item.kind === "image") {
      try {
        item = await ensureItemMaterialized(item);
      } catch (error) {
        console.error("Unable to materialize image for paste", error);
        showToast(_t("toast.imagePasteFailed"), "error");
        return;
      }
      const materializedItem = item;
      await pasteToPreviousApplication(
        materializedItem,
        {
          paste: "toast.imagePasteSuccess",
          copy: "toast.imageCopySuccess",
          failed: "toast.imagePasteFailed",
        },
        async () => {
          const src = convertFileSrc((materializedItem.resourcePath ?? "").replace(/\\/g, "/"));
          const response = await fetch(src);
          const blob = await response.blob();
          await writeClipboardImage(
            blob,
            materializedItem.resourcePath,
            materializedItem.contentHash,
          );
        },
      );
      return;
    }

    if (item.kind === "file") {
      try {
        item = await ensureItemMaterialized(item);
      } catch (error) {
        console.error("Unable to materialize files for paste", error);
        showToast(_t("toast.filePasteFailed"), "error");
        return;
      }
      const materializedItem = item;
      await pasteToPreviousApplication(
        materializedItem,
        {
          paste: "toast.filePasteSuccess",
          copy: "toast.fileCopySuccess",
          failed: "toast.filePasteFailed",
        },
        async () => {
          if (materializedItem.textContent && materializedItem.textContent.startsWith("[")) {
            try {
              const paths = JSON.parse(materializedItem.textContent) as string[];
              if (paths.length > 1) {
                await writeClipboardText(paths.join("\n"));
                return;
              }
            } catch {
              /* ignore */
            }
          }
          if (materializedItem.resourcePath) {
            await writeClipboardText(materializedItem.resourcePath);
          }
        },
      );
    }
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
    const item = items.find((i) => i.id === _id);
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
        await invoke("copy_file_to", { src: item.resourcePath, dst: filePath });
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
    const selectedItems = items.filter((i) => selectedIds.has(i.id));
    const text = selectedItems.map((i) => i.title).join("\n");
    void writeClipboardText(text)
      .then(() => {
        showToast(_t("toast.bulkCopySuccess", { count: selectedIds.size }), "success");
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
    const ids = items
      .filter((item) => selectedIds.has(item.id) && item.deleted)
      .map((item) => item.id);
    if (ids.length === 0) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);
    for (const id of ids) addSuppressedId(id);
    items = items.map((item) => (ids.includes(item.id) ? { ...item, deleted: false } : item));
    if (indexedItems) {
      indexedItems = indexedItems.map((item) =>
        ids.includes(item.id) ? { ...item, deleted: false } : item,
      );
    }
    selectedIds = new Set([...selectedIds].filter((id) => !ids.includes(id)));

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
        items = previousItems;
        indexedItems = previousIndexedItems;
        selectedIds = previousSelectedIds;
        statusMessage = _t("app.deleteFailed");
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function bulkPermanentDelete() {
    const ids = items
      .filter((item) => selectedIds.has(item.id) && item.deleted)
      .map((item) => item.id);
    if (ids.length === 0) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);
    const previousDetailItem = detailItem;
    for (const id of ids) addSuppressedId(id);
    removeItems(new Set(ids));
    selectedIds = new Set([...selectedIds].filter((id) => !ids.includes(id)));

    void persistBatchPermanentDelete(ids)
      .then((removed) => {
        if (removed === false) throw new Error("batch permanent delete failed");
        invalidateDeletedHistoryPagination();
        showToast(_t("toast.bulkDeleteSuccess", { count: ids.length }), "success");
      })
      .catch((error) => {
        console.error("Bulk permanent delete failed", error);
        for (const id of ids) deletedHistorySuppressedIds.delete(id);
        items = previousItems;
        indexedItems = previousIndexedItems;
        selectedIds = previousSelectedIds;
        detailItem = previousDetailItem;
        statusMessage = _t("app.deleteFailed");
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function bulkDelete() {
    const selectedItems = items.filter((item) => selectedIds.has(item.id));
    if (selectedItems.length === 0) return;

    const useRecycleBin = $generalSettings.useRecycleBin;
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
    const operationIds = new Set([...softIds, ...permanentIds, ...hardIds]);
    if (operationIds.size === 0) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);
    const previousDetailItem = detailItem;

    for (const id of softIds) deletedHistorySuppressedIds.delete(id);
    for (const id of permanentIds) addSuppressedId(id);

    items = items
      .filter((item) => !permanentIds.includes(item.id) && !hardIds.includes(item.id))
      .map((item) => (softIds.includes(item.id) ? { ...item, deleted: true } : item));
    if (indexedItems) {
      indexedItems = indexedItems
        .filter((item) => !permanentIds.includes(item.id) && !hardIds.includes(item.id))
        .map((item) => (softIds.includes(item.id) ? { ...item, deleted: true } : item));
    }
    selectedIds = new Set();
    if (detailItem && (permanentIds.includes(detailItem.id) || hardIds.includes(detailItem.id))) {
      detailItem = null;
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
      items = previousItems
        .filter((item) => !removedIds.has(item.id))
        .map((item) => (successfulSoft.has(item.id) ? { ...item, deleted: true } : item));
      if (previousIndexedItems) {
        indexedItems = previousIndexedItems
          .filter((item) => !removedIds.has(item.id))
          .map((item) => (successfulSoft.has(item.id) ? { ...item, deleted: true } : item));
      } else {
        indexedItems = null;
      }
      selectedIds = new Set([...previousSelectedIds].filter((id) => !succeededIds.has(id)));
      if (previousDetailItem && !removedIds.has(previousDetailItem.id)) {
        detailItem = previousDetailItem;
      } else if (removedIds.has(previousDetailItem?.id ?? "")) {
        detailItem = null;
      }

      if (successfulSoft.size > 0 || successfulPermanent.size > 0) {
        invalidateDeletedHistoryPagination();
      }
      if (successfulSoft.size > 0 || successfulHard.size > 0) {
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

    const current = Math.max(0, selectedIndex);
    const next = Math.min(filteredItems.length - 1, Math.max(0, current + offset));
    selectedId = filteredItems[next].id;
    const el = document.querySelector(`[data-id="${selectedId}"]`);
    if (el instanceof HTMLElement) {
      el.scrollIntoView({ block: "nearest" });
      el.focus();
    }
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
    // Don't preventDefault 鈥攍et the event continue so a single Esc
    // can clear bulk selection, close detail panel, or hide the window.
  }

  let tagAddSignal = $state(0);

  // Item-action shortcuts (Ctrl/鈱? letter) that should still operate on the
  // selected entry even when focus is in an editable target such as the search
  // box. Ctrl+A is deliberately excluded so the search box keeps its native
  // "select all text" behavior.
  function isItemActionShortcut(event: KeyboardEvent): boolean {
    if (!(event.ctrlKey || event.metaKey) || event.shiftKey) return false;
    return ["c", "d", "f", "e", "t", "s"].includes(event.key.toLowerCase());
  }

  // Native activatable controls fire their click action from the keydown
  // default behavior. When focus sits on one of them, Enter/Space must
  // activate that control 鈥攏ot the list selection below. Note: history
  // cards are divs with role="option" and deliberately keep the hijacked
  // Enter/Space activation, so role="option" is intentionally absent here;
  // the search-suggestion options are real <button> elements and are covered.
  function isActivatableKeyboardTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    return !!target.closest(
      "button, a, link, select, summary, label, [role='tab'], [role='menuitem'], [role='button'], [role='link'], [role='checkbox']",
    );
  }

  function handleGlobalKeydown(event: KeyboardEvent) {
    const editableTarget = isEditableKeyboardTarget(event.target);
    const quickCopyIndex =
      (event.metaKey || event.ctrlKey) && /^[1-9]$/.test(event.key) ? Number(event.key) - 1 : null;

    if (event.key === "Escape") {
      if (event.defaultPrevented || editingId || fullscreenFilePath) return;
      if (tagEditDialog) return;

      const detailEditorTarget =
        editableTarget &&
        event.target instanceof Element &&
        event.target.closest(".detail-panel") !== null;
      if (detailEditorTarget) return;

      if (detailItem) {
        // DetailPanel handles its own Escape
      } else if (tagFilter) {
        toggleTagFilter(tagFilter);
      } else if ("__TAURI_INTERNALS__" in window) {
        getCurrentWindow()
          .hide()
          .catch(() => {});
      }
      return;
    }

    if (
      (event.key === "/" && !editableTarget) ||
      ((event.ctrlKey || event.metaKey) && event.key === "k")
    ) {
      event.preventDefault();
      searchInputEl?.focus();
      return;
    }

    if (quickCopyIndex !== null && (!editableTarget || event.target === searchInputEl)) {
      event.preventDefault();
      const item = filteredItems[quickCopyIndex];
      if (item) {
        selectedId = item.id;
        activateSelected();
      }
      return;
    }

    const switchEditableTarget = !editableTarget || event.target === searchInputEl;
    if (!editingId && switchEditableTarget) {
      for (const [filterId, bindings] of Object.entries(filterShortcutBindings)) {
        if (bindings.some((binding) => shortcutMatchesEvent(binding, event))) {
          event.preventDefault();
          setFilter(filterId as ClipboardFilter);
          if (!editableTarget) {
            void tick().then(() => {
              const btn = document.querySelector<HTMLElement>(
                `.filters [role="tab"][aria-selected="true"]`,
              );
              btn?.focus();
            });
          }
          return;
        }
      }
    }

    if (
      navigationBindings.moveSelectionDown.some((binding) => shortcutMatchesEvent(binding, event))
    ) {
      event.preventDefault();
      moveSelection(1);
      return;
    }

    if (
      navigationBindings.moveSelectionUp.some((binding) => shortcutMatchesEvent(binding, event))
    ) {
      event.preventDefault();
      moveSelection(-1);
      return;
    }

    // Let the focused editable target (e.g. the search box) keep its own key
    // combinations (Ctrl+A to select text, Ctrl+Z/X/V, etc.). The item-action
    // shortcuts (Ctrl/鈱? C/D/F/E/T/S) are exempted so they still operate on
    // the selected entry even when the search box is focused 鈥攐therwise every
    // Ctrl+<letter> silently no-ops after a search or filter switch moves focus
    // into the search input.
    if (editableTarget && !isItemActionShortcut(event)) return;

    if (
      navigationBindings.switchFilterNext.some((binding) => shortcutMatchesEvent(binding, event))
    ) {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
        return;
      event.preventDefault();
      const idx = filters.findIndex((f) => f.id === activeFilter);
      const next = (idx + 1) % filters.length;
      setFilter(filters[next].id);
      void tick().then(() => {
        const btn = document.querySelector<HTMLElement>(
          `.filters [role="tab"][aria-selected="true"]`,
        );
        btn?.focus();
      });
      return;
    }

    if (
      navigationBindings.switchFilterPrev.some((binding) => shortcutMatchesEvent(binding, event))
    ) {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
        return;
      event.preventDefault();
      const idx = filters.findIndex((f) => f.id === activeFilter);
      const prev = (idx - 1 + filters.length) % filters.length;
      setFilter(filters[prev].id);
      void tick().then(() => {
        const btn = document.querySelector<HTMLElement>(
          `.filters [role="tab"][aria-selected="true"]`,
        );
        btn?.focus();
      });
      return;
    }

    if (event.key === "Enter") {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
        return;
      if (isActivatableKeyboardTarget(event.target)) return;
      event.preventDefault();
      activateSelected();
      return;
    }

    if (
      event.key === " " &&
      !(event.target instanceof HTMLInputElement) &&
      !(event.target instanceof HTMLTextAreaElement)
    ) {
      if (isActivatableKeyboardTarget(event.target)) return;
      event.preventDefault();
      if (selectedId) openDetail(selectedId);
      return;
    }

    if (event.key === "Backspace") {
      if (selectedIds.size > 0) {
        selectedIds = new Set();
      }
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key === "a") {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
        return;
      event.preventDefault();
      selectedIds = new Set(filteredItems.map((i) => i.id));
      return;
    }

    // Shortcuts for the focused item. These reach here only when the event
    // target is not an editable surface (the search box keeps its own Ctrl
    // combinations via the exemption in `isItemActionShortcut`), so no extra
    // input/textarea guard is needed.
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey) {
      let item = filteredItems.find((i) => i.id === selectedId);
      if (!item && filteredItems.length > 0) {
        // Heal a selection that no longer matches the active filter (e.g. after
        // a search or group switch) so the shortcut operates on a visible entry
        // instead of silently no-opping via the `if (!item) return` below.
        item = filteredItems[0];
        selectedId = item.id;
      }
      if (!item) return;

      if (event.key === "c") {
        event.preventDefault();
        if (selectedIds.size > 0) bulkCopy();
        else copyItem(selectedId);
        return;
      }
      if (event.key === "d") {
        event.preventDefault();
        if (selectedIds.size > 0) bulkDelete();
        else if (!item.favorite) deleteItem(selectedId);
        return;
      }
      if (event.key === "f") {
        event.preventDefault();
        if (selectedIds.size > 0) bulkFavorite();
        else toggleFavorite(selectedId);
        return;
      }
      if (event.key === "e") {
        event.preventDefault();
        openDetail(selectedId);
        return;
      }
      if (event.key === "t") {
        if (selectedIds.size > 0) return;
        event.preventDefault();
        tagAddSignal++;
        return;
      }
      if (event.key === "s") {
        if (item.kind === "image" || item.kind === "file") {
          event.preventDefault();
          saveItem(selectedId);
        }
        return;
      }
    }
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

  const filteredSourceApps = $derived(
    sourceAppSearch
      ? sourceApps.filter((a) => a.toLowerCase().includes(sourceAppSearch.toLowerCase()))
      : sourceApps,
  );

  $effect(() => {
    if (!sourceAppDropdownOpen) return;
    void filteredSourceApps;
    tick().then(() => {
      if (sourceAppDropdownEl) alignDropdownOptionText(sourceAppDropdownEl);
    });
  });

  $effect(() => {
    if (!dateDropdownOpen || !dateDropdownEl) return;
    const el = dateDropdownEl;
    tick().then(() => alignDropdownOptionText(el));
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
  <header
    class="search-header"
    role="presentation"
    aria-label={_t("actions.dragWindow")}
    onmousedown={(e) => {
      if (e.target === e.currentTarget) void getCurrentWindow().startDragging();
    }}
  >
    <div class="search-box">
      <input
        bind:this={searchInputEl}
        bind:value={query}
        aria-label={$generalSettings.searchPlaceholder?.trim() || _t("app.searchPlaceholder")}
        aria-autocomplete={searchAutocomplete}
        aria-controls={showSearchSuggestions ? "search-suggestions" : undefined}
        aria-expanded={showSearchSuggestions}
        aria-activedescendant={activeSearchOption
          ? `search-option-${searchOptions.indexOf(activeSearchOption)}`
          : undefined}
        autocomplete="off"
        placeholder={$generalSettings.searchPlaceholder?.trim() || _t("app.searchPlaceholder")}
        spellcheck="false"
        style={compactMode
          ? `height: ${compactSearchHeight}px; font-size: ${compactSearchFontSize}px;`
          : undefined}
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
      />
      {#if inlineSearchSuggestion}
        <span
          class="search-inline-hint"
          aria-hidden="true"
          style={compactMode ? `font-size: ${compactSearchFontSize}px;` : undefined}
        >
          <span>{normalizeSearchTerm(query)}</span>{inlineSearchSuggestionSuffix}
        </span>
      {/if}
      {#if showSearchSuggestions}
        <div
          id="search-suggestions"
          class="search-suggestions"
          role="listbox"
          aria-label={_t("search.suggestionsLabel")}
        >
          {#if visibleSearchHistory.length > 0}
            <div class="search-suggestions-heading">{_t("search.recent")}</div>
            {#each visibleSearchHistory as option, index (option.value)}
              {@const optionIndex = index}
              <button
                id={`search-option-${optionIndex}`}
                type="button"
                role="option"
                tabindex="-1"
                aria-selected={searchSuggestionIndex === optionIndex}
                class:active={searchSuggestionIndex === optionIndex}
                onmousedown={(event) => event.preventDefault()}
                onclick={() => chooseSearchOption(option.value)}
              >
                <AppIcon name="clock" size={14} />
                <span>{option.value}</span>
              </button>
            {/each}
          {/if}
          {#if visibleSearchSuggestions.length > 0}
            <div class="search-suggestions-heading">{_t("search.suggestions")}</div>
            {#each visibleSearchSuggestions as option, index (option.value)}
              {@const optionIndex = visibleSearchHistory.length + index}
              <button
                id={`search-option-${optionIndex}`}
                type="button"
                role="option"
                tabindex="-1"
                aria-selected={searchSuggestionIndex === optionIndex}
                class:active={searchSuggestionIndex === optionIndex}
                onmousedown={(event) => event.preventDefault()}
                onclick={() => chooseSearchOption(option.value)}
              >
                <AppIcon name="search" size={14} />
                <span>{option.value}</span>
              </button>
            {/each}
          {/if}
        </div>
      {/if}
      {#if query}
        <button
          class="clear-button"
          type="button"
          aria-label={_t("app.clearSearch")}
          onclick={clearSearchQuery}>脳</button
        >
      {/if}
    </div>
    <img
      class="brand-icon"
      src="{assets}/app-icon.png"
      alt="Clipboard"
      title="Clipboard"
      width="28"
      height="28"
    />
  </header>

  <div
    class="toolbar"
    role="presentation"
    aria-label={_t("actions.dragWindow")}
    onmousedown={(e) => {
      if (e.target === e.currentTarget) void getCurrentWindow().startDragging();
    }}
  >
    <div class="filters" role="tablist" aria-label={_t("filter.all")}>
      {#each filters as filter}
        <button
          type="button"
          role="tab"
          tabindex={activeFilter === filter.id ? 0 : -1}
          aria-selected={activeFilter === filter.id}
          class:active={activeFilter === filter.id}
          title={filterShortcutBindings[filter.id]?.[0]
            ? `${filter.label} (${filterShortcutBindings[filter.id]?.[0]})`
            : filter.label}
          onclick={() => setFilter(filter.id)}
        >
          <AppIcon
            name={filter.icon}
            size={16}
            filled={filter.id === "favorite" && activeFilter === filter.id}
          />
          <span>{filter.label}</span>
        </button>
      {/each}
    </div>

    <div class="filter-dropdowns">
      <!-- Source app filter -->
      <div class="dropdown-wrapper">
        <button
          type="button"
          class="filter-dropdown-btn"
          onclick={() => (sourceAppDropdownOpen = !sourceAppDropdownOpen)}
          aria-label={_t("sourceApp.all")}
          aria-haspopup="menu"
          aria-expanded={sourceAppDropdownOpen}
          title={_t("sourceApp.all")}
        >
          {#if !sourceAppFilter}
            <AppIcon name="filter" size={15} />
          {/if}
          <span class="dropdown-label">{sourceAppFilter || _t("sourceApp.all")}</span>
          {#if !sourceAppFilter}
            <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
          {/if}
        </button>
        {#if sourceAppDropdownOpen}
          <div class="dropdown-popover popover-surface" role="menu" bind:this={sourceAppDropdownEl}>
            <div
              class="dropdown-backdrop"
              onclick={() => (sourceAppDropdownOpen = false)}
              aria-hidden="true"
            ></div>
            <div class="dropdown-search">
              <AppIcon name="search" size={13} />
              <input
                type="text"
                bind:value={sourceAppSearch}
                placeholder={_t("sourceApp.placeholder")}
                autocomplete="off"
              />
            </div>
            <div class="dropdown-items">
              <button
                type="button"
                role="menuitem"
                class:selected={sourceAppFilter === ""}
                onclick={() => {
                  sourceAppFilter = "";
                  sourceAppDropdownOpen = false;
                  resetHistoryScroll();
                  void invalidateActiveHistoryPagination();
                }}><span>{_t("sourceApp.all")}</span></button
              >
              {#each filteredSourceApps as app}
                <button
                  type="button"
                  role="menuitem"
                  class:selected={sourceAppFilter === app}
                  onclick={() => {
                    sourceAppFilter = app;
                    sourceAppDropdownOpen = false;
                    resetHistoryScroll();
                    void invalidateActiveHistoryPagination();
                  }}><span>{app}</span></button
                >
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <!-- Date filter -->
      <div class="dropdown-wrapper">
        <button
          type="button"
          class="filter-dropdown-btn"
          onclick={() => (dateDropdownOpen = !dateDropdownOpen)}
          aria-label={_t("dateFilter.all")}
          aria-haspopup="menu"
          aria-expanded={dateDropdownOpen}
          title={_t("dateFilter.all")}
        >
          {#if dateFilter === "all"}
            <AppIcon name="calendar" size={15} />
          {/if}
          <span class="dropdown-label"
            >{dateFilter === "all"
              ? _t("dateFilter.all")
              : (dateFilterOptions.find((o) => o.id === dateFilter)?.label ??
                _t("dateFilter.all"))}</span
          >
          {#if dateFilter === "all"}
            <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
          {/if}
        </button>
        {#if dateDropdownOpen}
          <div class="dropdown-popover popover-surface" role="menu" bind:this={dateDropdownEl}>
            <div
              class="dropdown-backdrop"
              onclick={() => (dateDropdownOpen = false)}
              aria-hidden="true"
            ></div>
            {#each dateFilterOptions as option}
              <button
                type="button"
                role="menuitem"
                class:selected={dateFilter === option.id}
                onclick={() => {
                  dateFilter = option.id;
                  dateDropdownOpen = false;
                  resetHistoryScroll();
                  void invalidateActiveHistoryPagination();
                }}><span>{option.label}</span></button
              >
            {/each}
          </div>
        {/if}
      </div>
    </div>

    <div class="toolbar-actions">
      <button
        type="button"
        class:active={$generalSettings.alwaysOnTop}
        aria-label={_t("toolbar.pinWindow")}
        title={_t("toolbar.pinWindow")}
        onclick={() => generalSettings.updateSetting("alwaysOnTop", !$generalSettings.alwaysOnTop)}
        ><AppIcon name="window-top" size={17} /></button
      >
      <button
        type="button"
        aria-label={_t("toolbar.settings")}
        title={_t("toolbar.settings")}
        onclick={openSettings}><AppIcon name="settings" size={17} /></button
      >
    </div>
  </div>

  <div
    class="main-content"
    class:split-detail={detailDisplayMode === "split" && detailItem != null}
  >
    <section class="history-panel" aria-label={_t("app.recentRecords")}>
      <div class="section-heading"></div>

      {#if filteredItems.length > 0}
        <div
          class="history-list"
          role="listbox"
          aria-label={_t("app.recentRecords")}
          bind:this={historyListEl}
          onscroll={handleHistoryScroll}
        >
          <div
            class="virtual-container"
            style="height: {useVirtualScroll
              ? virtualList.totalHeight + 'px'
              : 'auto'}; position: {useVirtualScroll ? 'relative' : 'static'};"
          >
            {#each visiblePageItems as item, visibleIdx (item.id)}
              {#if useVirtualScroll}
                <div
                  style="position: absolute; top: {virtualList.visibleItems[visibleIdx]
                    .top}px; left: 0; right: 0;"
                >
                  <ClipboardCard
                    {item}
                    index={filteredItemIndexById.get(item.id) ?? 0}
                    now={currentTime}
                    selected={selectedIds.has(item.id) || item.id === selectedId}
                    checked={selectedIds.has(item.id)}
                    showCheckbox={false}
                    hideActions={selectedIds.size > 0 ||
                      (detailDisplayMode === "split" && detailItem != null)}
                    hideMetaRow={detailDisplayMode === "split" && detailItem != null}
                    compact={compactMode}
                    {compactPaddingTop}
                    {compactPaddingBottom}
                    {compactCardGap}
                    {compactCardBorderRadius}
                    compactCardHeight={compactCardHeightFor(item)}
                    {maxTextLines}
                    {showSecondaryText}
                    {alwaysShowActions}
                    quickCopyBadgeAlwaysVisible={quickCopyBadgeAlwaysVisible &&
                      !(detailDisplayMode === "split" && detailItem != null)}
                    onheightchange={recordCardHeight}
                    heightMeasurementKey={cardLayoutSignature(item)}
                    onselect={selectItem}
                    ontoggleSelect={toggleSelectItem}
                    ontoggleFavorite={toggleFavorite}
                    ondelete={deleteItem}
                    oncopy={copyItem}
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
                    {doubleClickPaste}
                    onrestore={restoreItem}
                    onsavetags={saveTags}
                    {tagColors}
                    ontoggleTagFilter={toggleTagFilter}
                    oneditTag={openTagEdit}
                    tagAddSignal={selectedId === item.id ? tagAddSignal : 0}
                  />
                </div>
              {:else}
                <ClipboardCard
                  {item}
                  index={filteredItemIndexById.get(item.id) ?? 0}
                  now={currentTime}
                  selected={selectedIds.has(item.id) || item.id === selectedId}
                  checked={selectedIds.has(item.id)}
                  showCheckbox={false}
                  hideActions={selectedIds.size > 0 ||
                    (detailDisplayMode === "split" && detailItem != null)}
                  hideMetaRow={detailDisplayMode === "split" && detailItem != null}
                  compact={compactMode}
                  {compactPaddingTop}
                  {compactPaddingBottom}
                  {compactCardGap}
                  {compactCardBorderRadius}
                  compactCardHeight={compactCardHeightFor(item)}
                  {maxTextLines}
                  {showSecondaryText}
                  {alwaysShowActions}
                  quickCopyBadgeAlwaysVisible={quickCopyBadgeAlwaysVisible &&
                    !(detailDisplayMode === "split" && detailItem != null)}
                  onheightchange={recordCardHeight}
                  heightMeasurementKey={cardLayoutSignature(item)}
                  onselect={selectItem}
                  ontoggleSelect={toggleSelectItem}
                  ontoggleFavorite={toggleFavorite}
                  ondelete={deleteItem}
                  oncopy={copyItem}
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
                  {doubleClickPaste}
                  onrestore={restoreItem}
                  onsavetags={saveTags}
                  {tagColors}
                  ontoggleTagFilter={toggleTagFilter}
                  oneditTag={openTagEdit}
                  tagAddSignal={selectedId === item.id ? tagAddSignal : 0}
                />
              {/if}
            {/each}
          </div>
        </div>
      {:else}
        <div class="empty-state">
          <span class="empty-icon"><AppIcon name="clipboard" size={28} /></span>
          <strong>{items.length === 0 ? _t("app.noRecords") : _t("app.noMatchRecords")}</strong>
          <p>
            {items.length === 0 ? _t("app.noRecordsHint") : _t("app.noMatchRecordsHint")}
          </p>
        </div>
      {/if}
    </section>

    {#if selectedIds.size > 0}
      <div class="bulk-bar">
        <button
          type="button"
          class="bulk-deselect"
          onclick={() => (selectedIds = new Set())}
          title={_t("bulk.deselectAll")}
        >
          <AppIcon name="x" size={14} strokeWidth={2.5} />
          <span>{selectedIds.size}</span>
        </button>
        <div class="bulk-actions">
          <button type="button" onclick={bulkCopy}>
            <AppIcon name="copy" size={14} />
            <span>{_t("bulk.copyN", { count: selectedIds.size })}</span>
          </button>
          {#if selectedActiveCount > 0 && activeFilter !== "favorite"}
            <button type="button" class="danger" onclick={bulkDelete}>
              <AppIcon name="trash" size={14} />
              <span>{_t("bulk.deleteN", { count: selectedActiveCount })}</span>
            </button>
          {/if}
          <button type="button" onclick={bulkFavorite}>
            <AppIcon name="star" size={14} />
            <span
              >{allSelectedFavorites
                ? _t("bulk.unfavoriteN", { count: selectedIds.size })
                : _t("bulk.favoriteN", { count: selectedIds.size })}</span
            >
          </button>
          {#if selectedDeletedCount > 0}
            <button type="button" onclick={bulkRestore}>
              <AppIcon name="restore" size={14} />
              <span>{_t("bulk.restoreN", { count: selectedDeletedCount })}</span>
            </button>
            <button type="button" class="danger" onclick={bulkPermanentDelete}>
              <AppIcon name="trash" size={14} />
              <span>{_t("bulk.permanentDeleteN", { count: selectedDeletedCount })}</span>
            </button>
          {/if}
        </div>
      </div>
    {/if}

    {#if detailDisplayMode === "split" && detailItem}
      <DetailPanel
        mode="split"
        item={detailItem}
        onclose={closeDetail}
        oncopy={copyItem}
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

  <footer class="status-bar" role="status" aria-live="polite">
    <span class="status-left">
      <span class="result-count">{resultSummary}</span>
      <span class="runtime-status"><i></i>{runtimeLabel}</span>
    </span>
    <span class="status-msg">{statusMessage}</span>
    <span class="shortcut-hints"><kbd>Alt</kbd><b>+</b><kbd>V</kbd> {_t("app.shortcutHint")}</span>
  </footer>
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

  .app-shell.split-detail > .search-header,
  .app-shell.split-detail > .toolbar,
  .app-shell.split-detail > .main-content,
  .app-shell.split-detail > .status-bar {
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

  .main-content.split-detail > *:last-child {
    grid-column: 2;
    grid-row: 1 / -1;
  }

  :global(.app-shell.compact .search-header) {
    padding: 6px 16px 2px;
    gap: 6px;
  }

  :global(.app-shell.compact .search-box input) {
    font-size: 12px;
  }

  :global(.app-shell.compact .toolbar) {
    padding: 2px 8px 8px;
  }

  :global(.app-shell.compact .toolbar-actions button) {
    width: 28px;
    height: 28px;
  }

  :global(.app-shell.compact .filters button) {
    padding: 3px 9px;
    font-size: 12px;
  }

  :global(.app-shell.compact .history-list) {
    padding: 0 4px 6px;
  }

  .search-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 20px 4px;
    border-bottom: none;
  }

  .search-box {
    position: relative;
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    color: var(--text-muted);
  }

  .search-suggestions {
    position: absolute;
    z-index: 110;
    top: calc(100% + 8px);
    left: 0;
    right: 0;
    max-height: min(280px, calc(100vh - 100px));
    padding: 6px 0;
    overflow-y: auto;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--surface-bg);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.48);
  }

  .search-suggestions-heading {
    padding: 5px 12px 3px;
    color: var(--text-faint);
    font-size: 10px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .search-suggestions button {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 6px 12px;
    border: 0;
    color: var(--text-secondary);
    background: transparent;
    text-align: left;
    cursor: pointer;
    font-size: 12px;
  }

  .search-suggestions button :global(svg) {
    flex: 0 0 auto;
    color: var(--text-muted);
  }

  .search-suggestions button span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .search-suggestions button:hover,
  .search-suggestions button.active {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .search-suggestions button.active :global(svg) {
    color: var(--selection-color);
  }

  .search-box input {
    flex: 1;
    min-width: 0;
    padding: 2px 0 0px;
    border: 0;
    outline: 0;
    color: var(--text-primary);
    background: transparent;
    font-size: clamp(17px, 3vw, 21px);
    font-weight: 350;
    letter-spacing: -0.02em;
  }

  .search-box input::placeholder {
    color: var(--placeholder-color);
    opacity: 1;
  }

  .search-inline-hint {
    position: absolute;
    top: 50%;
    left: 0;
    z-index: 0;
    overflow: hidden;
    max-width: calc(100% - 42px);
    color: var(--text-faint);
    pointer-events: none;
    transform: translateY(-50%);
    white-space: pre;
    font-size: clamp(17px, 3vw, 21px);
    font-weight: 350;
    letter-spacing: -0.02em;
  }

  .search-inline-hint span {
    visibility: hidden;
  }

  .clear-button {
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    color: var(--text-muted);
    background: var(--hover-bg);
    cursor: pointer;
  }

  .brand-icon {
    width: 28px;
    height: 28px;
    border-radius: 8px;
    object-fit: contain;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 2px 20px 4px 4px;
  }

  .filters,
  .filter-dropdowns,
  .toolbar-actions {
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .filters {
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .filters::-webkit-scrollbar {
    display: none;
  }

  .filters button,
  .toolbar-actions button {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    border: 0;
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
  }

  .filters button {
    gap: 5px;
    height: 31px;
    padding: 0 9px;
    border-radius: 6px;
    font-size: 12px;
  }

  .filters button:hover,
  .filters button.active {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .filters button.active:first-child {
    color: var(--selection-color);
  }

  /* Group tabs use a roving tabindex, so the focused tab is always the active
     one; its .active background already indicates state. Suppress the focus
     outline so mouse-click and keyboard selection look identical (no ring). */
  .filters button:focus-visible {
    outline: none;
  }

  .filters button:nth-child(2) :global(svg) {
    color: var(--warning-color);
  }
  .filters button:nth-child(3) :global(svg) {
    color: #a8b7c9;
  }
  .filters button:nth-child(4) :global(svg) {
    color: #6bbfc5;
  }
  .filters button:nth-child(5) :global(svg) {
    color: #8fc7de;
  }
  .filters button:nth-child(6) :global(svg) {
    color: #f5c842;
  }

  .filter-dropdowns {
    gap: 4px;
  }

  .dropdown-wrapper {
    position: relative;
  }

  .filter-dropdown-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    height: 29px;
    width: 80px;
    padding: 0 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
    font-size: 11.5px;
    white-space: nowrap;
    transition:
      color 100ms ease,
      border-color 100ms ease,
      background 100ms ease;
  }

  .filter-dropdown-btn:hover {
    color: var(--text-secondary);
    border-color: var(--border-color);
    background: var(--hover-bg);
  }

  .dropdown-label {
    min-width: 0;
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dropdown-popover {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    min-width: 100%;
    max-width: 150px;
    overflow: hidden;
  }

  .dropdown-backdrop {
    position: fixed;
    inset: 0;
    z-index: -1;
  }

  .dropdown-search {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text-faint);
  }

  .dropdown-search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    color: var(--text-primary);
    background: transparent;
    font-size: 12px;
  }

  .dropdown-search input::placeholder {
    color: var(--placeholder-color);
  }

  .dropdown-items {
    max-height: 180px;
    overflow-y: auto;
  }

  .toolbar-actions {
    flex: 0 0 auto;
  }

  .toolbar-actions button {
    width: 29px;
    height: 29px;
    padding: 0;
    border-radius: 6px;
    color: var(--text-faint);
  }

  .toolbar-actions button:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .toolbar-actions button.active {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .history-panel {
    display: flex;
    min-height: 0;
    flex-direction: column;
    overflow: hidden;
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    color: var(--text-muted);
    font-size: 11.5px;
  }

  .result-count {
    color: var(--text-faint);
  }

  .runtime-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .runtime-status i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--success-color);
    box-shadow: 0 0 8px color-mix(in srgb, var(--success-color) 40%, transparent);
  }

  .history-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 7px 18px;
  }

  .virtual-container {
    width: 100%;
  }

  .empty-state {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    color: var(--text-muted);
    text-align: center;
  }

  .empty-icon {
    display: inline-flex;
    margin-bottom: 12px;
    color: var(--text-faint);
  }

  .empty-state strong {
    color: var(--text-secondary);
    font-size: 14px;
  }

  .empty-state p {
    margin: 6px 0;
    font-size: 12px;
  }

  .bulk-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 14px;
    border-top: 1px solid var(--border-subtle);
    background: var(--input-bg);
  }

  .bulk-deselect {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border: 1px solid var(--border-color);
    border-radius: 14px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
    font-size: 11.5px;
    transition: color 100ms ease;
  }

  .bulk-deselect:hover {
    color: var(--text-secondary);
  }

  .bulk-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .bulk-actions button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    background: var(--card-bg);
    cursor: pointer;
    font-size: 11.5px;
    font-weight: 500;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .bulk-actions button:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .bulk-actions button.danger {
    border-color: color-mix(in srgb, var(--danger-color) 30%, transparent);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
  }

  .bulk-actions button.danger:hover {
    border-color: color-mix(in srgb, var(--danger-color) 50%, transparent);
    background: color-mix(in srgb, var(--danger-color) 10%, transparent);
    color: color-mix(in srgb, var(--danger-color) 85%, white);
  }

  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    min-height: 34px;
    padding: 7px 14px;
    border-top: 1px solid var(--border-subtle);
    color: var(--text-faint);
    background: var(--statusbar-bg);
    font-size: 11.5px;
    overflow: hidden;
  }

  .status-left {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-shrink: 0;
  }

  .status-msg {
    flex: 1 1 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .shortcut-hints {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 4px;
  }

  .shortcut-hints kbd {
    padding: 1px 5px;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-muted);
    background: var(--hover-bg);
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.4);
    font: inherit;
  }

  .shortcut-hints b {
    font-weight: 400;
  }

  @media (max-width: 660px) {
    .filter-dropdowns {
      display: none;
    }
    .toolbar-actions {
      display: none;
    }
    .status-bar > span:first-child {
      display: none;
    }
    .status-bar {
      justify-content: flex-end;
    }
  }
</style>
