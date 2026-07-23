<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { formatRelativeTime } from "$lib/utils/time";

  interface Props {
    item: ClipboardItem;
    index: number;
    now: number;
    selected: boolean;
    onselect: (id: string) => void;
    ontoggleFavorite: (id: string) => void;
  }

  let { item, index, now, selected, onselect, ontoggleFavorite }: Props = $props();

</script>

<article class:selected class="clip-card">
  <button
    class="card-select"
    type="button"
    aria-label={`选择剪贴板记录：${item.title}`}
    aria-pressed={selected}
    onclick={() => onselect(item.id)}
  ></button>

  <div class="content">
    {#if item.kind === "image"}
      <div class="image-preview" aria-label={item.preview}>
        <div class="fake-sidebar"></div>
        <div class="fake-editor">
          <span></span><span></span><span></span><span></span>
          <i></i>
        </div>
      </div>
    {:else if item.kind === "file"}
      <div class="file-title">
        <span class="file-icon"><AppIcon name="file" size={15} /></span>
        <span>{item.fileName ?? item.title}</span>
      </div>
    {:else}
      <div class="text-preview">{item.title}</div>
      {#if item.preview}
        <div class="secondary-preview">{item.preview}</div>
      {/if}
    {/if}
  </div>

  <div class="meta-row">
    <span class:source-red={item.sourceTone === "red"} class:source-blue={item.sourceTone === "blue"} class:source-violet={item.sourceTone === "violet"} class="source-mark">
      {#if item.sourceTone === "neutral"}
        <AppIcon name={item.kind === "file" ? "file" : "clipboard"} size={12} />
      {:else}
        <span class="source-dot"></span>
      {/if}
    </span>
    <span class="source-name">{item.sourceApp}</span>
    <span>{item.sizeLabel}</span>
    {#if item.detailLabel}<span>{item.detailLabel}</span>{/if}
    <span>{formatRelativeTime(item.createdAt, now)}</span>
    {#if item.kind === "file"}<span class="file-count">▣ {item.preview}</span>{/if}
  </div>

  <div class="actions" aria-label="项目操作">
    <button type="button" title="复制" aria-label="复制"><AppIcon name="copy" size={16} /></button>
    {#if item.kind === "image" || item.kind === "file"}
      <button type="button" title="导出" aria-label="导出"><AppIcon name="download" size={16} /></button>
    {/if}
    <button
      type="button"
      class:active={item.favorite}
      title={item.favorite ? "取消收藏" : "收藏"}
      aria-label={item.favorite ? "取消收藏" : "收藏"}
      onclick={(event) => {
        event.stopPropagation();
        ontoggleFavorite(item.id);
      }}
    ><AppIcon name="star" size={16} filled={item.favorite} /></button>
    <button type="button" title="删除" aria-label="删除"><AppIcon name="trash" size={16} /></button>
  </div>

  <span class="shortcut">⌘{index + 1}</span>
</article>

<style>
  .clip-card {
    position: relative;
    padding: 13px 14px 12px;
    border: 1px solid transparent;
    border-radius: 10px;
    color: #ececec;
    background: transparent;
    cursor: default;
    transition: background 120ms ease, border-color 120ms ease;
  }

  .card-select {
    position: absolute;
    z-index: 0;
    inset: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    border-radius: inherit;
    background: transparent;
    cursor: default;
  }

  .clip-card:hover,
  .clip-card.selected {
    border-color: rgba(255, 255, 255, 0.035);
    background: #242424;
  }

  .content {
    position: relative;
    z-index: 1;
    min-width: 0;
    padding-right: 76px;
    pointer-events: none;
  }

  .text-preview {
    overflow: hidden;
    font-size: 13px;
    line-height: 1.55;
    white-space: pre-line;
    text-overflow: ellipsis;
  }

  .secondary-preview {
    margin-top: 4px;
    overflow: hidden;
    color: #8e8e8e;
    font-size: 12px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .file-title {
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
    font-size: 13px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .file-icon {
    display: inline-flex;
    color: #d7d7d7;
  }

  .image-preview {
    display: flex;
    width: min(100%, 380px);
    height: 82px;
    overflow: hidden;
    border: 1px solid #303237;
    border-radius: 6px;
    background: #17191d;
    box-shadow: inset 0 0 40px rgba(0, 0, 0, 0.3);
  }

  .fake-sidebar {
    width: 30%;
    border-right: 1px solid #292b31;
    background:
      linear-gradient(#292b31 0 0) 12px 13px / 60px 5px no-repeat,
      repeating-linear-gradient(to bottom, transparent 0 12px, #24262b 12px 14px) 12px 28px / 78% 42px no-repeat,
      #1e2024;
  }

  .fake-editor {
    position: relative;
    flex: 1;
    padding: 13px 16px;
  }

  .fake-editor span {
    display: block;
    width: 64%;
    height: 4px;
    margin-bottom: 8px;
    border-radius: 3px;
    background: #33363e;
  }

  .fake-editor span:nth-child(2) { width: 84%; }
  .fake-editor span:nth-child(3) { width: 48%; }
  .fake-editor span:nth-child(4) { width: 70%; }

  .fake-editor i {
    position: absolute;
    right: 16px;
    bottom: 13px;
    left: 16px;
    height: 3px;
    border-radius: 3px;
    background: linear-gradient(90deg, #705cff 0 72%, #2b2d33 72%);
  }

  .meta-row {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    margin-top: 10px;
    padding-right: 82px;
    color: #8c8c8c;
    font-size: 11.5px;
    white-space: nowrap;
    pointer-events: none;
  }

  .source-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    color: #d7c47b;
  }

  .source-dot {
    width: 10px;
    height: 10px;
    border-radius: 3px 6px 3px 6px;
    background: currentColor;
    transform: rotate(-12deg);
  }

  .source-red { color: #ff4655; }
  .source-blue { color: #66bde1; }
  .source-violet { color: #746dff; }
  .source-name { color: #aaaaaa; }
  .file-count { overflow: hidden; text-overflow: ellipsis; }

  .actions {
    position: absolute;
    z-index: 2;
    right: 34px;
    bottom: 9px;
    display: flex;
    gap: 2px;
    opacity: 0;
    transform: translateY(2px);
    transition: opacity 120ms ease, transform 120ms ease;
  }

  .clip-card:hover .actions,
  .clip-card.selected .actions,
  .actions:focus-within {
    opacity: 1;
    transform: translateY(0);
  }

  .actions button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 27px;
    height: 27px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    color: #777777;
    background: transparent;
    cursor: pointer;
  }

  .actions button:hover,
  .actions button.active {
    color: #f0cb42;
    background: #303030;
  }

  .shortcut {
    position: absolute;
    z-index: 1;
    right: 11px;
    bottom: 15px;
    color: #747474;
    font-size: 10px;
    pointer-events: none;
  }

  @media (max-width: 620px) {
    .content { padding-right: 40px; }
    .meta-row { padding-right: 28px; }
    .actions { display: none; }
  }
</style>
