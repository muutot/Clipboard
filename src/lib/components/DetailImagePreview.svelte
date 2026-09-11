<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { assetUrl } from "$lib/utils/format";

  interface Props {
    item: ClipboardItem;
    onimagefullscreen?: (id: string) => void;
  }

  let { item, onimagefullscreen }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);
</script>

<div class="image-full-preview">
  {#if assetUrl(item.previewPath || item.resourcePath)}
    <img src={assetUrl(item.previewPath || item.resourcePath)} alt={item.preview || item.title} />
    <button
      type="button"
      class="image-fullscreen-btn"
      onclick={(e) => {
        e.stopPropagation();
        onimagefullscreen?.(item.id);
      }}
      aria-label={_t("detail.fullscreenPreview")}
    >
      <AppIcon name="maximize" size={16} strokeWidth={2} />
    </button>
  {:else}
    <div class="image-placeholder">
      <AppIcon name="image" size={48} strokeWidth={1.5} />
      {#if item.imageMeta}
        <span class="image-meta">{item.imageMeta.width} × {item.imageMeta.height}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .image-full-preview {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    min-height: 180px;
    padding: 16px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    color: var(--text-muted);
    background: var(--input-bg);
  }

  .image-full-preview img {
    max-width: 100%;
    max-height: 400px;
    object-fit: contain;
    border-radius: 4px;
  }

  .image-placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .image-meta {
    color: var(--text-muted);
    font-size: 11px;
  }

  .image-full-preview {
    position: relative;
  }

  .image-fullscreen-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-secondary);
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(4px);
    cursor: pointer;
    opacity: 0;
    transition:
      opacity 150ms ease,
      background 150ms ease;
  }

  .image-full-preview:hover .image-fullscreen-btn {
    opacity: 1;
  }

  .image-fullscreen-btn:hover {
    color: var(--text-primary);
    background: rgba(0, 0, 0, 0.75);
  }
</style>
