<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import ClipboardCard from "$lib/components/ClipboardCard.svelte";
  import StorageSettingsDialog from "$lib/components/StorageSettingsDialog.svelte";
  import { demoClipboardItems } from "$lib/data/demo-items";
  import {
    loadClipboardHistory,
    persistDelete,
    persistFavorite,
    searchClipboardHistory,
  } from "$lib/services/clipboard";
  import { getRuntimeInfo } from "$lib/services/runtime";
  import type { ClipboardFilter, ClipboardItem } from "$lib/types/clipboard";

  const filters = [
    { id: "all", label: "全部", icon: "grid" },
    { id: "text", label: "文本", icon: "text" },
    { id: "link", label: "链接", icon: "link" },
    { id: "image", label: "图片", icon: "image" },
    { id: "file", label: "文件", icon: "file" },
    { id: "favorite", label: "收藏", icon: "star" },
  ] as const;

  let items = $state<ClipboardItem[]>(demoClipboardItems.map((item) => ({ ...item })));
  let query = $state("");
  let activeFilter = $state<ClipboardFilter>("all");
  let selectedId = $state(demoClipboardItems[0]?.id ?? "");
  let currentTime = $state(Date.now());
  let runtimeLabel = $state("浏览器预览");
  let statusMessage = $state("使用 ↑ ↓ 选择，Enter 快速粘贴");
  let settingsOpen = $state(false);
  let indexedItems = $state<ClipboardItem[] | null>(null);
  let indexedQuery = $state("");
  let searchPending = $state(false);
  let searchRequestId = 0;

  const filteredItems = $derived.by(() => {
    const normalizedQuery = query.trim();
    const keywords = normalizedQuery.toLocaleLowerCase().split(/\s+/).filter(Boolean);
    const usesIndexedResults = indexedItems !== null && indexedQuery === normalizedQuery;
    const candidates = usesIndexedResults ? (indexedItems ?? []) : items;

    return candidates.filter((item) => {
      const matchesFilter =
        activeFilter === "all" ||
        (activeFilter === "favorite" ? item.favorite : item.kind === activeFilter);

      if (!matchesFilter || keywords.length === 0 || usesIndexedResults) {
        return matchesFilter;
      }

      const searchableText = [item.title, item.preview, item.sourceApp]
        .join(" ")
        .toLocaleLowerCase();

      return keywords.every((keyword) => searchableText.includes(keyword));
    });
  });

  const selectedIndex = $derived(filteredItems.findIndex((item) => item.id === selectedId));
  const resultSummary = $derived(searchPending ? "搜索中…" : `${filteredItems.length} 条记录`);

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
          statusMessage = `索引搜索命中 ${results.length} 条记录`;
        })
        .catch((error) => {
          if (requestId !== searchRequestId) return;
          console.error("Unable to search clipboard history", error);
          statusMessage = "索引搜索失败，已保留本地筛选结果";
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

  onMount(() => {
    const clock = window.setInterval(() => {
      currentTime = Date.now();
    }, 30_000);

    void getRuntimeInfo().then((runtime) => {
      if (runtime) {
        runtimeLabel = `${runtime.operatingSystem} / ${runtime.architecture} · 核心已连接`;
      }
    });

    void loadClipboardHistory()
      .then((storedItems) => {
        if (storedItems === null) return;

        items = storedItems;
        selectedId = storedItems[0]?.id ?? "";
        statusMessage = storedItems.length
          ? `已从本地数据库载入 ${storedItems.length} 条记录`
          : "剪贴板历史为空，复制内容后会出现在这里";
      })
      .catch((error) => {
        console.error("Unable to load clipboard history", error);
        statusMessage = "读取本地剪贴板历史失败";
      });

    return () => window.clearInterval(clock);
  });

  function setFilter(filter: ClipboardFilter) {
    activeFilter = filter;
  }

  function selectItem(id: string) {
    selectedId = id;
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
        statusMessage = "收藏状态保存失败";
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

    void persistDelete(id)
      .then((removed) => {
        if (removed === false) throw new Error("record not found");
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
        statusMessage = "删除记录失败";
      });
  }

  function activateSelected() {
    const item = filteredItems.find((candidate) => candidate.id === selectedId);
    if (!item) return;

    statusMessage = `已选择“${item.title.split("\n")[0]}” · 平台粘贴服务待接入`;
  }

  function moveSelection(offset: number) {
    if (filteredItems.length === 0) return;

    const current = Math.max(0, selectedIndex);
    const next = Math.min(filteredItems.length - 1, Math.max(0, current + offset));
    selectedId = filteredItems[next].id;
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

    if (event.key === "Escape" && query) {
      query = "";
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
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<main class="app-shell">
  <header class="search-header">
    <div class="search-box">
      <AppIcon name="search" size={20} strokeWidth={1.65} />
      <input
        bind:value={query}
        aria-label="搜索剪贴板历史"
        autocomplete="off"
        placeholder="输入关键字，唤醒沉睡记忆"
        spellcheck="false"
      />
      {#if query}
        <button
          class="clear-button"
          type="button"
          aria-label="清除搜索"
          onclick={() => (query = "")}>×</button
        >
      {/if}
    </div>

    <div class="brand-mark" title="Clipboard">
      <span></span>
      <span></span>
      <span></span>
    </div>
  </header>

  <div class="toolbar">
    <nav class="filters" aria-label="剪贴板类型筛选">
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

    <div class="toolbar-actions">
      <button type="button" aria-label="清理记录" title="清理记录"
        ><AppIcon name="trash" size={17} /></button
      >
      <button type="button" aria-label="帮助" title="帮助"><AppIcon name="help" size={17} /></button
      >
      <button type="button" aria-label="固定窗口" title="固定窗口"
        ><AppIcon name="pin" size={17} /></button
      >
      <button type="button" aria-label="设置" title="设置" onclick={() => (settingsOpen = true)}
        ><AppIcon name="settings" size={17} /></button
      >
    </div>
  </div>

  <section class="history-panel" aria-label="剪贴板历史">
    <div class="section-heading">
      <div>
        <span class="eyebrow">最近记录</span>
        <span class="result-count">{resultSummary}</span>
      </div>
      <span class="runtime-status"><i></i>{runtimeLabel}</span>
    </div>

    {#if filteredItems.length > 0}
      <div class="history-list" aria-label="剪贴板项目">
        {#each filteredItems as item, index (item.id)}
          <ClipboardCard
            {item}
            {index}
            now={currentTime}
            selected={item.id === selectedId}
            onselect={selectItem}
            ontoggleFavorite={toggleFavorite}
            ondelete={deleteItem}
          />
        {/each}
      </div>
    {:else}
      <div class="empty-state">
        <span class="empty-icon"><AppIcon name="clipboard" size={28} /></span>
        <strong>{items.length === 0 ? "暂无剪贴板记录" : "没有找到相关记录"}</strong>
        <p>
          {items.length === 0
            ? "复制文本、图片或文件后会出现在这里。"
            : "尝试更换关键字或内容类型。"}
        </p>
      </div>
    {/if}
  </section>

  <footer class="status-bar">
    <span>{statusMessage}</span>
    <span class="shortcut-hints"><kbd>Alt</kbd><b>+</b><kbd>V</kbd> 唤起</span>
  </footer>
</main>

<StorageSettingsDialog open={settingsOpen} onclose={() => (settingsOpen = false)} />

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
    gap: 14px;
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
    gap: 10px;
    padding: 2px 12px 9px;
    border-bottom: 1px solid #242424;
  }

  .filters,
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

  .history-panel {
    display: flex;
    min-height: 0;
    flex-direction: column;
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 11px 17px 7px;
    color: #777777;
    font-size: 10.5px;
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
    font-size: 10.5px;
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

  @media (max-width: 660px) {
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
