<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
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

  const presets = [
    "#e5484d",
    "#f76b15",
    "#ffb224",
    "#46a758",
    "#3e63dd",
    "#8e4ec6",
    "#00a2c7",
    "#5c7cfa",
    "#d6409f",
    "#12a594",
    "#ad5700",
    "#6b7280",
    "#84cc16",
    "#d946ef",
    "#f8fafc",
  ];

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

  async function pickColor(tag: TagInfo, color: string) {
    const next = tag.color === color ? "" : color;
    colorPopover = null;
    const ok = await setTagColor(tag.name, next);
    if (ok) {
      const index = tags.findIndex((t) => t.name === tag.name);
      if (index >= 0) tags[index] = { ...tags[index], color: next };
      notify(next ? _t("tags.colorSaved") : _t("tags.saved"));
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
    const popHeight = colorPopoverEl.offsetHeight;
    const popWidth = colorPopoverEl.offsetWidth;
    const gap = 4;
    const topBelow = rect.bottom + gap;
    const topAbove = rect.top - gap - popHeight;
    const fitsBelow = topBelow + popHeight <= window.innerHeight - 8;
    const fitsAbove = topAbove >= 8;
    popoverTop = fitsBelow || !fitsAbove ? topBelow : topAbove;
    popoverLeft = Math.max(8, Math.min(rect.left, window.innerWidth - 8 - popWidth));
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
        <div class="tag-color-grid">
          {#each presets as color (color)}
            <button
              type="button"
              class="tag-swatch-option"
              class:active={currentTag.color === color}
              style={`--swatch: ${color}`}
              aria-label={color}
              title={color}
              onclick={() => pickColor(currentTag, color)}
            ></button>
          {/each}
          <label
            class="tag-swatch-option tag-swatch-custom"
            class:active={currentTag.color !== "" && !presets.includes(currentTag.color)}
            style={currentTag.color && !presets.includes(currentTag.color)
              ? `--swatch: ${currentTag.color}`
              : undefined}
            title={_t("tags.customColor")}
            aria-label={_t("tags.customColor")}
          >
            <input
              type="color"
              value={/^#[0-9a-fA-F]{6}$/.test(currentTag.color) ? currentTag.color : "#5c7cfa"}
              onchange={(e) => pickColor(currentTag, e.currentTarget.value)}
            />
            <AppIcon name="palette" size={12} />
          </label>
        </div>
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

  .tag-color-grid {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 6px;
    justify-items: center;
    align-items: center;
  }

  .tag-color-grid .tag-swatch-option {
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 50%;
    background: var(--surface-bg);
    cursor: pointer;
  }

  .tag-color-grid .tag-swatch-option[style*="--swatch"] {
    background: var(--swatch);
  }

  .tag-color-grid .tag-swatch-custom {
    position: relative;
    overflow: hidden;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-secondary);
    background: conic-gradient(
      from 180deg,
      #e5484d,
      #f76b15,
      #ffb224,
      #46a758,
      #3e63dd,
      #8e4ec6,
      #00a2c7,
      #e5484d
    );
  }

  .tag-color-grid .tag-swatch-custom input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    opacity: 0;
    cursor: pointer;
  }

  .tag-color-grid .tag-swatch-custom.active {
    color: var(--text-primary);
  }

  .tag-color-grid .tag-swatch-option.active {
    outline: 2px solid var(--text-primary);
    outline-offset: 1px;
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
