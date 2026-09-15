<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { ClipboardItem, CardActionId } from "$lib/types/clipboard";
  import type { QuickAction } from "$lib/services/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { quickActionKind } from "$lib/utils/patterns";

  interface Props {
    item: ClipboardItem;
    index: number;
    quickCopyBadgeAlwaysVisible: boolean;
    contentActions: QuickAction[];
    dateViewIso: string | null;
    canEdit: boolean;
    canRestore: boolean;
    onquickaction: (event: MouseEvent, action: QuickAction) => void;
    onrunaction: (action: CardActionId, event: MouseEvent) => void;
    onsaveas: (event: MouseEvent) => void;
  }

  let {
    item,
    index,
    quickCopyBadgeAlwaysVisible,
    contentActions,
    dateViewIso,
    canEdit,
    canRestore,
    onquickaction,
    onrunaction,
    onsaveas,
  }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);
</script>

<div class="actions" aria-label={_t("card.itemActions")}>
  {#each contentActions as action (`${action.actionType}:${action.payload}`)}
    <button
      type="button"
      title={action.label}
      aria-label={action.label}
      aria-haspopup={action.actionType === "viewDate" ? "dialog" : undefined}
      aria-expanded={action.actionType === "viewDate" ? dateViewIso === action.payload : undefined}
      onclick={(event) => onquickaction(event, action)}
    >
      {#if quickActionKind(action) === "url"}
        <AppIcon name="globe" size={16} />
      {:else if quickActionKind(action) === "email"}
        <AppIcon name="mail" size={16} />
      {:else if quickActionKind(action) === "phone"}
        <AppIcon name="phone" size={16} />
      {:else if quickActionKind(action) === "date"}
        <AppIcon name="calendar" size={16} />
      {:else if quickActionKind(action) === "color"}
        <AppIcon name="palette" size={16} />
      {:else}
        <AppIcon name="copy" size={16} />
      {/if}
    </button>
  {/each}
  <button
    type="button"
    title={_t("card.viewDetail")}
    aria-label={_t("card.viewDetail")}
    onclick={(event) => onrunaction("detail", event)}><AppIcon name="eye" size={16} /></button
  >
  <button
    type="button"
    title={_t("card.copy")}
    aria-label={_t("card.copy")}
    onclick={(event) => onrunaction("copy", event)}><AppIcon name="copy" size={16} /></button
  >
  {#if item.kind === "image" || item.kind === "file"}
    <button
      type="button"
      title={_t("card.copyPath")}
      aria-label={_t("card.copyPath")}
      onclick={(event) => onrunaction("copyPath", event)}><AppIcon name="link" size={16} /></button
    >
  {/if}
  {#if item.kind === "image" || item.kind === "file"}
    <button
      type="button"
      title={_t("card.saveAs")}
      aria-label={_t("card.saveAs")}
      onclick={onsaveas}><AppIcon name="download" size={16} /></button
    >
  {/if}
  {#if canEdit}
    <button
      type="button"
      title={item.kind === "image" || item.kind === "file"
        ? _t("edit.editFileName")
        : _t("card.edit")}
      aria-label={item.kind === "image" || item.kind === "file"
        ? _t("edit.editFileName")
        : _t("card.edit")}
      onclick={(event) => onrunaction("edit", event)}><AppIcon name="edit" size={16} /></button
    >
  {/if}
  {#if item.kind === "text"}
    <button
      type="button"
      title={_t("card.pastePlain")}
      aria-label={_t("card.pastePlain")}
      onclick={(event) => onrunaction("plainpaste", event)}
      ><AppIcon name="type" size={16} /></button
    >
  {/if}
  {#if item.kind === "text" && item.htmlContent}
    <button
      type="button"
      title={_t("card.pasteFormat")}
      aria-label={_t("card.pasteFormat")}
      onclick={(event) => onrunaction("formatpaste", event)}
      ><AppIcon name="clipboard" size={16} /></button
    >
  {/if}
  {#if item.kind === "text"}
    <button
      type="button"
      title={_t("card.cleanPaste")}
      aria-label={_t("card.cleanPaste")}
      onclick={(event) => onrunaction("cleanpaste", event)}
      ><AppIcon name="scan" size={16} /></button
    >
  {/if}
  {#if !item.favorite}
    <button
      type="button"
      title={_t("card.delete")}
      aria-label={_t("card.delete")}
      onclick={(event) => onrunaction("delete", event)}><AppIcon name="trash" size={16} /></button
    >
  {/if}
  {#if item.deleted && canRestore}
    <button
      type="button"
      title={_t("card.restore")}
      aria-label={_t("card.restore")}
      onclick={(event) => onrunaction("restore", event)}
      ><AppIcon name="restore" size={16} /></button
    >
  {/if}
  <button
    type="button"
    class:active={item.favorite}
    title={item.favorite ? _t("card.unfavorite") : _t("card.favorite")}
    aria-label={item.favorite ? _t("card.unfavorite") : _t("card.favorite")}
    onclick={(event) => onrunaction("favorite", event)}
    ><AppIcon name="star" size={16} filled={item.favorite} /></button
  >
</div>
<span class="shortcut" class:shortcut-resident={quickCopyBadgeAlwaysVisible}
  >{index < 9 ? `⌘${index + 1}` : `#${index + 1}`}</span
>

<style>
  .actions {
    display: flex;
    flex: 0 0 auto;
    gap: 2px;
    margin-left: auto;
    opacity: 0;
    /* Hidden also removes the buttons from the Tab order, so an invisible
       control can never silently swallow keyboard focus (WCAG 2.4.7). */
    visibility: hidden;
    pointer-events: auto;
    transition:
      opacity 120ms ease,
      visibility 120ms ease;
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
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
  }

  .actions button:hover,
  .actions button.active {
    color: var(--warning-color);
    background: var(--hover-bg);
  }

  .shortcut {
    flex: 0 0 38px;
    width: 38px;
    margin-left: 2px;
    text-align: right;
    overflow: hidden;
    color: var(--text-faint);
    font-size: 11.5px;
    pointer-events: none;
    opacity: 0;
    transition: opacity 120ms ease;
  }
</style>
