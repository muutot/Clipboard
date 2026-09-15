<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import ClipboardCard from "$lib/components/ClipboardCard.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import type { VirtualListResult } from "$lib/utils/virtual-scroll";

  interface Props {
    items: ClipboardItem[];
    /**
     * Whether the filtered result set is non-empty. The empty state must key
     * off the full filtered count, not `items` (the virtual-scroll page
     * slice), otherwise a zero-height container yields an empty slice and the
     * list never mounts to be measured.
     */
    hasItems: boolean;
    useVirtualScroll: boolean;
    virtualList: VirtualListResult;
    indexById: Map<string, number>;
    currentTime: number;
    selectedIds: Set<string>;
    selectedId: string;
    splitDetail: boolean;
    cardPaddingTop: number;
    cardPaddingBottom: number;
    cardGap: number;
    cardBorderRadius: number;
    maxTextLines: number;
    showSecondaryText: boolean;
    alwaysShowActions: boolean;
    quickCopyBadgeAlwaysVisible: boolean;
    doubleClickPaste: boolean;
    tagColors: Record<string, string>;
    tagAddSignal: number;
    panelLabel: string;
    emptyTitle: string;
    emptyHint: string;
    cardHeightFor: (item: ClipboardItem) => number;
    cardLayoutSignature: (item: ClipboardItem) => string;
    onscroll: () => void;
    onheightchange: (id: string, height: number, immediate?: boolean) => void;
    onselect: (id: string, event?: MouseEvent) => void;
    ontoggleSelect: (id: string) => void;
    ontoggleFavorite: (id: string) => void;
    ondelete: (id: string) => void;
    oncopy: (id: string) => void;
    onsave: (id: string) => void;
    ondetail: (id: string) => void;
    onimagefullscreen: (id: string) => void;
    onmaterialize: (id: string) => void;
    onedit: (id: string) => void;
    onsaveedit: (id: string, content: string) => void | Promise<boolean>;
    onsaveasnew: (id: string, title: string, content: string) => void;
    oncanceledit: (id: string) => void;
    onplainpaste: (id: string) => void;
    onformatpaste: (id: string) => void;
    oncleanpaste: (id: string) => void;
    ondblclickpaste: (id: string) => void;
    onrestore: (id: string) => void;
    oncopyPath: (id: string) => void;
    onsavetags: (id: string, tags: string[]) => void;
    ontoggleTagFilter: (tag: string) => void;
    oneditTag: (tag: string) => void;
    listEl?: HTMLElement | null;
  }

  let {
    items,
    hasItems,
    useVirtualScroll,
    virtualList,
    indexById,
    currentTime,
    selectedIds,
    selectedId,
    splitDetail,
    cardPaddingTop,
    cardPaddingBottom,
    cardGap,
    cardBorderRadius,
    maxTextLines,
    showSecondaryText,
    alwaysShowActions,
    quickCopyBadgeAlwaysVisible,
    doubleClickPaste,
    tagColors,
    tagAddSignal,
    panelLabel,
    emptyTitle,
    emptyHint,
    cardHeightFor,
    cardLayoutSignature,
    onscroll,
    onheightchange,
    onselect,
    ontoggleSelect,
    ontoggleFavorite,
    ondelete,
    oncopy,
    onsave,
    ondetail,
    onimagefullscreen,
    onmaterialize,
    onedit,
    onsaveedit,
    onsaveasnew,
    oncanceledit,
    onplainpaste,
    onformatpaste,
    oncleanpaste,
    ondblclickpaste,
    onrestore,
    oncopyPath,
    onsavetags,
    ontoggleTagFilter,
    oneditTag,
    listEl = $bindable(null),
  }: Props = $props();
</script>

<section class="history-panel" aria-label={panelLabel}>
  <div class="section-heading"></div>
  {#snippet historyCard(item: ClipboardItem)}
    <ClipboardCard
      {item}
      index={indexById.get(item.id) ?? 0}
      now={currentTime}
      selected={selectedIds.has(item.id) || item.id === selectedId}
      checked={selectedIds.has(item.id)}
      showCheckbox={false}
      hideActions={selectedIds.size > 0 || splitDetail}
      hideMetaRow={splitDetail}
      {cardPaddingTop}
      {cardPaddingBottom}
      {cardGap}
      {cardBorderRadius}
      cardHeight={cardHeightFor(item)}
      {maxTextLines}
      {showSecondaryText}
      {alwaysShowActions}
      quickCopyBadgeAlwaysVisible={quickCopyBadgeAlwaysVisible && !splitDetail}
      {onheightchange}
      heightMeasurementKey={cardLayoutSignature(item)}
      {onselect}
      {ontoggleSelect}
      {ontoggleFavorite}
      {ondelete}
      {oncopy}
      {onsave}
      {ondetail}
      {onimagefullscreen}
      {onmaterialize}
      {onedit}
      {onsaveedit}
      {onsaveasnew}
      {oncanceledit}
      {onplainpaste}
      {onformatpaste}
      {oncleanpaste}
      {ondblclickpaste}
      {doubleClickPaste}
      {onrestore}
      {oncopyPath}
      {onsavetags}
      {tagColors}
      {ontoggleTagFilter}
      {oneditTag}
      tagAddSignal={selectedId === item.id ? tagAddSignal : 0}
    />
  {/snippet}

  {#if hasItems}
    <div class="history-list" role="listbox" aria-label={panelLabel} bind:this={listEl} {onscroll}>
      <div
        class="virtual-container"
        style="height: {useVirtualScroll
          ? virtualList.totalHeight + 'px'
          : 'auto'}; position: {useVirtualScroll ? 'relative' : 'static'};"
      >
        {#each items as item, visibleIdx (item.id)}
          {#if useVirtualScroll}
            <div
              style="position: absolute; top: {virtualList.visibleItems[visibleIdx]
                .top}px; left: 0; right: 0;"
            >
              {@render historyCard(item)}
            </div>
          {:else}
            {@render historyCard(item)}
          {/if}
        {/each}
      </div>
    </div>
  {:else}
    <div class="empty-state">
      <span class="empty-icon"><AppIcon name="clipboard" size={28} /></span>
      <strong>{emptyTitle}</strong>
      <p>{emptyHint}</p>
    </div>
  {/if}
</section>

<style>
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
    color: var(--text-muted);
    font-size: 11.5px;
  }

  .history-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 4px 6px;
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
    color: var(--text-muted);
    text-align: center;
  }

  .empty-icon {
    display: inline-flex;
    margin-bottom: 12px;
    color: var(--text-faint);
  }

  .empty-state strong {
    color: var(--text-secondary);
    font-size: 14px;
  }

  .empty-state p {
    margin: 6px 0;
    font-size: 12px;
  }
</style>
