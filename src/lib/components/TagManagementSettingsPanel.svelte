<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import TagColorPicker from "$lib/components/TagColorPicker.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { resolveFixedPopoverPosition } from "$lib/utils/dropdown";
  import type { TagsChangedPayload } from "$lib/types/clipboard";
  import {
    deleteTag,
    listAllTags,
    renameTag,
    setTagColor,
    type TagInfo,
  } from "$lib/services/clipboard";
  import { emit, listen } from "@tauri-apps/api/event";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  let suppressTagsChangedReload = false;

  function emitTagsChanged(payload: TagsChangedPayload) {
    suppressTagsChangedReload = true;
    emit("tags-changed", payload)
      .catch((err) => console.warn("tags-changed emit failed:", err))
      .finally(() => {
        setTimeout(() => {
          suppressTagsChangedReload = false;
        }, 0);
      });
  }

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
    tagSearch?: string;
    ontagSearchChange?: (value: string) => void;
  }

  let {
    onclose,
    showHeader = true,
    tagSearch = "",
    ontagSearchChange = () => {},
  }: Props = $props();

  let tags = $state<TagInfo[]>([]);
  let loading = $state(true);
  let feedback = $state<{ message: string; kind: "success" | "error" } | null>(null);
  let confirmDelete = $state<Record<string, boolean>>({});
  let feedbackTimer: ReturnType<typeof setTimeout> | undefined;

  let colorPopover = $state<string | null>(null);
  let colorTriggerEl = $state<HTMLButtonElement | null>(null);
  let colorPopoverEl = $state<HTMLDivElement | null>(null);
  let popoverTop = $state(0);
  let popoverLeft = $state(0);

  const currentTag = $derived(tags.find((t) => t.name === colorPopover) ?? null);

  const filteredTags = $derived.by(() => {
    const query = tagSearch.trim().toLowerCase();
    if (!query) return tags;
    return tags.filter((t) => t.name.toLowerCase().includes(query));
  });

  onMount(() => {
    let disposed = false;
    let unlistenTagsChanged: (() => void) | undefined;
    void load();
    listen<TagsChangedPayload>("tags-changed", () => {
      if (suppressTagsChangedReload) return;
      void load();
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenTagsChanged = unlisten;
    });
    return () => {
      disposed = true;
      unlistenTagsChanged?.();
    };
  });

  async function load() {
    loading = true;
    const result = await listAllTags();
    tags = (result ?? []).sort((a, b) => a.name.localeCompare(b.name));
    loading = false;
  }

  function notify(message: string, kind: "success" | "error" = "success") {
    clearTimeout(feedbackTimer);
    feedback = { message, kind };
    feedbackTimer = setTimeout(() => (feedback = null), 3000);
  }

  async function saveColor(tag: TagInfo, color: string) {
    colorPopover = null;
    const ok = await setTagColor(tag.name, color);
    if (ok) {
      const index = tags.findIndex((t) => t.name === tag.name);
      if (index >= 0) tags[index] = { ...tags[index], color };
      notify(color ? _t("tags.colorSaved") : _t("tags.saved"));
      emitTagsChanged({});
    }
  }

  function toggleColorPopover(name: string, event: MouseEvent) {
    if (colorPopover === name) {
      colorPopover = null;
      return;
    }
    colorPopover = name;
    colorTriggerEl = event.currentTarget as HTMLButtonElement;
  }

  function positionColorPopover() {
    if (!colorPopover || !colorTriggerEl || !colorPopoverEl) return;
    const rect = colorTriggerEl.getBoundingClientRect();
    const position = resolveFixedPopoverPosition(
      rect,
      colorPopoverEl.offsetWidth,
      colorPopoverEl.offsetHeight,
    );
    popoverTop = position.top;
    popoverLeft = position.left;
  }

  function onColorScroll(e: Event) {
    if (colorPopoverEl && e.target instanceof Node && colorPopoverEl.contains(e.target)) return;
    colorPopover = null;
  }

  function onColorKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") colorPopover = null;
  }

  $effect(() => {
    if (!colorPopover) return;
    positionColorPopover();
    window.addEventListener("resize", positionColorPopover);
    window.addEventListener("scroll", onColorScroll, true);
    window.addEventListener("keydown", onColorKeydown, true);
    return () => {
      window.removeEventListener("resize", positionColorPopover);
      window.removeEventListener("scroll", onColorScroll, true);
      window.removeEventListener("keydown", onColorKeydown, true);
    };
  });

  async function commitRename(tag: TagInfo, draft: string) {
    const name = draft.trim();
    if (!name || name === tag.name) {
      if (!name) load();
      return;
    }
    if (tags.some((t) => t.name !== tag.name && t.name === name)) {
      notify(_t("tags.renameConflict"), "error");
      load();
      return;
    }
    await renameTag(tag.name, name);
    notify(_t("tags.renamed"));
    emitTagsChanged({ renamed: { old: tag.name, new: name } });
    void load();
  }

  async function commitDelete(tag: TagInfo) {
    if (!confirmDelete[tag.name]) {
      confirmDelete = { ...confirmDelete, [tag.name]: true };
      return;
    }
    await deleteTag(tag.name);
    notify(_t("tags.deleted"));
    emitTagsChanged({ deleted: tag.name });
    void load();
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("tags.title")}</span>
      <h2>{_t("tags.title")}</h2>
      <p>{_t("tags.description")}</p>
    </div>
    <div class="header-actions">
      {#if !loading && tags.length > 0}
        <label class="search-field">
          <AppIcon name="search" size={15} />
          <input
            type="search"
            value={tagSearch}
            placeholder={_t("tags.searchPlaceholder")}
            aria-label={_t("tags.searchPlaceholder")}
            oninput={(e) => ontagSearchChange((e.currentTarget as HTMLInputElement).value)}
          />
        </label>
      {/if}
      <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
        >×</button
      >
    </div>
  </header>
{/if}

<div class="settings-scroll">
  {#if loading}
    <p class="auto-save-note">{_t("status.searching")}</p>
  {:else if tags.length === 0}
    <section class="setting-card">
      <p class="auto-save-note">{_t("tags.empty")}</p>
    </section>
  {:else if filteredTags.length === 0}
    <section class="setting-card">
      <p class="auto-save-note">{_t("tags.noResults")}</p>
    </section>
  {:else}
    {#each filteredTags as tag (tag.name)}
      <section class="setting-card tag-row">
        <button
          type="button"
          class="tag-color-trigger"
          style={tag.color ? `--tag-accent: ${tag.color}` : undefined}
          aria-haspopup="dialog"
          aria-expanded={colorPopover === tag.name}
          aria-label={_t("tags.color")}
          title={_t("tags.color")}
          onclick={(e) => toggleColorPopover(tag.name, e)}
        ></button>
        <input
          class="tag-name-input"
          value={tag.name}
          aria-label={_t("tags.renamePlaceholder")}
          onblur={(e) => commitRename(tag, (e.currentTarget as HTMLInputElement).value)}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              (e.currentTarget as HTMLInputElement).blur();
            } else if (e.key === "Escape") {
              load();
            }
          }}
        />
        <span class="tag-sep" aria-hidden="true"></span>
        <span class="tag-count">{_t("tags.count", { count: tag.count })}</span>
        <button
          type="button"
          class="tag-delete"
          class:confirm={confirmDelete[tag.name]}
          onclick={() => commitDelete(tag)}
        >
          {confirmDelete[tag.name] ? _t("tags.deleteConfirm") : _t("tags.delete")}
        </button>
      </section>
    {/each}

    {#if colorPopover && currentTag}
      <div
        class="tag-color-popover popover-surface"
        role="group"
        aria-label={_t("tags.color")}
        style="top: {popoverTop}px; left: {popoverLeft}px;"
        bind:this={colorPopoverEl}
      >
        <div
          class="custom-select-backdrop"
          onclick={() => (colorPopover = null)}
          aria-hidden="true"
        ></div>
        <TagColorPicker
          value={currentTag.color}
          customLabel={_t("tags.customColor")}
          size={20}
          gap={6}
          onchange={(color) => void saveColor(currentTag, color)}
        />
      </div>
    {/if}
  {/if}

  {#if feedback}
    <p class="settings-feedback" class:success={feedback.kind === "success"} role="status">
      {feedback.message}
    </p>
  {/if}
</div>

<style>
  .header-actions {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .search-field {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 180px;
    padding: 7px 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-faint);
    background: var(--input-bg);
  }

  .search-field input {
    min-width: 0;
    flex: 1;
    border: 0;
    outline: 0;
    color: var(--text-primary);
    background: transparent;
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  .tag-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .tag-color-trigger {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 50%;
    background: color-mix(in srgb, var(--tag-accent) 45%, var(--surface-bg));
    cursor: pointer;
  }

  .tag-color-trigger:hover {
    box-shadow: 0 0 0 2px var(--hover-bg);
  }

  .tag-color-trigger[aria-expanded="true"] {
    outline: 2px solid var(--text-faint);
    outline-offset: 1px;
  }

  .tag-name-input {
    flex: 1;
    min-width: 0;
    padding: 4px 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-primary);
    background: transparent;
    font-size: 13px;
    font-weight: 600;
  }

  .tag-sep {
    flex-shrink: 0;
    width: 1px;
    height: 14px;
    background: var(--border-subtle);
  }

  .tag-name-input:hover {
    border-color: var(--border-color);
  }

  .tag-name-input:focus {
    outline: none;
    border-color: var(--text-faint);
    background: var(--input-bg, var(--surface-bg));
  }

  .tag-count {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-faint);
    white-space: nowrap;
  }

  .tag-color-popover {
    position: fixed;
    padding: 10px;
  }

  .tag-delete {
    flex-shrink: 0;
    padding: 5px 10px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-secondary);
    background: transparent;
    font-size: 12px;
    cursor: pointer;
  }

  .tag-delete:hover {
    color: var(--danger-color);
    border-color: var(--danger-color);
  }

  .tag-delete.confirm {
    color: var(--danger-color);
    border-color: var(--danger-color);
  }
</style>
