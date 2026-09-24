<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import TagChip from "$lib/components/TagChip.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";

  interface Props {
    item: ClipboardItem;
    tagColors?: Record<string, string>;
    onsavetags: (id: string, tags: string[]) => void;
  }

  let { item, tagColors = {}, onsavetags }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  let tagDraft = $state("");

  // Reset the draft only when the entry actually changes: a tag save or OCR
  // patch replaces the item object but must keep an in-progress draft.
  const currentItemId = $derived(item.id);

  $effect(() => {
    const itemId = currentItemId;
    void itemId;
    tagDraft = "";
  });

  function addTagFromInput() {
    const value = tagDraft.trim();
    if (!value) return;
    const current = item.tags ?? [];
    if (current.some((t) => t === value)) {
      tagDraft = "";
      return;
    }
    onsavetags(item.id, [...current, value]);
    tagDraft = "";
  }

  function removeTag(tag: string) {
    onsavetags(
      item.id,
      (item.tags ?? []).filter((t) => t !== tag),
    );
  }
</script>

<div class="tags-tab">
  {#if (item.tags ?? []).length > 0}
    <div class="tags-list">
      {#each item.tags ?? [] as tag (tag)}
        <div class="tag-row">
          <TagChip {tag} accent={tagColors[tag]} />
          <button
            type="button"
            class="tag-remove"
            aria-label={_t("detail.removeTag")}
            title={_t("detail.removeTag")}
            onclick={() => removeTag(tag)}
          >
            <AppIcon name="x" size={12} />
          </button>
        </div>
      {/each}
    </div>
  {:else}
    <p class="tags-empty">{_t("detail.noTags")}</p>
  {/if}
  <div class="tag-input-wrap">
    <input
      bind:value={tagDraft}
      placeholder={_t("detail.addTagPlaceholder")}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          addTagFromInput();
        }
      }}
    />
    <button
      type="button"
      class="tag-add"
      aria-label={_t("detail.addTag")}
      disabled={!tagDraft.trim()}
      onclick={addTagFromInput}><AppIcon name="plus" size={12} /></button
    >
  </div>
</div>

<style>
  .tags-tab {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .tags-list {
    display: grid;
    gap: 6px;
  }

  .tag-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--input-bg);
  }

  .tag-row :global(.tag-chip) {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tag-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text-faint);
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    transition: color 100ms ease;
  }

  .tag-remove:hover {
    color: var(--danger-color);
    background: var(--hover-bg);
  }

  .tags-empty {
    margin: 0;
    color: var(--text-muted);
    font-size: 11px;
  }

  /* Single rounded control that mirrors the shared settings input look
     (`--input-bg`, `--border-color`, control radius) with the add action
     embedded as a trailing ghost button. */
  .tag-input-wrap {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 5px 0 10px;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--input-bg);
    transition: border-color 120ms ease;
  }

  .tag-input-wrap:focus-within {
    border-color: var(--text-faint);
  }

  .tag-input-wrap input {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    outline: none;
    color: var(--text-primary);
    background: transparent;
    font: inherit;
    font-size: 12px;
  }

  .tag-input-wrap input::placeholder {
    color: var(--placeholder-color);
    opacity: 1;
  }

  .tag-add {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    padding: 0;
    border: 0;
    border-radius: 6px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
    transition:
      color 120ms ease,
      background 120ms ease;
  }

  .tag-add:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .tag-add:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
