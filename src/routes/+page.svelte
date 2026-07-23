<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import ClipboardCard from "$lib/components/ClipboardCard.svelte";
  import DetailPanel from "$lib/components/DetailPanel.svelte";
  import StorageSettingsDialog from "$lib/components/StorageSettingsDialog.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import { demoClipboardItems } from "$lib/data/demo-items";
  import {
    loadClipboardHistory,
    persistDelete,
    persistFavorite,
    persistBatchFavorite,
    persistBatchDelete,
    searchClipboardHistory,
    listSourceApplications,
  } from "$lib/services/clipboard";
  import { getRuntimeInfo } from "$lib/services/runtime";
  import { showToast } from "$lib/services/toast";
  import type { ClipboardFilter, ClipboardItem } from "$lib/types/clipboard";
  import type { IconName } from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { createVirtualList, type VirtualScrollConfig } from "$lib/utils/virtual-scroll";
  import { parseDateQuery } from "$lib/utils/date-query";
  import { listen } from "@tauri-apps/api/event";
  import type { PersistedClipboardItem } from "$lib/types/clipboard";

  const _t = (path: string, params?: Record<string, string | number>) => resolvePath($messages, path, params);

  const filters = $derived([
    { id: "all" as ClipboardFilter, label: _t("filter.all"), icon: "grid" as IconName },
    { id: "text" as ClipboardFilter, label: _t("filter.text"), icon: "text" as IconName },
    { id: "link" as ClipboardFilter, label: _t("filter.link"), icon: "link" as IconName },
    { id: "image" as ClipboardFilter, label: _t("filter.image"), icon: "image" as IconName },
    { id: "file" as ClipboardFilter, label: _t("filter.file"), icon: "file" as IconName },
    { id: "favorite" as ClipboardFilter, label: _t("filter.favorite"), icon: "star" as IconName },
  ]);

  const dateFilterOptions = $derived([
    { id: "all" as const, label: _t("dateFilter.all") },
    { id: "today" as const, label: _t("dateFilter.today") },
    { id: "yesterday" as const, label: _t("dateFilter.yesterday") },
    { id: "week" as const, label: _t("dateFilter.week") },
    { id: "month" as const, label: _t("dateFilter.month") },
  ]);

  const VIRTUAL_SCROLL_CONFIG: VirtualScrollConfig = { itemHeight: 88, overscan: 5 };
  const VIRTUAL_SCROLL_THRESHOLD = 50;

  let items = $state<ClipboardItem[]>(demoClipboardItems.map((item) => ({ ...item })));
  let query = $state("");
  let activeFilter = $state<ClipboardFilter>("all");
  let selectedId = $state(demoClipboardItems[0]?.id ?? "");
  let currentTime = $state(Date.now());
  let runtimeLabel = $state(_t("app.browserPreview"));
  let statusMessage = $state(_t("app.activateHint"));
  let view = $state<'main' | 'settings'>('main');
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

    const startOfDay = (ts: number) => { const d = new Date(ts); d.setHours(0, 0, 0, 0); return d.getTime(); };
    const endOfDay = (ts: number) => { const d = new Date(ts); d.setHours(23, 59, 59, 999); return d.getTime(); };
    const startOfWeek = (ts: number) => {
      const d = new Date(ts);
      const day = d.getDay();
      const diff = d.getDate() - day + (day === 0 ? -6 : 1);
      d.setDate(diff);
      d.setHours(0, 0, 0, 0);
      return d.getTime();
    };

    switch (filter) {
      case "today": return { from: startOfDay(now), to: endOfDay(now) };
      case "yesterday": return { from: startOfDay(now - dayMs), to: endOfDay(now - dayMs) };
      case "week": return { from: startOfWeek(now), to: endOfDay(now) };
      case "month": {
        const d = new Date(now);
        d.setDate(1);
        d.setHours(0, 0, 0, 0);
        return { from: d.getTime(), to: endOfDay(now) };
      }
      default: return null;
    }
  }

  // --- Filtering ---

  const filteredItems = $derived.by(() => {
    const normalizedQuery = query.trim();
    const keywords = normalizedQuery.toLocaleLowerCase().split(/\s+/).filter(Boolean);
    const usesIndexedResults = indexedItems !== null && indexedQuery === normalizedQuery;
    const candidates = usesIndexedResults ? (indexedItems ?? []) : items;

    const dateRange = resolveDateRange(dateFilter);
    const dateRangeFromNl = !dateRange ? parseDateQuery(normalizedQuery) : null;
    const effectiveDateRange = dateRange ?? dateRangeFromNl;

    return candidates.filter((item) => {
      const matchesFilter =
        activeFilter === "all" ||
        (activeFilter === "favorite" ? item.favorite : item.kind === activeFilter);

      if (!matchesFilter) return false;

      if (sourceAppFilter && !item.sourceApp.toLowerCase().includes(sourceAppFilter.toLowerCase())) {
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
          return re.test([item.title, item.preview, item.sourceApp].join(" "));
        } catch {
          regexError = _t("search.regexError");
          return true;
        }
      }

      const searchableText = [item.title, item.preview, item.sourceApp]
        .join(" ")
        .toLocaleLowerCase();

      return keywords.every((keyword) => searchableText.includes(keyword));
    });
  });

  const selectedIndex = $derived(filteredItems.findIndex((item) => item.id === selectedId));
  const resultSummary = $derived(searchPending ? _t("status.searching") : _t("status.recordCount", { count: filteredItems.length }));

  // --- Virtual scrolling ---

  const virtualList = $derived(
    createVirtualList(filteredItems.length, containerHeight, scrollTop, VIRTUAL_SCROLL_CONFIG),
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

    if (!requestedQuery) {
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

  onMount(() => {
    const clock = window.setInterval(() => {
      currentTime = Date.now();
    }, 30_000);

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
        statusMessage = storedItems.length
          ? _t("status.recordCount", { count: storedItems.length })
          : _t("app.historyEmpty");
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
        preview: record.textContent && record.textContent !== record.title ? record.textContent : "",
        sourceApp,
        sourceTone: sourceApp.includes("codex") ? "violet"
          : sourceApp.includes("Chrome") || sourceApp.includes("Edge") ? "blue"
          : sourceApp === "Clipboard" ? "neutral" : "red",
        sizeLabel: record.kind === "text" || record.kind === "link"
          ? `${record.textContent?.length || record.title.length} chars`
          : `${record.sizeBytes} B`,
        createdAt: record.createdAtMs,
        favorite: record.isFavorite,
        fileName: record.kind === "file" ? record.resourcePath?.split(/[\\/]/).pop() || record.title : undefined,
        previewPath: record.previewPath,
        resourcePath: record.resourcePath,
        textContent: record.textContent,
      };
      const existingIdx = items.findIndex(i => i.id === newItem.id);
      if (existingIdx >= 0) {
        items[existingIdx] = newItem;
        items = items;
      } else {
        items = [newItem, ...items];
        selectedId = newItem.id;
      }
    });

    return () => {
      window.clearInterval(clock);
      unlisten.then(fn => fn());
    };
  });

  // --- Handlers ---

  function setFilter(filter: ClipboardFilter) {
    activeFilter = filter;
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
        showToast(nextFavorite ? _t("toast.favoriteSuccess") : _t("toast.unfavoriteSuccess"), "success");
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
    const deleted = filteredItems.find((item) => item.id === id);
    if (!deleted) return;

    const historyIndex = items.findIndex((item) => item.id === id);
    const searchIndex = indexedItems?.findIndex((item) => item.id === id) ?? -1;
    items = items.filter((item) => item.id !== id);
    if (indexedItems) {
      indexedItems = indexedItems.filter((item) => item.id !== id);
    }
    selectedIds = new Set([...selectedIds].filter((x) => x !== id));

    void persistDelete(id)
      .then((removed) => {
        if (removed === false) throw new Error("record not found");
        showToast(_t("toast.deleteSuccess"), "success");
      })
      .catch((error) => {
        console.error("Unable to delete clipboard item", error);
        if (historyIndex >= 0) {
          items = [...items.slice(0, historyIndex), deleted, ...items.slice(historyIndex)];
        }
        if (indexedItems && searchIndex >= 0) {
          indexedItems = [
            ...indexedItems.slice(0, searchIndex),
            deleted,
            ...indexedItems.slice(searchIndex),
          ];
        }
        statusMessage = _t("app.deleteFailed");
        showToast(_t("app.deleteFailed"), "error");
      });
  }

  function copyItem(id: string) {
    const item = items.find((i) => i.id === id);
    if (!item) return;
    void navigator.clipboard.writeText(item.title).then(() => {
      showToast(_t("toast.copySuccess"), "success");
    }).catch(() => {
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

  function startEdit(id: string) {
    // handled within ClipboardCard
  }

  function saveEdit(id: string, content: string) {
    const firstLine = content.split('\n')[0].slice(0, 200);
    const rest = content.length > firstLine.length ? content.slice(firstLine.length).trim() : '';
    items = items.map((item) =>
      item.id === id ? { ...item, title: firstLine, textContent: content, preview: rest || item.preview } : item,
    );
    if (indexedItems) {
      indexedItems = indexedItems.map((item) =>
        item.id === id ? { ...item, title: firstLine, textContent: content, preview: rest || item.preview } : item,
      );
    }
    showToast(_t("toast.editSaved"), "success");
  }

  function cancelEdit(_id: string) {
    // handled within ClipboardCard
  }

  function plainPaste(_id: string) {
    const item = items.find((i) => i.id === _id);
    if (!item) return;
    void navigator.clipboard.writeText(item.title).then(() => {
      showToast(_t("toast.plainPasteSuccess"), "success");
    }).catch(() => {
      showToast(_t("toast.copyFailed"), "error");
    });
  }

  function formatPaste(_id: string) {
    const item = items.find((i) => i.id === _id);
    if (!item) return;
    void navigator.clipboard.writeText(item.title).then(() => {
      showToast(_t("toast.copySuccess"), "success");
    }).catch(() => {
      showToast(_t("toast.copyFailed"), "error");
    });
  }

  // --- Bulk operations ---

  function bulkCopy() {
    const selectedItems = items.filter((i) => selectedIds.has(i.id));
    const text = selectedItems.map((i) => i.title).join("\n");
    void navigator.clipboard.writeText(text).then(() => {
      showToast(_t("toast.bulkCopySuccess", { count: selectedIds.size }), "success");
    }).catch(() => {
      showToast(_t("toast.copyFailed"), "error");
    });
  }

  function bulkFavorite() {
    const ids = [...selectedIds];
    items = items.map((item) =>
      selectedIds.has(item.id) ? { ...item, favorite: true } : item,
    );
    void persistBatchFavorite(ids, true).then(() => {
      showToast(_t("toast.bulkFavoriteSuccess", { count: ids.length }), "success");
      selectedIds = new Set();
    }).catch((error) => {
      console.error("Bulk favorite failed", error);
    });
  }

  function bulkDelete() {
    const ids = [...selectedIds];
    const removed = items.filter((i) => selectedIds.has(i.id));
    items = items.filter((i) => !selectedIds.has(i.id));
    selectedIds = new Set();

    void persistBatchDelete(ids).then(() => {
      showToast(_t("toast.bulkDeleteSuccess", { count: ids.length }), "success");
    }).catch((error) => {
      console.error("Bulk delete failed", error);
      items = [...items, ...removed];
    });
  }

  function activateSelected() {
    const item = filteredItems.find((candidate) => candidate.id === selectedId);
    if (!item) return;

    statusMessage = _t("app.activateItem", { title: item.title.split("\n")[0] });
  }

  function moveSelection(offset: number) {
    if (filteredItems.length === 0) return;

    const current = Math.max(0, selectedIndex);
    const next = Math.min(filteredItems.length - 1, Math.max(0, current + offset));
    selectedId = filteredItems[next].id;
  }

  function clearHistory() {
    const nonFavorites = items.filter((item) => !item.favorite);
    if (nonFavorites.length === 0) {
      showToast(_t("toast.noRecordsToClear"), "info");
      return;
    }

    const removedIds = new Set(nonFavorites.map((i) => i.id));
    items = items.filter((item) => item.favorite);
    if (indexedItems) {
      indexedItems = indexedItems.filter((item) => item.favorite);
    }
    selectedIds = new Set([...selectedIds].filter((x) => !removedIds.has(x)));

    void invoke<number>("clear_all_non_favorite_items")
      .then((count) => {
        showToast(_t("toast.clearHistorySuccess", { count }), "success");
        statusMessage = items.length
          ? _t("status.recordCount", { count: items.length })
          : _t("app.historyEmpty");
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

    if (event.key === "Enter") {
      event.preventDefault();
      activateSelected();
      return;
    }

    if (event.key === "Escape") {
      if (selectedIds.size > 0) {
        selectedIds = new Set();
        return;
      }
      if (query) {
        query = "";
        return;
      }
      if (view === 'settings') {
        view = 'main';
        return;
      }
      import('@tauri-apps/api/window').then(m => m.getCurrentWindow().hide());
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key === "a") {
      event.preventDefault();
      selectedIds = new Set(filteredItems.map((i) => i.id));
      return;
    }

    if ((event.metaKey || event.ctrlKey) && /^[1-9]$/.test(event.key)) {
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
  {#if view === 'main'}
    <header class="search-header">
      <div class="search-box">
        <AppIcon name="search" size={20} strokeWidth={1.65} />
        <input
          bind:value={query}
          aria-label={_t("app.searchPlaceholder")}
          autocomplete="off"
          placeholder={_t("app.searchPlaceholder")}
          spellcheck="false"
      />
      {#if query}
        <button
          class="clear-button"
          type="button"
          aria-label={_t("app.clearSearch")}
          onclick={() => (query = "")}>×</button>
      {/if}
    </div>
    <button
      type="button"
      class="regex-toggle"
      class:regex-active={regexMode}
      title={_t("search.regex")}
      aria-label={_t("search.regex")}
      aria-pressed={regexMode}
      onclick={() => (regexMode = !regexMode)}
    ><AppIcon name="regex" size={15} strokeWidth={2} /></button>
    <div class="brand-mark" title="Clipboard">
      <span></span>
      <span></span>
      <span></span>
    </div>
  </header>

  <div class="toolbar">
    <nav class="filters" aria-label={_t("filter.all")}>
      {#each filters as filter}
        <button
          type="button"
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
        <button type="button" class="filter-dropdown-btn" onclick={() => (dateDropdownOpen = !dateDropdownOpen)}
          aria-label={_t("dateFilter.all")} title={_t("dateFilter.all")}
        >
          <AppIcon name="calendar" size={15} />
          <span class="dropdown-label">{dateFilter === "all" ? _t("dateFilter.all") : dateFilterOptions.find(o => o.id === dateFilter)?.label ?? _t("dateFilter.all")}</span>
          <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
        </button>
        {#if dateDropdownOpen}
          <div class="dropdown-popover" role="menu">
            <div class="dropdown-backdrop" onclick={() => (dateDropdownOpen = false)} aria-hidden="true"></div>
            {#each dateFilterOptions as option}
              <button
                type="button"
                role="menuitem"
                class:selected={dateFilter === option.id}
                onclick={() => { dateFilter = option.id; dateDropdownOpen = false; }}
              >{option.label}</button>
            {/each}
          </div>
        {/if}
      </div>

      <!-- Source app filter -->
      <div class="dropdown-wrapper">
        <button type="button" class="filter-dropdown-btn" onclick={() => (sourceAppDropdownOpen = !sourceAppDropdownOpen)}
          aria-label={_t("sourceApp.all")} title={_t("sourceApp.all")}
        >
          <AppIcon name="filter" size={15} />
          <span class="dropdown-label">{sourceAppFilter || _t("sourceApp.all")}</span>
          <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
        </button>
        {#if sourceAppDropdownOpen}
          <div class="dropdown-popover" role="menu">
            <div class="dropdown-backdrop" onclick={() => (sourceAppDropdownOpen = false)} aria-hidden="true"></div>
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
                onclick={() => { sourceAppFilter = ""; sourceAppDropdownOpen = false; }}
              >{_t("sourceApp.all")}</button>
              {#each filteredSourceApps as app}
                <button
                  type="button"
                  role="menuitem"
                  class:selected={sourceAppFilter === app}
                  onclick={() => { sourceAppFilter = app; sourceAppDropdownOpen = false; }}
                >{app}</button>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    </div>

    <div class="toolbar-actions">
      <button type="button" aria-label={_t("toolbar.clearHistory")} title={_t("toolbar.clearHistory")}
        onclick={clearHistory}
        ><AppIcon name="trash" size={17} /></button>
      <button type="button" aria-label={_t("toolbar.help")} title={_t("toolbar.help")}
        ><AppIcon name="help" size={17} /></button>
      <button type="button" aria-label={_t("toolbar.pinWindow")} title={_t("toolbar.pinWindow")}
        ><AppIcon name="pin" size={17} /></button>
      <button type="button" aria-label={_t("toolbar.settings")} title={_t("toolbar.settings")} onclick={() => (view = 'settings')}
        ><AppIcon name="settings" size={17} /></button>
    </div>
  </div>

  {#if regexError}
    <div class="regex-error">{regexError}</div>
  {/if}

  <section class="history-panel" aria-label={_t("app.recentRecords")}>
    <div class="section-heading">
      <div>
        <span class="eyebrow">{_t("app.recentRecords")}</span>
        <span class="result-count">{resultSummary}</span>
      </div>
      {#if selectedIds.size > 0}
        <span class="multi-count">{selectedIds.size} {_t("status.recordCount", { count: 0 }).replace("0 ", "")}</span>
      {/if}
      <span class="runtime-status"><i></i>{runtimeLabel}</span>
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
          style="height: {useVirtualScroll ? virtualList.totalHeight + 'px' : 'auto'}; position: {useVirtualScroll ? 'relative' : 'static'};"
        >
          {#each visiblePageItems as item, visibleIdx (item.id)}
            {#if useVirtualScroll}
              <div
                style="position: absolute; top: {virtualList.offsetY + visibleIdx * VIRTUAL_SCROLL_CONFIG.itemHeight}px; left: 0; right: 0;"
              >
                <ClipboardCard
                  {item}
                  index={filteredItems.indexOf(item)}
                  now={currentTime}
                  selected={item.id === selectedId}
                  checked={selectedIds.has(item.id)}
                  showCheckbox={selectedIds.size > 0}
                  onselect={selectItem}
                  ontoggleSelect={toggleSelectItem}
                  ontoggleFavorite={toggleFavorite}
                  ondelete={deleteItem}
                  oncopy={copyItem}
                  ondetail={openDetail}
                  onedit={startEdit}
                  onsaveedit={saveEdit}
                  oncanceledit={cancelEdit}
                  onplainpaste={plainPaste}
                  onformatpaste={formatPaste}
                />
              </div>
            {:else}
              <ClipboardCard
                {item}
                index={filteredItems.indexOf(item)}
                now={currentTime}
                selected={item.id === selectedId}
                checked={selectedIds.has(item.id)}
                showCheckbox={selectedIds.size > 0}
                onselect={selectItem}
                ontoggleSelect={toggleSelectItem}
                ontoggleFavorite={toggleFavorite}
                ondelete={deleteItem}
                oncopy={copyItem}
                ondetail={openDetail}
                onedit={startEdit}
                onsaveedit={saveEdit}
                oncanceledit={cancelEdit}
                onplainpaste={plainPaste}
                onformatpaste={formatPaste}
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
          {items.length === 0
            ? _t("app.noRecordsHint")
            : _t("app.noMatchRecordsHint")}
        </p>
      </div>
    {/if}
  </section>

  {#if selectedIds.size > 0}
    <div class="bulk-bar">
      <button type="button" class="bulk-deselect" onclick={() => (selectedIds = new Set())}
        title={_t("bulk.deselectAll")}>
        <AppIcon name="x" size={14} strokeWidth={2.5} />
        <span>{selectedIds.size}</span>
      </button>
      <div class="bulk-actions">
        <button type="button" onclick={bulkCopy}>&#47;&#47; {_t("bulk.copyN", { count: selectedIds.size })}</button>
        <button type="button" onclick={bulkFavorite}>&#42; {_t("bulk.favoriteN", { count: selectedIds.size })}</button>
        <button type="button" class="danger" onclick={bulkDelete}>&#47;&#47; {_t("bulk.deleteN", { count: selectedIds.size })}</button>
      </div>
    </div>
  {/if}

  <footer class="status-bar" role="status" aria-live="polite">
    <span>{statusMessage}</span>
    <span class="shortcut-hints"><kbd>Alt</kbd><b>+</b><kbd>V</kbd> {_t("app.shortcutHint")}</span>
  </footer>
  {:else}
    <div class="settings-page">
      <button type="button" class="settings-back-btn" onclick={() => (view = 'main')} aria-label="Back">
        <AppIcon name="chevron-left" size={18} strokeWidth={2} />
        <span>Back</span>
      </button>
      <StorageSettingsDialog open={true} onclose={() => (view = 'main')} />
    </div>
  {/if}
</main>

<Toast />
{#if view === 'main'}
  <DetailPanel
    item={detailItem}
    onclose={closeDetail}
    oncopy={copyItem}
    onedit={startEdit}
    onplainpaste={plainPaste}
    onformatpaste={formatPaste}
  />
{/if}

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

  .search-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 15px 16px 8px;
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
    transition: color 100ms ease, border-color 100ms ease, background 100ms ease;
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

  .brand-mark {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    gap: 2px;
    width: 28px;
    height: 28px;
    padding: 6px;
    border: 1px solid rgba(255, 255, 255, 0.26);
    border-radius: 8px;
    background: linear-gradient(145deg, #ff4b4b, #c61f29);
    box-shadow: 0 4px 14px rgba(220, 35, 45, 0.18);
  }

  .brand-mark span {
    width: 3px;
    border-radius: 3px;
    background: #ffffff;
  }

  .brand-mark span:nth-child(1) {
    height: 9px;
    opacity: 0.75;
  }
  .brand-mark span:nth-child(2) {
    height: 15px;
  }
  .brand-mark span:nth-child(3) {
    height: 12px;
    opacity: 0.88;
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
    transition: color 100ms ease, border-color 100ms ease, background 100ms ease;
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
    padding: 11px 17px 7px;
    color: #777777;
    font-size: 11.5px;
  }

  .section-heading > div {
    display: flex;
    align-items: center;
    gap: 9px;
  }

  .eyebrow {
    color: #bcbcbc;
    font-weight: 600;
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
    transition: background 100ms ease, color 100ms ease;
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
  }

  .status-bar > span:first-child {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
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

  .settings-page {
    position: relative;
    width: 100%;
    height: 100%;
  }

  .settings-back-btn {
    position: absolute;
    top: 12px;
    left: 12px;
    z-index: 51;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #b2b2b2;
    background: #1e1e1e;
    cursor: pointer;
    font-size: 12px;
  }

  .settings-back-btn:hover {
    color: #e8e8e8;
    background: #2c2c2c;
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
