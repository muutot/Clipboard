<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { PhysicalPosition } from "@tauri-apps/api/dpi";
  import { listen } from "@tauri-apps/api/event";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import {
    copyClipboardItem,
    getDisplayTitle,
    loadClipboardHistory,
  } from "$lib/services/clipboard";
  import type { ClipboardItem } from "$lib/types/clipboard";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  type FloatFilter = "all" | "favorite";

  let filter = $state<FloatFilter>("all");
  let items = $state<ClipboardItem[]>([]);
  let loading = $state(true);
  let copyingId = $state<string | null>(null);

  async function load() {
    if (!isTauriRuntime()) {
      loading = false;
      return;
    }
    loading = true;
    try {
      const args = filter === "favorite" ? { favorite: true } : {};
      items = (await loadClipboardHistory(100, 0, args)) ?? [];
    } catch (error) {
      console.error("Unable to load float history", error);
      items = [];
    } finally {
      loading = false;
    }
  }

  function switchFilter(next: FloatFilter) {
    if (filter === next) return;
    filter = next;
    void load();
  }

  async function copy(id: string) {
    const item = items.find((i) => i.id === id);
    if (!item || copyingId) return;
    copyingId = id;
    try {
      await copyClipboardItem(item);
    } finally {
      copyingId = null;
    }
  }

  function rowTitle(item: ClipboardItem): string {
    return getDisplayTitle(item.title) || item.preview || item.id;
  }

  onMount(() => {
    let disposed = false;
    void load();
    const reload = () => {
      if (!disposed) void load();
    };
    let unlistenAdded: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    if (isTauriRuntime()) {
      listen("clipboard-item-added", reload).then((unlisten) => {
        if (disposed) unlisten();
        else unlistenAdded = unlisten;
      });
      getCurrentWindow()
        .onFocusChanged(({ payload: focused }) => {
          if (focused) reload();
        })
        .then((unlisten) => {
          if (disposed) unlisten();
          else unlistenFocus = unlisten;
        })
        .catch(() => {});
    }
    return () => {
      disposed = true;
      endHeaderDrag();
      unlistenAdded?.();
      unlistenFocus?.();
    };
  });

  function close() {
    if (isTauriRuntime())
      void getCurrentWindow()
        .close()
        .catch(() => {});
  }

  /**
   * Header drag uses manual mouse tracking instead of the native drag
   * region: presses on empty header space (title/gaps) move the window via
   * `setPosition`, while presses resolving to a button keep native behavior
   * (target may be a text node, so walk up to the nearest element first).
   */
  function isButtonPress(event: MouseEvent): boolean {
    let node: Node | null = event.target instanceof Node ? event.target : null;
    while (node instanceof Element) {
      if (node.tagName === "BUTTON" || node.closest("button")) return true;
      node = node.parentNode;
    }
    return false;
  }

  let dragState: {
    startX: number;
    startY: number;
    winX: number;
    winY: number;
    scale: number;
  } | null = null;

  async function startHeaderDrag(event: MouseEvent) {
    if (!isTauriRuntime() || event.button !== 0 || isButtonPress(event)) return;
    const win = getCurrentWindow();
    try {
      const [pos, scale] = await Promise.all([win.outerPosition(), win.scaleFactor()]);
      dragState = {
        startX: event.screenX,
        startY: event.screenY,
        winX: pos.x,
        winY: pos.y,
        scale,
      };
      window.addEventListener("mousemove", onHeaderDragMove);
      window.addEventListener("mouseup", endHeaderDrag, { once: true });
    } catch {
      dragState = null;
    }
  }

  function onHeaderDragMove(event: MouseEvent) {
    if (!dragState || !isTauriRuntime()) return;
    const x = Math.round(dragState.winX + (event.screenX - dragState.startX) * dragState.scale);
    const y = Math.round(dragState.winY + (event.screenY - dragState.startY) * dragState.scale);
    void getCurrentWindow()
      .setPosition(new PhysicalPosition(x, y))
      .catch(() => {});
  }

  function endHeaderDrag() {
    dragState = null;
    window.removeEventListener("mousemove", onHeaderDragMove);
  }
</script>

<div class="float-shell">
  <!-- svelte-ignore a11y_no_static_element_interactions: header is a window drag handle; inner buttons keep native semantics -->
  <header class="float-header" data-tauri-drag-region onmousedown={startHeaderDrag}>
    <span class="float-title">{_t("float.title")}</span>
    <div class="float-tabs" role="tablist" aria-label={_t("float.title")}>
      <button
        type="button"
        role="tab"
        aria-selected={filter === "all"}
        class:active={filter === "all"}
        onclick={() => switchFilter("all")}>{_t("float.all")}</button
      >
      <button
        type="button"
        role="tab"
        aria-selected={filter === "favorite"}
        class:active={filter === "favorite"}
        onclick={() => switchFilter("favorite")}>{_t("float.favorites")}</button
      >
    </div>
    <button type="button" class="float-close" aria-label={_t("actions.close")} onclick={close}
      >×</button
    >
  </header>

  <main class="float-list" aria-label={_t("float.title")}>
    {#if loading}
      <p class="float-empty">{_t("status.searching")}</p>
    {:else if items.length === 0}
      <p class="float-empty">{_t("float.empty")}</p>
    {:else}
      {#each items as item (item.id)}
        <button
          type="button"
          class="float-row"
          title={rowTitle(item)}
          disabled={copyingId === item.id}
          onclick={() => void copy(item.id)}
        >
          <AppIcon name={item.kind} size={14} />
          <span class="float-row-title">{rowTitle(item)}</span>
          {#if item.favorite}
            <AppIcon name="star" size={12} filled={true} />
          {/if}
        </button>
      {/each}
    {/if}
  </main>
</div>

<Toast />

<style>
  .float-shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    border-radius: 10px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-app);
    color: var(--text-primary);
    font-size: 12px;
  }

  .float-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 6px 10px;
    border-bottom: 1px solid var(--border-subtle);
    user-select: none;
  }

  .float-title {
    color: var(--text-muted);
    font-weight: 600;
    white-space: nowrap;
  }

  .float-tabs {
    display: flex;
    gap: 2px;
    margin-left: auto;
  }

  .float-tabs button {
    padding: 3px 10px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
    font-size: 11.5px;
  }

  .float-tabs button:hover {
    background: var(--hover-bg);
  }

  .float-tabs button.active {
    color: var(--text-primary);
    background: var(--hover-bg);
    border-color: var(--border-subtle);
  }

  .float-close {
    padding: 2px 8px;
    border: none;
    border-radius: 6px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
    font-size: 14px;
    line-height: 1.4;
  }

  .float-close:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .float-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px;
  }

  .float-empty {
    padding: 16px 8px;
    color: var(--text-faint);
    text-align: center;
  }

  .float-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
    text-align: left;
    font-size: 12px;
  }

  .float-row:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .float-row:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .float-row-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
