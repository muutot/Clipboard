<script lang="ts">
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { assetUrl } from "$lib/utils/format";
  import { ocrBlockMatches, ocrBlockRect } from "$lib/utils/ocr-blocks";
  let {
    item,
    query = "",
    selected = new Set<number>(),
    ontoggle,
    src,
  }: {
    item: ClipboardItem;
    query?: string;
    selected?: ReadonlySet<number>;
    ontoggle?: (index: number) => void;
    src?: string;
  } = $props();
</script>

<div class="ocr-image">
  <img
    src={src ?? assetUrl(item.previewPath || item.resourcePath)}
    alt={item.preview || item.title}
  />
  {#if item.imageMeta}
    {#each item.ocrBlocks ?? [] as block, index}
      {@const rect = ocrBlockRect(block, item.imageMeta.width, item.imageMeta.height)}
      {@const matched = ocrBlockMatches(block, query)}
      {#if rect && (ontoggle || matched)}
        {#if ontoggle}
          <button
            type="button"
            class="ocr-box"
            class:matched
            class:selected={selected.has(index)}
            style:left={rect.left + "%"}
            style:top={rect.top + "%"}
            style:width={rect.width + "%"}
            style:height={rect.height + "%"}
            aria-label={block.text}
            title={block.text}
            aria-pressed={selected.has(index)}
            onclick={() => ontoggle?.(index)}
          ></button>
        {:else}
          <span
            class="ocr-box matched"
            style:left={rect.left + "%"}
            style:top={rect.top + "%"}
            style:width={rect.width + "%"}
            style:height={rect.height + "%"}
            title={block.text}
          ></span>
        {/if}
      {/if}
    {/each}
  {/if}
</div>

<style>
  .ocr-image {
    position: relative;
    width: fit-content;
    max-width: 100%;
    line-height: 0;
  }
  img {
    display: block;
    max-width: 100%;
    max-height: 400px;
    object-fit: contain;
    border-radius: 4px;
  }
  .ocr-box {
    position: absolute;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 2px;
    background: transparent;
    min-width: 0;
    min-height: 0;
  }
  button.ocr-box {
    cursor: pointer;
  }
  .matched {
    border-color: var(--warning-color);
    background: color-mix(in srgb, var(--warning-color) 24%, transparent);
  }
  .selected {
    border: 2px solid var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 32%, transparent);
  }
  .ocr-box:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
