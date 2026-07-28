<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { assetUrl } from "$lib/utils/format";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    filePath: string;
    opacity: number;
    onclose: () => void;
  }

  let { filePath, opacity, onclose }: Props = $props();

  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let isDragging = $state(false);
  let dragStartX = 0;
  let dragStartY = 0;
  let panStartX = 0;
  let panStartY = 0;

  onMount(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        e.stopImmediatePropagation();
        onclose();
      }
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

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

<div
  class="image-viewer-overlay"
  style="background: rgba(0, 0, 0, {opacity})"
  onwheel={onWheel}
  onmousedown={onMouseDown}
  onmousemove={onMouseMove}
  onmouseup={onMouseUp}
  onmouseleave={onMouseUp}
  ondblclick={onDblClick}
>
  <button
    type="button"
    class="viewer-close-btn"
    onclick={onclose}
    aria-label={_t("actions.close")}
  >
    <AppIcon name="x" size={20} strokeWidth={2.5} />
  </button>
  <div class="viewer-zoom-hint">{Math.round(zoom * 100)}%</div>
  <img
    class="viewer-image"
    class:dragging={isDragging}
    src={assetUrl(filePath)}
    alt=""
    draggable="false"
    style="transform: translate({panX}px, {panY}px) scale({zoom})"
  />
</div>

<style>
  .image-viewer-overlay {
    position: fixed;
    z-index: 200;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    user-select: none;
  }

  .image-viewer-overlay:active {
    cursor: grabbing;
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

  .viewer-close-btn {
    position: fixed;
    top: 16px;
    right: 16px;
    z-index: 201;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    color: var(--text-secondary);
    background: rgba(30, 30, 30, 0.7);
    backdrop-filter: blur(6px);
    cursor: pointer;
    transition:
      color 120ms ease,
      background 120ms ease;
  }

  .viewer-close-btn:hover {
    color: var(--text-primary);
    background: rgba(60, 60, 60, 0.8);
  }

  .viewer-zoom-hint {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 201;
    padding: 4px 14px;
    border-radius: 6px;
    color: var(--text-secondary);
    background: rgba(30, 30, 30, 0.7);
    backdrop-filter: blur(6px);
    font-size: 12px;
    pointer-events: none;
  }
</style>
