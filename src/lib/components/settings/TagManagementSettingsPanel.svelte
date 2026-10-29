<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import SearchField from "$lib/components/common/SearchField.svelte";
  import TagColorPicker from "$lib/components/card/TagColorPicker.svelte";
  import TagRow from "$lib/components/card/TagRow.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { resolveFixedPopoverPosition } from "$lib/utils/settings/dropdown";
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
  let disposed = false;

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
  let loadFailed = $state(false);
  let feedback = $state<{ message: string; kind: "success" | "error" } | null>(null);
  let confirmDelete = $state<Record<string, boolean>>({});
  let feedbackTimer: ReturnType<typeof setTimeout> | undefined;

  // Clear a pending feedback reset when the panel unmounts so the timer
  // cannot fire against a destroyed component.
  $effect(() => {
    return () => clearTimeout(feedbackTimer);
  });

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
    if (disposed) return;
    loading = true;
    loadFailed = false;
    try {
      const result = await listAllTags();
      if (disposed) return;
      if (result === null) {
        loadFailed = true;
        return;
      }
      tags = [...result].sort((a, b) => a.name.localeCompare(b.name));
    } catch {
      if (!disposed) loadFailed = true;
    } finally {
      if (!disposed) loading = false;
    }
  }

  function notify(message: string, kind: "success" | "error" = "success") {
    if (disposed) return;
    clearTimeout(feedbackTimer);
    feedback = { message, kind };
    feedbackTimer = setTimeout(() => (feedback = null), 3000);
  }

  async function saveColor(tag: TagInfo, color: string) {
    colorPopover = null;
    try {
      const ok = await setTagColor(tag.name, color);
      if (ok !== true) {
        notify(_t("toast.saveFailed"), "error");
        return;
      }
      const index = tags.findIndex((t) => t.name === tag.name);
      if (index >= 0) tags[index] = { ...tags[index], color };
      notify(color ? _t("tags.colorSaved") : _t("tags.saved"));
      emitTagsChanged({});
    } catch {
      notify(_t("toast.saveFailed"), "error");
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
    if (e.key === "Escape") {
      colorPopover = null;
      e.preventDefault();
      e.stopImmediatePropagation();
    }
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
    try {
      const updated = await renameTag(tag.name, name);
      if (updated === null) {
        notify(_t("toast.saveFailed"), "error");
        return;
      }
      notify(_t("tags.renamed"));
      emitTagsChanged({ renamed: { old: tag.name, new: name } });
      void load();
    } catch {
      notify(_t("toast.saveFailed"), "error");
    }
  }

  async function commitDelete(tag: TagInfo) {
    if (!confirmDelete[tag.name]) {
      confirmDelete = { ...confirmDelete, [tag.name]: true };
      return;
    }
    try {
      const updated = await deleteTag(tag.name);
      if (updated === null) {
        notify(_t("toast.saveFailed"), "error");
        return;
      }
      notify(_t("tags.deleted"));
      emitTagsChanged({ deleted: tag.name });
      void load();
    } catch {
      notify(_t("toast.saveFailed"), "error");
    }
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
  {:else if loadFailed}
    <div class="settings-state" role="status">
      <p>{_t("tags.loadFailed")}</p>
      <button class="settings-action-btn" onclick={load}>{_t("storage.retryLoad")}</button>
    </div>
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
</div>

<style>
  .header-actions {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .tag-color-popover {
    position: fixed;
    padding: 10px;
  }
</style>
