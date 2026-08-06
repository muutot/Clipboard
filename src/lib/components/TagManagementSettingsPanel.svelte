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
  import { emit } from "@tauri-apps/api/event";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  function emitTagsChanged(payload: TagsChangedPayload) {
    emit("tags-changed", payload).catch((err) => console.warn("tags-changed emit failed:", err));
  }

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

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

  onMount(() => {
    void load();
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
    const ok = await setTagColor(tag.name, next);
    if (ok) {
      const index = tags.findIndex((t) => t.name === tag.name);
      if (index >= 0) tags[index] = { ...tags[index], color: next };
      notify(next ? _t("tags.colorSaved") : _t("tags.saved"));
      emitTagsChanged({});
    }
  }

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
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  {#if loading}
    <p class="auto-save-note">{_t("status.searching")}</p>
  {:else if tags.length === 0}
    <section class="setting-card">
      <p class="auto-save-note">{_t("tags.empty")}</p>
    </section>
  {:else}
    {#each tags as tag (tag.name)}
      <section class="setting-card tag-row">
        <span
          class="tag-swatch"
          style={tag.color ? `--tag-accent: ${tag.color}` : undefined}
          aria-hidden="true"
        ></span>
        <div class="tag-fields">
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
          <span class="tag-count">{_t("tags.count", { count: tag.count })}</span>
        </div>
        <div class="tag-colors" aria-label={_t("tags.title")}>
          <button
            type="button"
            class="tag-swatch-option"
            class:active={!tag.color}
            title={_t("tags.colorNone")}
            aria-label={_t("tags.colorNone")}
            onclick={() => pickColor(tag, "")}
          ></button>
          {#each presets as color (color)}
            <button
              type="button"
              class="tag-swatch-option"
              class:active={tag.color === color}
              style={`--swatch: ${color}`}
              aria-label={color}
              title={color}
              onclick={() => pickColor(tag, color)}
            ></button>
          {/each}
        </div>
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
  {/if}

  {#if feedback}
    <p class="settings-feedback" class:success={feedback.kind === "success"} role="status">
      {feedback.message}
    </p>
  {/if}
</div>

<style>
  .tag-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .tag-swatch {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    border: 1px solid var(--border-color);
    border-radius: 50%;
    background: color-mix(in srgb, var(--tag-accent) 45%, var(--surface-bg));
  }

  .tag-fields {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .tag-name-input {
    width: 100%;
    padding: 4px 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-primary);
    background: transparent;
    font-size: 13px;
    font-weight: 600;
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
    font-size: 11px;
    color: var(--text-faint);
  }

  .tag-colors {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    align-items: center;
  }

  .tag-swatch-option {
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 50%;
    background: var(--surface-bg);
    cursor: pointer;
  }

  .tag-swatch-option[style*="--swatch"] {
    background: var(--swatch);
  }

  .tag-swatch-option.active {
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
