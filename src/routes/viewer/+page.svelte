<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  let src = $state($page.url.searchParams.get("src") ?? "");
  let opacity = $state(
    Math.min(1, Math.max(0, Number($page.url.searchParams.get("opacity")) || 0.92)),
  );

  const viewerSrc = $derived(
    src ? convertFileSrc(src.replace(/\\/g, "/")) : undefined,
  );

  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let isDragging = $state(false);
  let dragStartX = 0;
  let dragStartY = 0;
  let panStartX = 0;
  let panStartY = 0;

  let windowLabel = "image-viewer";

  async function hideWindow() {
    try {
      const w = await WebviewWindow.getByLabel(windowLabel);
      if (w) { await w.hide(); return; }
    } catch {}
  }

  onMount(() => {
    function onEscape(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        hideWindow();
      }
    }
    document.addEventListener("keydown", onEscape, true);

    let unlisten: UnlistenFn | undefined;
    let alive = true;
    import("@tauri-apps/api/event").then(({ listen }) => {
      if (!alive) return;
      listen<{ src: string; opacity: number }>("viewer:open", (event) => {
        src = event.payload.src;
        opacity = event.payload.opacity;
        zoom = 1;
        panX = 0;
        panY = 0;
      }).then((fn) => { if (alive) unlisten = fn; });
    });

    return () => {
      alive = false;
      document.removeEventListener("keydown", onEscape, true);
      if (unlisten) unlisten();
    };
  });

  function onBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) hideWindow();
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 0.9 : 1.1;
    zoom = Math.min(20, Math.max(0.1, zoom * delta));
  }

  function onMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    isDragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    panStartX = panX;
    panStartY = panY;
  }

  function onMouseMove(e: MouseEvent) {
    if (!isDragging) return;
    panX = panStartX + (e.clientX - dragStartX);
    panY = panStartY + (e.clientY - dragStartY);
  }

  function onMouseUp() {
    isDragging = false;
  }

  function onDblClick() {
    if (zoom !== 1 || panX !== 0 || panY !== 0) {
      zoom = 1;
      panX = 0;
      panY = 0;
    } else {
      zoom = 2;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="viewer-overlay"
  style:background={opacity >= 1 ? "rgba(0,0,0,1)" : `rgba(0,0,0,${opacity})`}
  onclick={onBackdropClick}
  onwheel={onWheel}
  onmousedown={onMouseDown}
  onmousemove={onMouseMove}
  onmouseup={onMouseUp}
  onmouseleave={onMouseUp}
  ondblclick={onDblClick}
  onkeydown={(e) => { if (e.key === "Escape") hideWindow(); }}
>
  <button type="button" class="close-btn" onclick={hideWindow} aria-label="Close">
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
      <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  </button>
  <div class="zoom-hint">{Math.round(zoom * 100)}%</div>
  {#if viewerSrc}
    <img
      class="viewer-image"
      class:dragging={isDragging}
      src={viewerSrc}
      alt=""
      draggable="false"
      style="transform: translate({panX}px, {panY}px) scale({zoom})"
    />
  {/if}
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  .viewer-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    user-select: none;
  }

  .viewer-overlay:active {
    cursor: grabbing;
  }

  .close-btn {
    position: fixed;
    top: 16px;
    right: 16px;
    z-index: 10;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 1px solid #555;
    border-radius: 8px;
    color: #ddd;
    background: rgba(30, 30, 30, 0.7);
    backdrop-filter: blur(6px);
    cursor: pointer;
    transition: color 120ms ease, background 120ms ease;
  }

  .close-btn:hover {
    color: #fff;
    background: rgba(60, 60, 60, 0.8);
  }

  .zoom-hint {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 10;
    padding: 4px 14px;
    border-radius: 6px;
    color: #bbb;
    background: rgba(30, 30, 30, 0.7);
    backdrop-filter: blur(6px);
    font-size: 12px;
    font-family: system-ui, sans-serif;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }

  .viewer-image {
    max-width: 90vw;
    max-height: 90vh;
    object-fit: contain;
    transform-origin: center center;
    transition: transform 0.05s linear;
    pointer-events: none;
  }

  .viewer-image.dragging {
    transition: none;
  }
</style>
