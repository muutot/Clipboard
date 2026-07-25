<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import ClipboardCard from "$lib/components/ClipboardCard.svelte";
  import DetailPanel from "$lib/components/DetailPanel.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import { demoClipboardItems } from "$lib/data/demo-items";
  import {
    loadClipboardHistory,
    loadDeletedClipboardHistory,
    persistDelete,
    persistRestore,
    persistBatchRestore,
    persistPermanentDelete,
    persistBatchPermanentDelete,
    persistFavorite,
    persistBatchFavorite,
    persistBatchDelete,
    searchClipboardHistory,
    listSourceApplications,
    formatTextLength,
    formatSizeSimple,
  } from "$lib/services/clipboard";
  import { getRuntimeInfo, isTauriRuntime } from "$lib/services/runtime";
  import { showToast } from "$lib/services/toast";
  import type { ClipboardFilter, ClipboardItem, WindowPosition } from "$lib/types/clipboard";
  import type { IconName } from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { assets } from "$app/paths";
  import {
    createVirtualList,
    editHeight,
    itemHeight,
    type VirtualScrollConfig,
  } from "$lib/utils/virtual-scroll";
  import { parseDateQuery } from "$lib/utils/date-query";
  import { listen } from "@tauri-apps/api/event";
  import type { PersistedClipboardItem } from "$lib/types/clipboard";
  import {
    generalSettings,
    restoreWindowPosition,
    saveWindowPosition,
  } from "$lib/services/settings";
  import { iconsDir } from "$lib/services/paths";
  import { getStorageStatus } from "$lib/services/storage";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  const filters = $derived([
    { id: "all" as ClipboardFilter, label: _t("filter.all"), icon: "grid" as IconName },
    { id: "text" as ClipboardFilter, label: _t("filter.text"), icon: "text" as IconName },
    { id: "link" as ClipboardFilter, label: _t("filter.link"), icon: "link" as IconName },
    { id: "image" as ClipboardFilter, label: _t("filter.image"), icon: "image" as IconName },
    { id: "file" as ClipboardFilter, label: _t("filter.file"), icon: "file" as IconName },
    { id: "favorite" as ClipboardFilter, label: _t("filter.favorite"), icon: "star" as IconName },
    ...($generalSettings.useRecycleBin
      ? [
          {
            id: "deleted" as ClipboardFilter,
            label: _t("filter.deleted"),
            icon: "trash" as IconName,
          },
        ]
      : []),
  ]);

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

  let items = $state<ClipboardItem[]>(demoClipboardItems.map((item) => ({ ...item })));
  let deletedHistoryLoaded = $state(false);
  let deletedHistoryLoading = $state(false);
  let deletedHistoryOffset = $state(0);
  let deletedHistoryHasMore = $state(true);
  let deletedHistoryRequestId = 0;
  let query = $state("");
  let activeFilter = $state<ClipboardFilter>("all");
  let selectedId = $state(demoClipboardItems[0]?.id ?? "");
  let currentTime = $state(Date.now());
  let runtimeLabel = $state(_t("app.browserPreview"));
  let statusMessage = $state(_t("app.activateHint"));
  let lastBackspaceAt = $state(0);
  let indexedItems = $state<ClipboardItem[] | null>(null);
  let indexedQuery = $state("");
  let searchPending = $state(false);
  let searchRequestId = 0;

  let dateFilter = $state<string>("all");
  let sourceAppFilter = $state("");
  let sourceApps = $state<string[]>([]);
  let sourceAppSearch = $state("");
  let sourceAppDropdownOpen = $state(false);
  let dateDropdownOpen = $state(false);

  let regexMode = $state(false);
  let regexError = $state("");

  let detailItem = $state<ClipboardItem | null>(null);

  let selectedIds = $state<Set<string>>(new Set());
  let lastClickedIndex = $state(-1);

  let historyListEl = $state<HTMLElement | null>(null);
  let scrollTop = $state(0);
  let containerHeight = $state(0);

  // --- Date range resolution ---

  function resolveDateRange(filter: string): { from: number; to: number } | null {
    const now = Date.now();
    const dayMs = 24 * 60 * 60 * 1_000;

    const startOfDay = (ts: number) => {
      const d = new Date(ts);
      d.setHours(0, 0, 0, 0);
      return d.getTime();
    };
    const endOfDay = (ts: number) => {
      const d = new Date(ts);
      d.setHours(23, 59, 59, 999);
      return d.getTime();
    };
    const startOfWeek = (ts: number) => {
      const d = new Date(ts);
      const day = d.getDay();
      const diff = d.getDate() - day + (day === 0 ? -6 : 1);
      d.setDate(diff);
      d.setHours(0, 0, 0, 0);
      return d.getTime();
    };

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
    // in the record text (e.g. "昨天" should not be required in the title).
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
              ? item.favorite
              : item.kind === activeFilter;

      if (!matchesFilter) return false;

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

      if (regexMode) {
        try {
          const re = new RegExp(normalizedQuery, "i");
          regexError = "";
          return re.test([item.title, item.preview, item.textContent, item.sourceApp].join(" "));
        } catch {
          regexError = _t("search.regexError");
          return false;
        }
      }

      const searchableText = [item.title, item.preview, item.textContent, item.sourceApp]
        .join(" ")
        .toLocaleLowerCase();

      return keywords.every((keyword) => searchableText.includes(keyword));
    });
  });

  const selectedIndex = $derived(filteredItems.findIndex((item) => item.id === selectedId));
  const selectedDeletedCount = $derived(
    items.filter((item) => selectedIds.has(item.id) && !!item.deleted).length,
  );
  const selectedActiveCount = $derived(
    items.filter((item) => selectedIds.has(item.id) && !item.deleted).length,
  );
  const resultSummary = $derived(
    searchPending
      ? _t("status.searching")
      : _t("status.recordCount", { count: filteredItems.length }),
  );

  // --- Virtual scrolling ---

  const compactMode = $derived($generalSettings.compactMode);
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

  const virtualList = $derived(
    createVirtualList(
      filteredItems.length,
      containerHeight,
      scrollTop,
      VIRTUAL_SCROLL_CONFIG,
      filteredItems.map((i) =>
        editingId === i.id
          ? editHeight((i.textContent || "").split("\n").length, !!i.customTitle, compactCardGap)
          : itemHeight(
              i.kind,
              i.kind !== "image" && i.kind !== "file" && !!i.preview,
              compactMode,
              compactText,
              compactTallText,
              compactImage,
              compactCardGap,
              showSecondaryText,
              !!i.customTitle,
              compactCustomTitle,
            ),
      ),
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
    const requestId = ++searchRequestId;
    indexedItems = null;
    indexedQuery = "";

    if (!requestedQuery || activeFilter === "deleted") {
      searchPending = false;
      return;
    }

    // Regex and natural-language date queries are intentionally evaluated
    // against the full local snapshot. The Tantivy endpoint only implements
    // tokenized AND search, so replacing the candidates here would silently
    // disable those two modes in the desktop runtime.
    if (regexMode || parseDateQuery(requestedQuery)) {
      searchPending = false;
      return;
    }

    searchPending = true;
    const timer = window.setTimeout(() => {
      void searchClipboardHistory(requestedQuery, 500)
        .then((results) => {
          if (requestId !== searchRequestId || results === null) return;
          indexedItems = results;
          indexedQuery = requestedQuery;
          statusMessage = _t("app.searchHitSummary", { count: results.length });
        })
        .catch((error) => {
          if (requestId !== searchRequestId) return;
          console.error("Unable to search clipboard history", error);
          statusMessage = _t("app.searchFailed");
        })
        .finally(() => {
          if (requestId === searchRequestId) searchPending = false;
        });
    }, 120);

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
    if (!$generalSettings.useRecycleBin && activeFilter === "deleted") {
      activeFilter = "all";
      selectedIds = new Set();
    }
  });

  onMount(() => {
    const clock = window.setInterval(() => {
      currentTime = Date.now();
    }, 30_000);

    void getStorageStatus().then((status) => {
      if (status) {
        iconsDir.set(status.iconsDir);
      }
    });

    void getRuntimeInfo().then((runtime) => {
      if (runtime) {
        runtimeLabel = `${runtime.operatingSystem} / ${runtime.architecture} \u00b7 ${_t("app.coreConnected")}`;
      }
    });

    void loadClipboardHistory()
      .then((storedItems) => {
        if (storedItems === null) return;

        items = storedItems;
        selectedId = storedItems[0]?.id ?? "";
      })
      .catch((error) => {
        console.error("Unable to load clipboard history", error);
        statusMessage = _t("app.databaseLoadFailed");
      });

    void listSourceApplications().then((apps) => {
      if (apps) sourceApps = apps;
    });

    const unlisten = listen<PersistedClipboardItem>("clipboard-item-added", (event) => {
      const record = event.payload;
      const sourceApp = record.sourceApp?.trim() || "Clipboard";
      const newItem: ClipboardItem = {
        id: record.id,
        kind: record.kind,
        title: record.title,
        preview:
          record.textContent && record.textContent !== record.title ? record.textContent : "",
        sourceApp,
        sourceTone: sourceApp.includes("codex")
          ? "violet"
          : sourceApp.includes("Chrome") || sourceApp.includes("Edge")
            ? "blue"
            : sourceApp === "Clipboard"
              ? "neutral"
              : "red",
        sizeLabel:
          record.kind === "text" || record.kind === "link"
            ? formatTextLength(record.textContent?.length || record.title.length)
            : formatSizeSimple(record),
        createdAt: record.createdAtMs,
        favorite: record.isFavorite,
        fileName:
          record.kind === "file"
            ? record.resourcePath?.split(/[\\/]/).pop() || record.title
            : undefined,
        previewPath: record.previewPath,
        resourcePath: record.resourcePath,
        textContent: record.textContent,
        iconPath: record.iconPath,
        metadataJson: record.metadataJson,
      };
      const existingIdx = items.findIndex((i) => i.id === newItem.id);
      if (existingIdx >= 0) {
        items[existingIdx] = newItem;
        items = items;
      } else {
        items = [newItem, ...items];
        selectedId = newItem.id;
      }
    });

    const appWindow = isTauriRuntime() ? getCurrentWindow() : null;
    let restoreAttempted = false;
    let previousRememberWindowPosition = false;
    let boundsTimer: number | undefined;
    let boundsWriteInFlight: Promise<void> | undefined;
    let pendingBounds: WindowPosition | undefined;

    function readLegacyWindowPosition(): { x: number; y: number } | null {
      try {
        const raw = localStorage.getItem("windowPosition");
        if (!raw) return null;
        const parsed = JSON.parse(raw) as { x?: unknown; y?: unknown };
        if (
          typeof parsed.x !== "number" ||
          !Number.isFinite(parsed.x) ||
          typeof parsed.y !== "number" ||
          !Number.isFinite(parsed.y)
        ) {
          return null;
        }
        return { x: Math.round(parsed.x), y: Math.round(parsed.y) };
      } catch {
        return null;
      }
    }

    async function captureWindowBounds(): Promise<WindowPosition> {
      if (!appWindow) throw new Error("window bounds are only available in Tauri");
      const [position, size] = await Promise.all([
        appWindow.outerPosition(),
        appWindow.outerSize(),
      ]);
      return {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
      };
    }

    function drainWindowBoundsWrites(): Promise<void> {
      if (!appWindow) return Promise.resolve();
      if (boundsWriteInFlight) return boundsWriteInFlight;

      boundsWriteInFlight = (async () => {
        while (pendingBounds) {
          if (!$generalSettings.rememberWindowPosition) {
            pendingBounds = undefined;
            return;
          }
          const bounds = pendingBounds;
          pendingBounds = undefined;
          try {
            await saveWindowPosition(bounds);
          } catch (error) {
            pendingBounds = bounds;
            throw error;
          }
        }
      })().finally(() => {
        boundsWriteInFlight = undefined;
      });
      return boundsWriteInFlight;
    }

    function scheduleWindowBoundsSave() {
      if (!appWindow || !$generalSettings.rememberWindowPosition) return;
      if (boundsTimer !== undefined) window.clearTimeout(boundsTimer);
      boundsTimer = window.setTimeout(() => {
        boundsTimer = undefined;
        void captureWindowBounds()
          .then((bounds) => {
            if (!$generalSettings.rememberWindowPosition) return;
            pendingBounds = bounds;
            return drainWindowBoundsWrites();
          })
          .catch(() => {});
      }, 120);
    }

    async function flushWindowBounds() {
      if (!appWindow || !$generalSettings.rememberWindowPosition) return;
      if (boundsTimer !== undefined) {
        window.clearTimeout(boundsTimer);
        boundsTimer = undefined;
        try {
          pendingBounds = await captureWindowBounds();
        } catch {
          return;
        }
      }
      await drainWindowBoundsWrites().catch(() => {});
    }

    async function restoreSavedWindowBounds() {
      if (!appWindow || !$generalSettings.rememberWindowPosition || restoreAttempted) return;
      restoreAttempted = true;
      try {
        const saved = await restoreWindowPosition();
        if (!$generalSettings.rememberWindowPosition) return;
        if (saved && saved.width > 0 && saved.height > 0) {
          await appWindow.setSize(new PhysicalSize(saved.width, saved.height));
          await appWindow.setPosition(new PhysicalPosition(saved.x, saved.y));
          try {
            localStorage.removeItem("windowPosition");
          } catch {}
          return;
        }

        // Migrate the old x/y-only browser storage once the backend has no
        // bounds yet. The current native size supplies the missing dimensions.
        const legacy = readLegacyWindowPosition();
        if (!legacy) return;
        const size = await appWindow.outerSize();
        const migrated: WindowPosition = {
          x: legacy.x,
          y: legacy.y,
          width: size.width,
          height: size.height,
        };
        if (!$generalSettings.rememberWindowPosition) return;
        await saveWindowPosition(migrated);
        if (!$generalSettings.rememberWindowPosition) return;
        await appWindow.setPosition(new PhysicalPosition(migrated.x, migrated.y));
        try {
          localStorage.removeItem("windowPosition");
        } catch {}
      } catch {
        // Keep the legacy key if restoring or migrating failed; retry on the
        // next transition to rememberWindowPosition=true.
        restoreAttempted = false;
      }
    }

    function applySettings(s: typeof $generalSettings) {
      const r = document.documentElement.style;
      if (s.fontSizes) {
        r.fontSize = `${s.fontSizes.base}px`;
        r.setProperty("--font-size-base", `${s.fontSizes.base}px`);
        r.setProperty("--font-size-secondary", `${s.fontSizes.secondary}px`);
        r.setProperty("--font-size-tiny", `${s.fontSizes.tiny}px`);
        r.setProperty("--font-size-cardTitle", `${s.fontSizes.cardTitle}px`);
        r.setProperty("--font-size-cardPreview", `${s.fontSizes.cardPreview}px`);
      }
      if (s.display) {
        r.setProperty("--show-secondary", s.display.showSecondaryText ? "block" : "none");
      }
      if (appWindow) {
        appWindow.setAlwaysOnTop(s.alwaysOnTop).catch(() => {});
        if (!s.rememberWindowPosition) {
          restoreAttempted = false;
        } else if (!previousRememberWindowPosition) {
          void restoreSavedWindowBounds();
        }
      }
      previousRememberWindowPosition = s.rememberWindowPosition;
      const shell = document.querySelector(".app-shell");
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
      display: { showSecondaryText: boolean };
    }>("settings-font-changed", (event) => {
      const { base, secondary, tiny, cardTitle, cardPreview } = event.payload.fontSizes || {};
      const display = event.payload.display;
      if (base !== undefined) {
        document.documentElement.style.fontSize = `${base}px`;
        document.documentElement.style.setProperty("--font-size-base", `${base}px`);
        document.documentElement.style.setProperty("--font-size-secondary", `${secondary}px`);
        document.documentElement.style.setProperty("--font-size-tiny", `${tiny}px`);
        document.documentElement.style.setProperty("--font-size-cardTitle", `${cardTitle}px`);
        document.documentElement.style.setProperty("--font-size-cardPreview", `${cardPreview}px`);
      }
      if (display) {
        document.documentElement.style.setProperty(
          "--show-secondary",
          display.showSecondaryText ? "block" : "none",
        );
      }
    });

    let unlistenMove: (() => void) | undefined;
    let unlistenResize: (() => void) | undefined;
    if (appWindow) {
      appWindow
        .onMoved(() => {
          scheduleWindowBoundsSave();
        })
        .then((fn) => {
          unlistenMove = fn;
        });
      appWindow
        .onResized(() => {
          scheduleWindowBoundsSave();
        })
        .then((fn) => {
          unlistenResize = fn;
        });
    }

    return () => {
      window.clearInterval(clock);
      unlisten.then((fn) => fn());
      unsubSettings();
      if (unlistenMove) unlistenMove();
      if (unlistenResize) unlistenResize();
      void flushWindowBounds();
    };
  });

  function mergeDeletedHistoryPage(page: ClipboardItem[]) {
    const incoming = new Map(page.map((item) => [item.id, item]));
    const merged = items.map((item) => {
      const persisted = incoming.get(item.id);
      // A local restore/delete mutation may have landed while the page was
      // in flight. Do not let a stale recycle-bin response undo a restore.
      if (!persisted || !item.deleted) return item;
      return { ...item, ...persisted, deleted: true };
    });
    const existingIds = new Set(merged.map((item) => item.id));
    for (const item of page) {
      if (!existingIds.has(item.id)) merged.push({ ...item, deleted: true });
    }
    items = merged;
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

  async function openSettings() {
    if ("__TAURI_INTERNALS__" in window) {
      const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
      const existing = await WebviewWindow.getByLabel("settings");
      if (existing) {
        existing.setFocus();
        return;
      }
      new WebviewWindow("settings", {
        url: "/settings",
        title: "Settings",
        width: 760,
        height: 640,
        minWidth: 560,
        minHeight: 480,
        center: true,
        resizable: true,
        decorations: false,
      });
    }
  }

  function setFilter(filter: ClipboardFilter) {
    if (filter === "deleted" && !$generalSettings.useRecycleBin) return;
    activeFilter = filter;
    selectedIds = new Set();
    indexedItems = null;
    indexedQuery = "";
    if (filter === "deleted" && !deletedHistoryLoaded) {
      void loadDeletedHistoryPage();
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
    items = items.map((item) => (item.id === id ? { ...item, favorite: nextFavorite } : item));
    if (indexedItems) {
      indexedItems = indexedItems.map((item) =>
        item.id === id ? { ...item, favorite: nextFavorite } : item,
      );
    }

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
        items = items.map((item) =>
          item.id === id ? { ...item, favorite: original.favorite } : item,
        );
        if (indexedItems) {
          indexedItems = indexedItems.map((item) =>
            item.id === id ? { ...item, favorite: original.favorite } : item,
          );
        }
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
      permanentlyDeleteItem(id);
      return;
    }

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);

    // Soft delete: mark as deleted, don't remove from list
    items = items.map((item) => (item.id === id ? { ...item, deleted: true } : item));
    if (indexedItems) {
      indexedItems = indexedItems.map((item) =>
        item.id === id ? { ...item, deleted: true } : item,
      );
    }
    selectedIds = new Set([...selectedIds].filter((x) => x !== id));

    void persistDelete(id)
      .then((removed) => {
        if (removed === false) throw new Error("record not found");
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

    items = items.filter((item) => item.id !== id);
    if (indexedItems) indexedItems = indexedItems.filter((item) => item.id !== id);
    selectedIds = new Set([...selectedIds].filter((x) => x !== id));

    void persistPermanentDelete(id)
      .then((removed) => {
        if (removed === false) throw new Error("record not found");
        showToast(_t("toast.deleteSuccess"), "success");
      })
      .catch((error) => {
        console.error("Unable to permanently delete clipboard item", error);
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

    items = items.map((item) => (item.id === id ? { ...item, deleted: false } : item));
    if (indexedItems) {
      indexedItems = indexedItems.map((item) =>
        item.id === id ? { ...item, deleted: false } : item,
      );
    }
    void persistRestore(id)
      .then((restored) => {
        if (restored === false) throw new Error("record not found");
        const translated = _t("toast.restoreSuccess");
        showToast(translated === "toast.restoreSuccess" ? "Restored" : translated, "success");
      })
      .catch((error) => {
        console.error("Unable to restore clipboard item", error);
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
    const item = items.find((i) => i.id === id);
    if (!item) return;

    if ($generalSettings.pinCopiedToTop) moveToTop(id);

    if (item.kind === "image" && item.resourcePath) {
      try {
        const src = convertFileSrc(item.resourcePath.replace(/\\/g, "/"));
        const response = await fetch(src);
        const blob = await response.blob();
        await navigator.clipboard.write([new ClipboardItem({ [blob.type || "image/png"]: blob })]);
        statusMessage = _t("app.copiedItem", { title: item.title.split("\n")[0] });
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
            invoke("mark_self_triggered", { text: paths.join("\n") }).catch(() => {});
            await navigator.clipboard.writeText(paths.join("\n"));
            statusMessage = _t("app.copiedItem", { title: item.title.split("\n")[0] });
            showToast(_t("toast.copySuccess"), "success");
            return;
          }
        } catch {
          /* ignore */
        }
      }
      if (item.resourcePath) {
        try {
          invoke("mark_self_triggered", { text: item.resourcePath }).catch(() => {});
          await navigator.clipboard.writeText(item.resourcePath);
          statusMessage = _t("app.copiedItem", {
            title: item.fileName || item.title.split("\n")[0],
          });
          showToast(_t("toast.copySuccess"), "success");
        } catch {
          showToast(_t("toast.copyFailed"), "error");
        }
      }
      return;
    }

    void navigator.clipboard
      .writeText(item.textContent || item.title)
      .then(() => {
        invoke("mark_self_triggered", { text: item.textContent || item.title }).catch(() => {});
        statusMessage = _t("app.copiedItem", { title: item.title.split("\n")[0] });
        showToast(_t("toast.copySuccess"), "success");
      })
      .catch(() => {
        showToast(_t("toast.copyFailed"), "error");
      });
  }

  function openDetail(id: string) {
    const item = items.find((i) => i.id === id);
    if (item) detailItem = item;
  }

  function closeDetail() {
    detailItem = null;
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
    const newTitle = isText ? (item.customTitle ? item.title : content.slice(0, 200)) : content;
    const newTextContent = isText ? content : (item?.textContent ?? null);
    const newPreview = isText && content.length > 200 ? content.slice(200) : (item?.preview ?? "");

    if (isMedia && content) {
      try {
        const updated = await invoke<ClipboardItem>("rename_item", { id, newName: content });
        items = items.map((item) =>
          item.id === id
            ? {
                ...item,
                title: updated.title,
                resourcePath: updated.resourcePath,
                previewPath: updated.previewPath,
              }
            : item,
        );
        if (detailItem?.id === id) {
          detailItem = {
            ...detailItem,
            title: updated.title,
            resourcePath: updated.resourcePath,
            previewPath: updated.previewPath,
          };
        }
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

    items = items.map((item) =>
      item.id === id
        ? {
            ...item,
            textContent: newTextContent,
            preview: newPreview,
            ...(item.customTitle ? {} : { title: newTitle }),
          }
        : item,
    );
    if (detailItem?.id === id) {
      detailItem = {
        ...detailItem,
        textContent: newTextContent,
        preview: newPreview,
        ...(detailItem.customTitle ? {} : { title: newTitle }),
      };
    }
    if (indexedItems) {
      indexedItems = indexedItems.map((item) =>
        item.id === id
          ? {
              ...item,
              textContent: newTextContent,
              preview: newPreview,
              ...(item.customTitle ? {} : { title: newTitle }),
            }
          : item,
      );
    }
    editingId = null;
    showToast(_t("toast.editSaved"), "success");
    return true;
  }

  function cancelEdit(_id: string) {
    editingId = null;
  }

  function renameTitle(id: string, title: string) {
    items = items.map((item) => (item.id === id ? { ...item, title, customTitle: true } : item));
    if (detailItem?.id === id) detailItem = { ...detailItem, title, customTitle: true };
    if (indexedItems) {
      indexedItems = indexedItems.map((item) =>
        item.id === id ? { ...item, title, customTitle: true } : item,
      );
    }
    invoke("rename_item", { id, newName: title }).catch(() => {});
  }

  function plainPaste(_id: string) {
    const item = items.find((i) => i.id === _id);
    if (!item) return;
    if ($generalSettings.pinCopiedToTop) moveToTop(_id);
    const text = item.textContent || item.title;
    invoke("mark_self_triggered", { text }).catch(() => {});
    void navigator.clipboard
      .writeText(text)
      .then(() => {
        showToast(_t("toast.plainPasteSuccess"), "success");
      })
      .catch(() => {
        showToast(_t("toast.copyFailed"), "error");
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
    const item = items.find((i) => i.id === _id);
    if (!item) return;
    const name = item.fileName ?? item.title;
    void navigator.clipboard
      .writeText(name)
      .then(() => {
        showToast(_t("toast.copySuccess"), "success");
      })
      .catch(() => {
        showToast(_t("toast.copyFailed"), "error");
      });
  }

  // --- Bulk operations ---

  function bulkCopy() {
    const selectedItems = items.filter((i) => selectedIds.has(i.id));
    const text = selectedItems.map((i) => i.title).join("\n");
    invoke("mark_self_triggered", { text }).catch(() => {});
    void navigator.clipboard
      .writeText(text)
      .then(() => {
        showToast(_t("toast.bulkCopySuccess", { count: selectedIds.size }), "success");
      })
      .catch(() => {
        showToast(_t("toast.copyFailed"), "error");
      });
  }

  function bulkFavorite() {
    const ids = [...selectedIds];
    const previousItems = items;
    const previousIndexedItems = indexedItems;
    items = items.map((item) => (selectedIds.has(item.id) ? { ...item, favorite: true } : item));
    if (indexedItems) {
      indexedItems = indexedItems.map((item) =>
        selectedIds.has(item.id) ? { ...item, favorite: true } : item,
      );
    }
    void persistBatchFavorite(ids, true)
      .then((updated) => {
        if (updated === false) throw new Error("batch favorite failed");
        showToast(_t("toast.bulkFavoriteSuccess", { count: ids.length }), "success");
        selectedIds = new Set();
      })
      .catch((error) => {
        console.error("Bulk favorite failed", error);
        items = previousItems;
        indexedItems = previousIndexedItems;
        statusMessage = _t("app.favoriteFailed");
        showToast(_t("app.favoriteFailed"), "error");
      });
  }

  function bulkRestore() {
    const ids = items.filter((item) => selectedIds.has(item.id) && item.deleted).map((item) => item.id);
    if (ids.length === 0) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);
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
        const translated = _t("toast.restoreSuccess");
        showToast(
          translated === "toast.restoreSuccess" ? "Restored" : translated,
          "success",
        );
      })
      .catch((error) => {
        console.error("Bulk restore failed", error);
        items = previousItems;
        indexedItems = previousIndexedItems;
        selectedIds = previousSelectedIds;
        statusMessage = _t("app.deleteFailed");
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function bulkPermanentDelete() {
    const ids = items.filter((item) => selectedIds.has(item.id) && item.deleted).map((item) => item.id);
    if (ids.length === 0) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);
    const previousDetailItem = detailItem;
    items = items.filter((item) => !ids.includes(item.id));
    if (indexedItems) indexedItems = indexedItems.filter((item) => !ids.includes(item.id));
    selectedIds = new Set([...selectedIds].filter((id) => !ids.includes(id)));
    if (detailItem && ids.includes(detailItem.id)) detailItem = null;

    void persistBatchPermanentDelete(ids)
      .then((removed) => {
        if (removed === false) throw new Error("batch permanent delete failed");
        showToast(_t("toast.bulkDeleteSuccess", { count: ids.length }), "success");
      })
      .catch((error) => {
        console.error("Bulk permanent delete failed", error);
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
    const softIds = useRecycleBin
      ? selectedItems.filter((item) => !item.deleted).map((item) => item.id)
      : [];
    const permanentIds = useRecycleBin
      ? selectedItems.filter((item) => item.deleted).map((item) => item.id)
      : selectedItems.map((item) => item.id);
    const operationIds = new Set([...softIds, ...permanentIds]);
    if (operationIds.size === 0) return;

    const previousItems = items.map((entry) => ({ ...entry }));
    const previousIndexedItems = indexedItems?.map((entry) => ({ ...entry })) ?? null;
    const previousSelectedIds = new Set(selectedIds);
    const previousDetailItem = detailItem;

    items = items
      .filter((item) => !permanentIds.includes(item.id))
      .map((item) => (softIds.includes(item.id) ? { ...item, deleted: true } : item));
    if (indexedItems) {
      indexedItems = indexedItems
        .filter((item) => !permanentIds.includes(item.id))
        .map((item) => (softIds.includes(item.id) ? { ...item, deleted: true } : item));
    }
    selectedIds = new Set();
    if (detailItem && permanentIds.includes(detailItem.id)) detailItem = null;

    const operations: {
      ids: string[];
      permanent: boolean;
      run: () => Promise<boolean | null>;
    }[] = [];
    if (softIds.length > 0) {
      operations.push({ ids: softIds, permanent: false, run: () => persistBatchDelete(softIds) });
    }
    if (permanentIds.length > 0) {
      operations.push({
        ids: permanentIds,
        permanent: true,
        run: () => persistBatchPermanentDelete(permanentIds),
      });
    }

    void Promise.all(
      operations.map(async (operation) => {
        try {
          const result = await operation.run();
          return { ...operation, ok: result !== false };
        } catch (error) {
          console.error(
            operation.permanent ? "Bulk permanent delete failed" : "Bulk delete failed",
            error,
          );
          return { ...operation, ok: false };
        }
      }),
    ).then((outcomes) => {
      const successfulSoft = new Set(
        outcomes.filter((outcome) => outcome.ok && !outcome.permanent).flatMap((o) => o.ids),
      );
      const successfulPermanent = new Set(
        outcomes.filter((outcome) => outcome.ok && outcome.permanent).flatMap((o) => o.ids),
      );
      const failedIds = new Set(
        outcomes.filter((outcome) => !outcome.ok).flatMap((o) => o.ids),
      );
      const succeededIds = new Set([...successfulSoft, ...successfulPermanent]);

      // Rebuild from the snapshot so a partially failed mixed batch mirrors
      // exactly which backend transaction succeeded.
      items = previousItems
        .filter((item) => !successfulPermanent.has(item.id))
        .map((item) => (successfulSoft.has(item.id) ? { ...item, deleted: true } : item));
      if (previousIndexedItems) {
        indexedItems = previousIndexedItems
          .filter((item) => !successfulPermanent.has(item.id))
          .map((item) => (successfulSoft.has(item.id) ? { ...item, deleted: true } : item));
      } else {
        indexedItems = null;
      }
      selectedIds = new Set([...previousSelectedIds].filter((id) => !succeededIds.has(id)));
      if (previousDetailItem && !successfulPermanent.has(previousDetailItem.id)) {
        detailItem = previousDetailItem;
      } else if (successfulPermanent.has(previousDetailItem?.id ?? "")) {
        detailItem = null;
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
    // The clear-history command soft-deletes active records only. Keep
    // records already in the recycle bin visible until they are restored or
    // permanently deleted explicitly.
    const nonFavorites = items.filter((item) => !item.favorite && !item.deleted);
    if (nonFavorites.length === 0) {
      showToast(_t("toast.noRecordsToClear"), "info");
      return;
    }

    const removedIds = new Set(nonFavorites.map((i) => i.id));
    items = items.filter((item) => item.favorite || item.deleted);
    if (indexedItems) {
      indexedItems = indexedItems.filter((item) => item.favorite || item.deleted);
    }
    selectedIds = new Set([...selectedIds].filter((x) => !removedIds.has(x)));

    void invoke<number>("clear_all_non_favorite_items")
      .then((count) => {
        showToast(_t("toast.clearHistorySuccess", { count }), "success");
      })
      .catch((error) => {
        console.error("Unable to clear history", error);
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function handleGlobalKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveSelection(1);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveSelection(-1);
      return;
    }

    if (event.key === "ArrowRight" || (event.key === "Tab" && !event.shiftKey)) {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
        return;
      event.preventDefault();
      const idx = filters.findIndex((f) => f.id === activeFilter);
      const next = (idx + 1) % filters.length;
      setFilter(filters[next].id);
      return;
    }

    if (event.key === "ArrowLeft" || (event.key === "Tab" && event.shiftKey)) {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
        return;
      event.preventDefault();
      const idx = filters.findIndex((f) => f.id === activeFilter);
      const prev = (idx - 1 + filters.length) % filters.length;
      setFilter(filters[prev].id);
      return;
    }

    if (event.key === "Enter") {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
        return;
      event.preventDefault();
      activateSelected();
      return;
    }

    if (event.key === "Escape") {
      if (editingId || document.activeElement instanceof HTMLTextAreaElement) {
        return;
      } else if (detailItem) {
        // DetailPanel handles its own Escape
      } else if (selectedIds.size > 0) {
        selectedIds = new Set();
      } else if ("__TAURI_INTERNALS__" in window) {
        getCurrentWindow()
          .hide()
          .catch(() => {});
      }
      return;
    }

    if (
      event.key === "Backspace" &&
      !(event.target instanceof HTMLInputElement) &&
      !(event.target instanceof HTMLTextAreaElement)
    ) {
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

    // Shortcuts for the focused item
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey) {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) {
        // Let the input handle its own Ctrl+key combinations (Ctrl+A, Ctrl+C, etc.)
        return;
      }
      const item = filteredItems.find((i) => i.id === selectedId);
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
        if (item.kind === "text" || item.kind === "link") {
          openDetail(selectedId);
        }
        return;
      }
    }

    if ((event.metaKey || event.ctrlKey) && /^[1-9]$/.test(event.key)) {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
        return;
      const index = Number(event.key) - 1;
      const item = filteredItems[index];
      if (item) {
        selectedId = item.id;
        activateSelected();
      }
    }
  }

  function handleHistoryScroll() {
    if (historyListEl) {
      scrollTop = historyListEl.scrollTop;
      if (
        activeFilter === "deleted" &&
        deletedHistoryHasMore &&
        !deletedHistoryLoading &&
        historyListEl.scrollTop + historyListEl.clientHeight >= historyListEl.scrollHeight - 180
      ) {
        void loadDeletedHistoryPage();
      }
    }
  }

  async function measureContainer() {
    if (historyListEl) {
      containerHeight = historyListEl.clientHeight;
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
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<main class="app-shell">
  <header
    class="search-header"
    role="presentation"
    aria-label="拖拽窗口"
    onmousedown={(e) => {
      if (e.target === e.currentTarget) void getCurrentWindow().startDragging();
    }}
  >
    <div class="search-box">
      <input
        bind:value={query}
        aria-label={_t("app.searchPlaceholder")}
        autocomplete="off"
        placeholder={_t("app.searchPlaceholder")}
        spellcheck="false"
        style={compactMode
          ? `height: ${compactSearchHeight}px; font-size: ${compactSearchFontSize}px;`
          : undefined}
        onkeydown={(e) => {
          if (e.key === "Backspace") {
            const now = Date.now();
            if (now - lastBackspaceAt < 400 && query) {
              e.preventDefault();
              query = "";
              lastBackspaceAt = 0;
            } else {
              lastBackspaceAt = now;
            }
          } else {
            lastBackspaceAt = 0;
          }
        }}
      />
      {#if query}
        <button
          class="clear-button"
          tabindex="-1"
          type="button"
          aria-label={_t("app.clearSearch")}
          onclick={() => (query = "")}>×</button
        >
      {/if}
    </div>
    <button
      type="button"
      tabindex="-1"
      class="regex-toggle"
      class:regex-active={regexMode}
      title={_t("search.regex")}
      aria-label={_t("search.regex")}
      aria-pressed={regexMode}
      onclick={() => (regexMode = !regexMode)}
      ><AppIcon name="regex" size={15} strokeWidth={2} /></button
    >
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
    aria-label="拖拽窗口"
    onmousedown={(e) => {
      if (e.target === e.currentTarget) void getCurrentWindow().startDragging();
    }}
  >
    <nav class="filters" aria-label={_t("filter.all")}>
      {#each filters as filter}
        <button
          type="button"
          tabindex="-1"
          class:active={activeFilter === filter.id}
          aria-pressed={activeFilter === filter.id}
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
    </nav>

    <div class="filter-dropdowns">
      <!-- Date filter -->
      <div class="dropdown-wrapper">
        <button
          type="button"
          tabindex="-1"
          class="filter-dropdown-btn"
          onclick={() => (dateDropdownOpen = !dateDropdownOpen)}
          aria-label={_t("dateFilter.all")}
          title={_t("dateFilter.all")}
        >
          <AppIcon name="calendar" size={15} />
          <span class="dropdown-label"
            >{dateFilter === "all"
              ? _t("dateFilter.all")
              : (dateFilterOptions.find((o) => o.id === dateFilter)?.label ??
                _t("dateFilter.all"))}</span
          >
          <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
        </button>
        {#if dateDropdownOpen}
          <div class="dropdown-popover" role="menu">
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
                }}>{option.label}</button
              >
            {/each}
          </div>
        {/if}
      </div>

      <!-- Source app filter -->
      <div class="dropdown-wrapper">
        <button
          type="button"
          tabindex="-1"
          class="filter-dropdown-btn"
          onclick={() => (sourceAppDropdownOpen = !sourceAppDropdownOpen)}
          aria-label={_t("sourceApp.all")}
          title={_t("sourceApp.all")}
        >
          <AppIcon name="filter" size={15} />
          <span class="dropdown-label">{sourceAppFilter || _t("sourceApp.all")}</span>
          <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
        </button>
        {#if sourceAppDropdownOpen}
          <div class="dropdown-popover" role="menu">
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
                }}>{_t("sourceApp.all")}</button
              >
              {#each filteredSourceApps as app}
                <button
                  type="button"
                  role="menuitem"
                  class:selected={sourceAppFilter === app}
                  onclick={() => {
                    sourceAppFilter = app;
                    sourceAppDropdownOpen = false;
                  }}>{app}</button
                >
              {/each}
            </div>
          </div>
        {/if}
      </div>
    </div>

    <div class="toolbar-actions">
      <button
        type="button"
        tabindex="-1"
        aria-label={_t("toolbar.pinWindow")}
        title={_t("toolbar.pinWindow")}><AppIcon name="pin" size={17} /></button
      >
      <button
        type="button"
        tabindex="-1"
        aria-label={_t("toolbar.settings")}
        title={_t("toolbar.settings")}
        onclick={openSettings}><AppIcon name="settings" size={17} /></button
      >
    </div>
  </div>

  {#if regexError}
    <div class="regex-error">{regexError}</div>
  {/if}

  <section class="history-panel" aria-label={_t("app.recentRecords")}>
    <div class="section-heading">
      {#if selectedIds.size > 0}
        <span class="multi-count"
          >{selectedIds.size} {_t("status.recordCount", { count: 0 }).replace("0 ", "")}</span
        >
      {/if}
    </div>

    {#if filteredItems.length > 0}
      <div
        class="history-list"
        aria-label="clipboard items"
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
                  index={filteredItems.indexOf(item)}
                  now={currentTime}
                  selected={selectedIds.has(item.id) || item.id === selectedId}
                  checked={selectedIds.has(item.id)}
                  showCheckbox={false}
                  hideActions={selectedIds.size > 0}
                  compact={compactMode}
                  {compactPaddingTop}
                  {compactPaddingBottom}
                  {compactCardGap}
                  {compactCardBorderRadius}
                  compactCardHeight={compactMode
                    ? item.customTitle
                      ? (compactCustomTitle ?? 80)
                      : item.kind === "image"
                        ? (compactImage ?? 130)
                        : item.kind !== "file" && !!item.preview && showSecondaryText !== false
                          ? (compactTallText ?? 70)
                          : (compactText ?? 58)
                    : 0}
                  onselect={selectItem}
                  ontoggleSelect={toggleSelectItem}
                  ontoggleFavorite={toggleFavorite}
                  ondelete={deleteItem}
                  oncopy={copyItem}
                  ondetail={openDetail}
                  onedit={startEdit}
                  onsaveedit={saveEdit}
                  onsaveasnew={saveAsNew}
                  oncanceledit={cancelEdit}
                  onplainpaste={plainPaste}
                  onduplicate={duplicateItem}
                  onrestore={restoreItem}
                />
              </div>
            {:else}
              <ClipboardCard
                {item}
                index={filteredItems.indexOf(item)}
                now={currentTime}
                selected={selectedIds.has(item.id) || item.id === selectedId}
                checked={selectedIds.has(item.id)}
                showCheckbox={false}
                hideActions={selectedIds.size > 0}
                compact={compactMode}
                {compactPaddingTop}
                {compactPaddingBottom}
                {compactCardGap}
                {compactCardBorderRadius}
                compactCardHeight={compactMode
                  ? item.customTitle
                    ? (compactCustomTitle ?? 80)
                    : item.kind === "image"
                      ? (compactImage ?? 130)
                      : item.kind !== "file" && !!item.preview && showSecondaryText !== false
                        ? (compactTallText ?? 70)
                        : (compactText ?? 58)
                  : 0}
                onselect={selectItem}
                ontoggleSelect={toggleSelectItem}
                ontoggleFavorite={toggleFavorite}
                ondelete={deleteItem}
                oncopy={copyItem}
                ondetail={openDetail}
                onedit={startEdit}
                onsaveedit={saveEdit}
                onsaveasnew={saveAsNew}
                oncanceledit={cancelEdit}
                onplainpaste={plainPaste}
                onduplicate={duplicateItem}
                onrestore={restoreItem}
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
        <button type="button" onclick={bulkCopy}
          >&#47;&#47; {_t("bulk.copyN", { count: selectedIds.size })}</button
        >
        <button type="button" onclick={bulkFavorite}
          >&#42; {_t("bulk.favoriteN", { count: selectedIds.size })}</button
        >
        {#if selectedActiveCount > 0}
          <button type="button" class="danger" onclick={bulkDelete}
            >&#47;&#47; {_t("bulk.deleteN", { count: selectedActiveCount })}</button
          >
        {/if}
        {#if selectedDeletedCount > 0}
          <button
            type="button"
            onclick={bulkRestore}
            title="Restore selected records"
            aria-label="Restore selected records"
            >&#8634; {selectedDeletedCount}</button
          >
          <button
            type="button"
            class="danger"
            onclick={bulkPermanentDelete}
            title="Permanently delete selected records"
            aria-label="Permanently delete selected records"
            >&#47;&#47; {_t("bulk.deleteN", { count: selectedDeletedCount })}</button
          >
        {/if}
      </div>
    </div>
  {/if}

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
<DetailPanel
  item={detailItem}
  onclose={closeDetail}
  oncopy={copyItem}
  onedit={startEdit}
  onsaveedit={saveEdit}
  onrenametitle={renameTitle}
  onplainpaste={plainPaste}
  onduplicate={duplicateItem}
  onsaveasnew={saveAsNew}
  oncopyfilename={copyFilename}
/>

<style>
  .app-shell {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr) auto;
    width: 100%;
    height: 100vh;
    min-height: 480px;
    overflow: hidden;
    border: 1px solid #363636;
    color: #eeeeee;
    background: rgba(27, 27, 27, 0.985);
  }

  :global(.app-shell.compact .search-header) {
    padding: 6px 16px 2px;
    gap: 6px;
  }

  :global(.app-shell.compact .search-box input) {
    font-size: 12px;
  }

  :global(.app-shell.compact .toolbar) {
    padding: 2px 8px 4px;
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
    padding: 14px 20px 8px;
  }

  .search-box {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    color: #777777;
  }

  .search-box input {
    flex: 1;
    min-width: 0;
    padding: 2px 0 4px;
    border: 0;
    outline: 0;
    color: #f1f1f1;
    background: transparent;
    font-size: clamp(17px, 3vw, 21px);
    font-weight: 350;
    letter-spacing: -0.02em;
  }

  .search-box input::placeholder {
    color: #6e6e6e;
    opacity: 1;
  }

  .clear-button {
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    color: #8b8b8b;
    background: #2c2c2c;
    cursor: pointer;
  }

  .regex-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 6px;
    color: #5a5a5a;
    background: transparent;
    cursor: pointer;
    transition:
      color 100ms ease,
      border-color 100ms ease,
      background 100ms ease;
  }

  .regex-toggle:hover {
    color: #999;
    background: #2c2c2c;
  }

  .regex-toggle.regex-active {
    color: #b57aec;
    border-color: rgba(181, 122, 236, 0.3);
    background: rgba(181, 122, 236, 0.08);
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
    padding: 2px 12px 9px;
    border-bottom: 1px solid #242424;
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
    color: #b2b2b2;
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
    color: #f3f3f3;
    background: #303030;
  }

  .filters button.active:first-child {
    color: #4aa8ff;
  }

  .filters button:nth-child(2) :global(svg) {
    color: #e2c05d;
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
    gap: 4px;
    height: 29px;
    padding: 0 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: #8b8b8b;
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
    color: #c4c4c4;
    border-color: #3a3a3a;
    background: #252525;
  }

  .dropdown-label {
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dropdown-popover {
    position: absolute;
    z-index: 100;
    top: calc(100% + 4px);
    left: 0;
    min-width: 150px;
    border: 1px solid #3a3a3a;
    border-radius: 8px;
    background: #1e1e1e;
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }

  .dropdown-backdrop {
    position: fixed;
    inset: 0;
    z-index: -1;
  }

  .dropdown-popover button {
    display: block;
    width: 100%;
    padding: 7px 12px;
    border: 0;
    color: #b2b2b2;
    background: transparent;
    text-align: left;
    font-size: 12px;
    cursor: pointer;
  }

  .dropdown-popover button:hover {
    color: #f3f3f3;
    background: #2c2c2c;
  }

  .dropdown-popover button.selected {
    color: #4aa8ff;
    background: rgba(74, 168, 255, 0.08);
  }

  .dropdown-search {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid #2a2a2a;
    color: #6e6e6e;
  }

  .dropdown-search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    color: #e0e0e0;
    background: transparent;
    font-size: 12px;
  }

  .dropdown-search input::placeholder {
    color: #555;
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
    color: #707070;
  }

  .toolbar-actions button:hover {
    color: #d8d8d8;
    background: #2c2c2c;
  }

  .regex-error {
    padding: 4px 16px;
    color: #e85d5d;
    font-size: 11.5px;
    background: rgba(232, 93, 93, 0.06);
    border-bottom: 1px solid rgba(232, 93, 93, 0.12);
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
    padding: 0 17px 7px;
    color: #777777;
    font-size: 11.5px;
  }

  .result-count {
    color: #6f6f6f;
  }

  .multi-count {
    color: #4aa8ff;
    font-weight: 600;
    font-size: 11.5px;
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
    background: #51b96b;
    box-shadow: 0 0 8px rgba(81, 185, 107, 0.4);
  }

  .history-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 7px 18px;
    scrollbar-color: #9a9a9a transparent;
    scrollbar-width: thin;
  }

  .history-list::-webkit-scrollbar {
    width: 7px;
  }
  .history-list::-webkit-scrollbar-track {
    background: transparent;
  }
  .history-list::-webkit-scrollbar-thumb {
    border-radius: 10px;
    background: #858585;
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
    color: #777777;
    text-align: center;
  }

  .empty-icon {
    display: inline-flex;
    margin-bottom: 12px;
    color: #555555;
  }

  .empty-state strong {
    color: #bdbdbd;
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
    border-top: 1px solid #292929;
    background: #1a1a1a;
  }

  .bulk-deselect {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border: 1px solid #3a3a3a;
    border-radius: 14px;
    color: #8b8b8b;
    background: transparent;
    cursor: pointer;
    font-size: 11.5px;
    transition: color 100ms ease;
  }

  .bulk-deselect:hover {
    color: #ccc;
  }

  .bulk-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .bulk-actions button {
    padding: 5px 12px;
    border: 1px solid #333;
    border-radius: 6px;
    color: #b2b2b2;
    background: #222;
    cursor: pointer;
    font-size: 11.5px;
    font-weight: 500;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .bulk-actions button:hover {
    color: #e8e8e8;
    background: #2c2c2c;
  }

  .bulk-actions button.danger {
    border-color: rgba(232, 93, 93, 0.3);
    color: #d87575;
  }

  .bulk-actions button.danger:hover {
    border-color: rgba(232, 93, 93, 0.5);
    background: rgba(232, 93, 93, 0.1);
    color: #e88080;
  }

  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    min-height: 34px;
    padding: 7px 14px;
    border-top: 1px solid #292929;
    color: #6f6f6f;
    background: #181818;
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
    border: 1px solid #393939;
    border-radius: 4px;
    color: #929292;
    background: #232323;
    box-shadow: 0 1px 0 #0f0f0f;
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
