<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import SearchField from "$lib/components/SearchField.svelte";
  import TagColorPicker from "$lib/components/TagColorPicker.svelte";
  import TagRow from "$lib/components/TagRow.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { resolveFixedPopoverPosition } from "$lib/utils/dropdown";
  import type { TagsChangedPayload } from "$lib/types/clipboard";
  import {
    deleteTag,
    getAutoTagRules,
    listAllTags,
    renameTag,
    setAutoTagRules,
    setTagColor,
    type AutoTagRule,
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
  let rules = $state<AutoTagRule[]>([]);
  let rulesSaving = $state(false);
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
    void loadRules();
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

  async function loadRules() {
    rules = (await getAutoTagRules()) ?? [];
  }

  function addRule() {
    rules = [...rules, { pattern: "", tag: "" }];
  }

  function removeRule(index: number) {
    rules = rules.filter((_, i) => i !== index);
  }

  async function saveRules() {
    if (rulesSaving) return;
    rulesSaving = true;
    try {
      const saved = await setAutoTagRules(
        rules.map((rule) => ({ pattern: rule.pattern.trim(), tag: rule.tag.trim() })),
      );
      rules = saved ?? rules;
      notify(_t("tags.autoTagSaved"));
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      rulesSaving = false;
    }
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
        <SearchField
          value={tagSearch}
          oninput={ontagSearchChange}
          placeholder={_t("tags.searchPlaceholder")}
          ariaLabel={_t("tags.searchPlaceholder")}
          width={180}
        />
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
    <section class="setting-card empty-state">
      <p class="auto-save-note">{_t("tags.empty")}</p>
    </section>
  {:else if filteredTags.length === 0}
    <section class="setting-card empty-state">
      <p class="auto-save-note">{_t("tags.noResults")}</p>
    </section>
  {:else}
    {#each filteredTags as tag (tag.name)}
      <TagRow
        {tag}
        confirmDelete={confirmDelete[tag.name]}
        colorPopoverOpen={colorPopover === tag.name}
        ontoggleColorPopover={toggleColorPopover}
        oncommitRename={commitRename}
        oncommitDelete={commitDelete}
        onreset={load}
      />
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

  <section class="setting-card autotag-card">
    <div class="setting-heading">
      <div>
        <strong>{_t("tags.autoTagTitle")}</strong>
        <p>{_t("tags.autoTagDescription")}</p>
      </div>
    </div>
    {#each rules as rule, index (index)}
      <div class="autotag-row">
        <input
          class="autotag-pattern"
          type="text"
          bind:value={rule.pattern}
          placeholder={_t("tags.autoTagPatternPlaceholder")}
          aria-label={_t("tags.autoTagPatternPlaceholder")}
          spellcheck={false}
        />
        <input
          class="autotag-tag"
          type="text"
          bind:value={rule.tag}
          placeholder={_t("tags.autoTagTagPlaceholder")}
          aria-label={_t("tags.autoTagTagPlaceholder")}
          spellcheck={false}
        />
        <button
          type="button"
          class="autotag-remove"
          aria-label={_t("tags.autoTagDelete")}
          title={_t("tags.autoTagDelete")}
          onclick={() => removeRule(index)}>×</button
        >
      </div>
    {/each}
    <div class="autotag-actions">
      <button type="button" class="autotag-add" onclick={addRule}>
        {_t("tags.autoTagAdd")}
      </button>
      <button
        type="button"
        class="autotag-save"
        disabled={rulesSaving || !isTauriRuntime()}
        onclick={() => void saveRules()}
      >
        {_t("tags.autoTagSave")}
      </button>
    </div>
  </section>
</div>

<style>
  .header-actions {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .autotag-card {
    margin-top: 12px;
  }

  .autotag-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }

  .autotag-pattern {
    flex: 3;
    min-width: 0;
  }

  .autotag-tag {
    flex: 2;
    min-width: 0;
  }

  .autotag-pattern,
  .autotag-tag {
    padding: 5px 8px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    background: var(--input-bg, var(--surface-bg));
    font-size: 12px;
  }

  .autotag-pattern:focus,
  .autotag-tag:focus {
    outline: none;
    border-color: var(--text-faint);
  }

  .autotag-remove {
    flex-shrink: 0;
    padding: 2px 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
    font-size: 14px;
    line-height: 1.4;
  }

  .autotag-remove:hover {
    color: var(--danger-color);
    border-color: var(--border-color);
  }

  .autotag-actions {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }

  .autotag-add,
  .autotag-save {
    padding: 5px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    background: var(--card-bg);
    cursor: pointer;
    font-size: 11.5px;
    font-weight: 500;
  }

  .autotag-add:hover,
  .autotag-save:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .autotag-save:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .tag-color-popover {
    position: fixed;
    padding: 10px;
  }
</style>
