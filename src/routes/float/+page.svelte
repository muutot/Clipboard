<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { PhysicalPosition } from "@tauri-apps/api/dpi";
  import { showToast } from "$lib/services/toast";
  import { emitTo, listen } from "@tauri-apps/api/event";
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import Toast from "$lib/components/common/Toast.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import {
    copyClipboardItem,
    loadClipboardHistory,
    pasteClipboardItem,
    persistDelete,
    persistFavorite,
    persistHardDelete,
  } from "$lib/services/clipboard";
  import { getDisplayTitle } from "$lib/utils/content/display-text";
  import { generalSettings } from "$lib/services/settings";
  import { applyGeneralSettingsToDocument } from "$lib/services/settings-bootstrap";
  import type {
    ClipboardItem,
    ClipboardItemsChangedPayload,
    FloatPanelClickAction,
  } from "$lib/types/clipboard";
  import {
    applyItemPatches,
    createItemStore,
    removeItems,
    replaceViewItems,
  } from "$lib/utils/store/item-store";
  import { createItemStoreView } from "$lib/utils/store/item-store-view.svelte";
  import { applyItemsChangedEvent } from "$lib/utils/store/item-changes";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  // The layout bootstraps theme/font styles once at load, but this webview
  // outlives the settings window: remote changes arrive through the store
  // (Tauri event or localStorage sync) and must be re-applied to the
  // document live, exactly like the settings page does.
  $effect(() => generalSettings.subscribe((v) => applyGeneralSettingsToDocument(v)));

  type FloatFilter = "all" | "favorite";

  let filter = $state<FloatFilter>("all");
  // The panel is a second window, so it cannot share the main route's store
  // instance — but it uses the same one so its mutations go through the same
  // funnel, and `clipboard-items-changed` keeps the two copies from drifting.
  const itemStore = createItemStoreView(createItemStore());
  const items = $derived(
    itemStore.history.filter((item) => !item.deleted && (filter !== "favorite" || item.favorite)),
  );
  let loading = $state(true);
  const copyingIds = new SvelteSet<string>();
  /** Guards against a stale response overwriting a newer filter's list. */
  let loadRequestId = 0;

  function setItems(next: ClipboardItem[]) {
    itemStore.current = replaceViewItems(itemStore.current, next, "history");
  }

  async function load() {
    if (!isTauriRuntime()) {
      loading = false;
      return;
    }
    const request = ++loadRequestId;
    // Stale-while-revalidate: keep the current list visible while it
    // refreshes (every focus triggers a reload) and only show the
    // placeholder on the very first load of an empty panel.
    if (items.length === 0) loading = true;
    try {
      const args = filter === "favorite" ? { favorite: true } : {};
      const result = (await loadClipboardHistory(100, 0, args)) ?? [];
      if (request === loadRequestId) setItems(result);
    } catch (error) {
      console.error("Unable to load float history", error);
      if (request === loadRequestId) setItems([]);
    } finally {
      if (request === loadRequestId) loading = false;
    }
  }

  function switchFilter(next: FloatFilter) {
    if (filter === next) return;
    filter = next;
    // Drop the old filter's rows instead of showing them stale-while-
    // revalidate: a click during the reload would otherwise copy an entry
    // from the previous filter. (Focus-triggered reloads keep SWR in
    // `load()`; only explicit filter switches clear.)
    setItems([]);
    void load();
  }

  async function copy(id: string) {
    const item = items.find((i) => i.id === id);
    // Guard per row, not globally: the template already disables only the
    // in-flight row, so a global guard silently swallowed clicks on other
    // rows with no feedback.
    if (!item || copyingIds.has(id)) return;
    copyingIds.add(id);
    try {
      await copyClipboardItem(item);
    } finally {
      copyingIds.delete(id);
    }
  }

  async function runClickAction(id: string, action: FloatPanelClickAction) {
    const item = items.find((i) => i.id === id);
    if (!item || action === "none") return;
    if (action === "copy") {
      await copy(id);
      return;
    }
    if (action === "copyPaste") {
      if (copyingIds.has(id)) return;
      copyingIds.add(id);
      try {
        // pasteClipboardItem already stages the clipboard content and
        // restores the previous app with a single toast; copying first
        // would write the clipboard twice and pop a second toast.
        await pasteClipboardItem(item, "auto");
      } finally {
        copyingIds.delete(id);
      }
      return;
    }
    if (action === "favorite") {
      const ok = await persistFavorite(id, !item.favorite);
      if (ok) {
        // The main window's optimistic path reuses the record the backend
        // broadcast; the panel holds the row itself, so patching the one record
        // in its store is the same funnel without a second round trip.
        itemStore.current = applyItemPatches(
          itemStore.current,
          new Map([[id, { favorite: !item.favorite }]]),
        );
        showToast(
          _t(item.favorite ? "toast.unfavoriteSuccess" : "toast.favoriteSuccess"),
          "success",
        );
      } else {
        showToast(_t("app.favoriteFailed"), "error");
      }
      return;
    }
    if (action === "detail") {
      try {
        const main = await WebviewWindow.getByLabel("main");
        if (!main) throw new Error("main window is unavailable");
        await main.show();
        await emitTo("main", "clipboard-open-detail", item);
        await main.setFocus();
      } catch (error) {
        console.error("Unable to open clipboard details", error);
        showToast(_t("float.detailFailed"), "error");
      }
      return;
    }
    if (action === "delete") {
      try {
        const ok = await ($generalSettings.useRecycleBin
          ? persistDelete(id)
          : persistHardDelete(id));
        if (!ok) throw new Error("record was not deleted");
        itemStore.current = removeItems(itemStore.current, new Set([id]));
        showToast(_t("toast.deleteSuccess"), "success");
      } catch {
        showToast(_t("app.deleteFailed"), "error");
      }
    }
  }

  function handleRowClick(id: string, event: MouseEvent) {
    if (event.button === 2) {
      event.preventDefault();
      void runClickAction(id, $generalSettings.floatPanelRightClick ?? "none");
      return;
    }
    if (event.button === 1) {
      event.preventDefault();
      void runClickAction(id, $generalSettings.floatPanelMiddleClick ?? "none");
      return;
    }
    void runClickAction(id, $generalSettings.floatPanelLeftClick ?? "copy");
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
    let unlistenItemsChanged: (() => void) | undefined;
    let unlistenHistoryInvalidated: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    if (isTauriRuntime()) {
      // The backend builds this window hidden so creation never races the
      // webview init or steals focus mid-gesture (that race wedged the
      // first paint as a stuck white window whenever the main window was
      // in front). Reveal from here instead: onMount only runs once the JS
      // runtime, DOM, and Tauri bridge are all live.
      void revealFloatPanel(() => !disposed);
      listen("clipboard-item-added", reload).then((unlisten) => {
        if (disposed) unlisten();
        else unlistenAdded = unlisten;
      });
      // A favorite or delete made in the main window has to land here too,
      // otherwise this panel keeps showing the row as it was. Same handler the
      // main route uses, so the two windows cannot drift apart.
      listen<ClipboardItemsChangedPayload>("clipboard-items-changed", (event) => {
        if (disposed) return;
        itemStore.current = applyItemsChangedEvent(itemStore.current, event.payload, {
          // This panel has no copy-side promotion of its own — it used to leave the
          // row where it was and let the next reload sort it out — so the usage
          // broadcast is what moves a copied entry up here.
          promoteUsed: $generalSettings.pinCopiedToTop,
        });
        // Patching loaded rows cannot add a newly favorited/restored row or
        // refill a page after deletion. Reload the filtered page and invalidate
        // any older request so it cannot overwrite the event we just applied.
        const payload = event.payload;
        if (
          payload.items?.length ||
          payload.deletedIds?.length ||
          payload.restoredIds?.length ||
          payload.removedIds?.length
        ) {
          reload();
        }
      }).then((unlisten) => {
        if (disposed) unlisten();
        else unlistenItemsChanged = unlisten;
      });
      listen<{ deletedIds?: string[] }>("clipboard-history-invalidated", (event) => {
        if (disposed) return;
        itemStore.current = removeItems(itemStore.current, new Set(event.payload.deletedIds ?? []));
        reload();
      }).then((unlisten) => {
        if (disposed) unlisten();
        else unlistenHistoryInvalidated = unlisten;
      });
      void resolveFloatWindow().then((win) => {
        if (disposed || !win) return;
        win
          .onFocusChanged(({ payload: focused }) => {
            if (focused) reload();
          })
          .then((unlisten) => {
            if (disposed) unlisten();
            else unlistenFocus = unlisten;
          })
          .catch(() => {});
      });
    }
    return () => {
      disposed = true;
      loadRequestId += 1;
      endHeaderDrag();
      unlistenAdded?.();
      unlistenItemsChanged?.();
      unlistenHistoryInvalidated?.();
      unlistenFocus?.();
    };
  });

  function close() {
    void resolveFloatWindow().then((win) => {
      if (!win) {
        showToast(_t("float.closeFailed"), "error");
        return;
      }
      return win.close().catch((error) => {
        console.error("Unable to close the float panel", error);
        showToast(_t("float.closeFailed"), "error");
      });
    });
  }

  /**
   * Own window handle, resolved by label rather than ambient context: the
   * label lookup goes through invoke (proven working in this webview) while
   * `getCurrentWindow()` metadata has proven unreliable here. Falls back to
   * the ambient window, then to null with visible feedback at call sites.
   */
  let floatWin: Window | null = null;

  async function resolveFloatWindow(): Promise<Window | null> {
    if (floatWin) return floatWin;
    if (!isTauriRuntime()) return null;
    try {
      floatWin = await WebviewWindow.getByLabel("float");
    } catch {
      floatWin = null;
    }
    if (!floatWin) {
      try {
        floatWin = getCurrentWindow();
      } catch {
        floatWin = null;
      }
    }
    return floatWin;
  }

  /**
   * Shows and focuses the own window. The backend builds it hidden (see
   * `open_float_panel`), so the first reveal happens here on mount. A
   * redundant show on an already-visible window is harmless: mount only
   * runs at creation (or page reload), never on plain show/hide toggles.
   * A refused focus is non-fatal: the visible panel stays usable and the
   * next toggle focuses it.
   */
  async function revealFloatPanel(isAlive: () => boolean): Promise<void> {
    const win = await resolveFloatWindow();
    if (!win || !isAlive()) return;
    try {
      await win.show();
    } catch (error) {
      console.error("Unable to show the float panel", error);
      return;
    }
    try {
      await win.setFocus();
    } catch {
      // Foreground rules may refuse the steal; visibility is what matters.
    }
  }

  /**
   * Header drag uses manual pointer tracking instead of the native drag
   * region: presses on empty header space (title/gaps) move the window via
   * `setPosition`, while presses resolving to a button keep native behavior
   * (target may be a text node, so walk up to the nearest element first).
   * Pointer capture on the header guarantees `pointerup`/`pointercancel`
   * delivery even when the cursor outruns the moving window (the async
   * `setPosition` round-trip lags fast mouse movement, so a plain `mouseup`
   * listener on the DOM window can miss the release and leave the panel
   * stuck following the cursor).
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

  // Cancels a drag whose async window lookup resolves after the panel closed
  // (or after another pointerdown), so its window-level listeners cannot leak.
  let dragRequestId = 0;

  async function startHeaderDrag(event: PointerEvent) {
    if (
      !isTauriRuntime() ||
      event.button !== 0 ||
      event.isPrimary === false ||
      isButtonPress(event)
    )
      return;
    if (dragState) return;
    const requestId = ++dragRequestId;
    // `event.currentTarget` is nulled once dispatch leaves this handler, and
    // the awaits below yield — capture the header reference up front so
    // pointer capture actually runs instead of always being skipped.
    const header = event.currentTarget instanceof Element ? event.currentTarget : null;
    const win = await resolveFloatWindow();
    if (requestId !== dragRequestId) return;
    if (!win) return;
    try {
      const [pos, scale] = await Promise.all([win.outerPosition(), win.scaleFactor()]);
      if (requestId !== dragRequestId) return;
      dragState = {
        startX: event.screenX,
        startY: event.screenY,
        winX: pos.x,
        winY: pos.y,
        scale,
      };
      try {
        header?.setPointerCapture(event.pointerId);
      } catch {
        // Pointer capture is best-effort; the window-level listeners below
        // still end the drag for releases inside the webview.
      }
      window.addEventListener("pointermove", onHeaderDragMove);
      window.addEventListener("pointerup", endHeaderDrag);
      window.addEventListener("pointercancel", endHeaderDrag);
      window.addEventListener("lostpointercapture", endHeaderDrag);
    } catch {
      dragState = null;
    }
  }

  function onHeaderDragMove(event: PointerEvent) {
    if (!dragState || !floatWin) return;
    if (event.buttons === 0 && event.pointerType === "mouse") {
      // Safety net: the release was missed (e.g. outside the webview), so a
      // button-less move must end the drag instead of following the cursor.
      endHeaderDrag();
      return;
    }
    const x = Math.round(dragState.winX + (event.screenX - dragState.startX) * dragState.scale);
    const y = Math.round(dragState.winY + (event.screenY - dragState.startY) * dragState.scale);
    void floatWin.setPosition(new PhysicalPosition(x, y)).catch(() => {});
  }

  function endHeaderDrag() {
    dragRequestId += 1;
    dragState = null;
    window.removeEventListener("pointermove", onHeaderDragMove);
    window.removeEventListener("pointerup", endHeaderDrag);
    window.removeEventListener("pointercancel", endHeaderDrag);
    window.removeEventListener("lostpointercapture", endHeaderDrag);
  }
</script>

<div class="float-shell">
  <!-- svelte-ignore a11y_no_static_element_interactions: header is a window drag handle; inner buttons keep native semantics -->
  <!-- No data-tauri-drag-region: the native modal drag loop would race the
       manual pointer tracking below and swallow its pointerup, leaving a
       stale dragState that blocks the next drag. -->
  <header class="float-header" onpointerdown={startHeaderDrag}>
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
          disabled={copyingIds.has(item.id)}
          onclick={(event) => handleRowClick(item.id, event)}
          oncontextmenu={(event) => {
            event.preventDefault();
            void runClickAction(item.id, $generalSettings.floatPanelRightClick ?? "none");
          }}
          onauxclick={(event) => {
            if (event.button === 1) {
              event.preventDefault();
              void runClickAction(item.id, $generalSettings.floatPanelMiddleClick ?? "none");
            }
          }}
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
