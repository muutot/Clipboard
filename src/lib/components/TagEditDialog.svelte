<script lang="ts">
  import { onMount, untrack } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type { TagsChangedPayload } from "$lib/types/clipboard";
  import { listAllTags, renameTag, setTagColor } from "$lib/services/clipboard";
  import { showToast } from "$lib/services/toast";
  import { emit } from "@tauri-apps/api/event";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    tag: string;
    color: string;
    onclose: () => void;
  }

  let { tag, color, onclose }: Props = $props();

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

  let dialog = $state<HTMLDialogElement | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);
  let currentName = $state(untrack(() => tag));
  let nameDraft = $state(untrack(() => tag));
  let currentColor = $state(untrack(() => color));

  onMount(() => {
    queueMicrotask(() => {
      dialog?.showModal();
      nameInput?.focus();
      nameInput?.select();
    });
  });

  function emitTagsChanged(payload: TagsChangedPayload) {
    emit("tags-changed", payload).catch((err) => console.warn("tags-changed emit failed:", err));
  }

  async function commitRename() {
    const name = nameDraft.trim();
    if (!name || name === currentName) {
      nameDraft = currentName;
      return;
    }
    const all = (await listAllTags()) ?? [];
    if (all.some((entry) => entry.name !== currentName && entry.name === name)) {
      showToast(_t("tags.renameConflict"), "error");
      return;
    }
    const ok = await renameTag(currentName, name);
    if (ok !== null) {
      showToast(_t("tags.renamed"), "success");
      emitTagsChanged({ renamed: { old: currentName, new: name } });
      currentName = name;
      nameDraft = name;
    }
  }

  async function saveColor(target: string) {
    const ok = await setTagColor(currentName, target);
    if (ok) {
      currentColor = target;
      showToast(target ? _t("tags.colorSaved") : _t("tags.saved"), "success");
      emitTagsChanged({});
    }
  }

  function pickColor(next: string) {
    void saveColor(currentColor === next ? "" : next);
  }

  function selectCustom(next: string) {
    void saveColor(next);
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) onclose();
  }
</script>

<dialog
  bind:this={dialog}
  class="tag-edit-dialog"
  aria-label={_t("tags.editTitle")}
  {onclose}
  onclick={handleBackdropClick}
>
  <div class="tag-edit-content">
    <div class="tag-edit-top">
      <span
        class="tag-edit-swatch"
        style={currentColor ? `--tag-accent: ${currentColor}` : undefined}
        aria-hidden="true"
      ></span>
      <input
        bind:value={nameDraft}
        bind:this={nameInput}
        class="tag-edit-name"
        aria-label={_t("tags.name")}
        placeholder={_t("tags.renamePlaceholder")}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void commitRename();
          }
        }}
      />
      <button
        type="button"
        class="tag-edit-close"
        aria-label={_t("actions.closeViewer")}
        onclick={onclose}
      >
        <AppIcon name="x" size={14} />
      </button>
    </div>

    <div class="tag-edit-colors" aria-label={_t("tags.color")}>
      {#each presets as presetColor (presetColor)}
        <button
          type="button"
          class="tag-swatch-option"
          class:active={currentColor === presetColor}
          style={`--swatch: ${presetColor}`}
          aria-label={presetColor}
          title={presetColor}
          onclick={() => pickColor(presetColor)}
        ></button>
      {/each}
      <label
        class="tag-swatch-option tag-swatch-custom"
        class:active={currentColor !== "" && !presets.includes(currentColor)}
        style={currentColor && !presets.includes(currentColor)
          ? `--swatch: ${currentColor}`
          : undefined}
        title={_t("tags.customColor")}
        aria-label={_t("tags.customColor")}
      >
        <input
          type="color"
          value={/^#[0-9a-fA-F]{6}$/.test(currentColor) ? currentColor : "#5c7cfa"}
          onchange={(e) => selectCustom(e.currentTarget.value)}
        />
        <AppIcon name="palette" size={12} />
      </label>
    </div>
  </div>
</dialog>

<style>
  .tag-edit-dialog {
    margin: auto;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 10px;
    color: var(--text-primary);
    background: var(--card-bg);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    width: min(300px, calc(100vw - 32px));
  }

  .tag-edit-dialog::backdrop {
    background: rgba(0, 0, 0, 0.52);
    backdrop-filter: blur(2px);
  }

  .tag-edit-content {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
  }

  .tag-edit-top {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .tag-edit-swatch {
    flex-shrink: 0;
    width: 14px;
    height: 14px;
    border: 1px solid var(--border-color);
    border-radius: 50%;
    background: color-mix(in srgb, var(--tag-accent) 45%, var(--surface-bg));
  }

  .tag-edit-name {
    flex: 1;
    min-width: 0;
    padding: 6px 8px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    background: var(--input-bg);
    font-size: 13px;
    font-weight: 600;
    outline: none;
  }

  .tag-edit-name:focus {
    border-color: var(--text-faint);
  }

  .tag-edit-close {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
  }

  .tag-edit-close:hover {
    color: var(--text-primary);
    border-color: var(--border-color);
  }

  .tag-edit-colors {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 8px;
    justify-items: center;
    align-items: center;
  }

  .tag-swatch-option {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
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

  .tag-swatch-custom {
    position: relative;
    overflow: hidden;
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

  .tag-swatch-custom input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    opacity: 0;
    cursor: pointer;
  }

  .tag-swatch-custom.active {
    color: var(--text-primary);
  }
</style>
