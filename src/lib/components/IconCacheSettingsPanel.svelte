<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import Checkbox from "$lib/components/Checkbox.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    deleteIconFiles,
    listIconCache,
    replaceIconFile,
    type IconCacheEntry,
  } from "$lib/services/storage";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { formatBytes } from "$lib/utils/format";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    iconsDir: string;
    onfeedback: (message: string, success: boolean) => void;
    onclose: () => void;
  }

  let { iconsDir, onfeedback, onclose }: Props = $props();

  let iconFiles = $state<IconCacheEntry[]>([]);
  let loadingIcons = $state(false);
  let selectedIconFiles = $state<Set<string>>(new Set());
  let deletingIcons = $state(false);
  let replacingIcon = $state(false);
  let replaceTarget = $state<IconCacheEntry | null>(null);
  let selectedExistingIcon = $state<string | null>(null);
  let iconReplaceOptions = $derived([
    ...new Map(
      iconFiles.filter((f) => f.iconName && f.contentHash).map((e) => [e.contentHash!, e] as const),
    ).values(),
  ]);

  onMount(() => {
    void loadIconList();
  });

  async function loadIconList() {
    loadingIcons = true;
    try {
      iconFiles = await listIconCache();
      selectedIconFiles = new Set();
    } catch (error) {
      console.error("Unable to list icon cache", error);
    } finally {
      loadingIcons = false;
    }
  }

  function toggleIconFile(name: string) {
    const next = new Set(selectedIconFiles);
    if (next.has(name)) {
      next.delete(name);
    } else {
      next.add(name);
    }
    selectedIconFiles = next;
  }

  async function deleteSelectedIcons() {
    if (selectedIconFiles.size === 0) return;
    deletingIcons = true;
    try {
      const deleted = await deleteIconFiles([...selectedIconFiles]);
      onfeedback(_t("storage.iconsDeleted", { count: deleted }), true);
      await loadIconList();
    } catch (error) {
      console.error("Unable to delete icon files", error);
      onfeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      deletingIcons = false;
    }
  }

  async function applyIconReplacement(name: string, sourcePath: string) {
    if (!isTauriRuntime() || replacingIcon) return;
    replacingIcon = true;
    onfeedback("", false);
    try {
      await replaceIconFile(name, sourcePath);
      onfeedback(_t("storage.iconReplaced", { name }), true);
      await loadIconList();
      closeReplaceDialog();
    } catch (error) {
      console.error("Unable to replace icon", error);
      onfeedback(
        _t("storage.iconReplaceFailed", {
          error: error instanceof Error ? error.message : String(error),
        }),
        false,
      );
    } finally {
      replacingIcon = false;
    }
  }

  async function chooseReplaceFile() {
    const name = replaceTarget?.targetIconName;
    if (!name || replacingIcon) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const filePath = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "ico", "webp", "svg"] }],
    });
    if (!filePath) return;
    await applyIconReplacement(name, filePath);
  }

  async function confirmExistingIcon() {
    const name = replaceTarget?.targetIconName;
    if (!name || !selectedExistingIcon || replacingIcon) return;
    const sourcePath = `${iconsDir}/${selectedExistingIcon}`.replace(/\\/g, "/");
    await applyIconReplacement(name, sourcePath);
  }

  function openReplaceDialog(file: IconCacheEntry) {
    replaceTarget = file;
    selectedExistingIcon = null;
  }

  function closeReplaceDialog() {
    replaceTarget = null;
    selectedExistingIcon = null;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    if (replaceTarget) {
      event.preventDefault();
      event.stopPropagation();
      closeReplaceDialog();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="settings-scroll">
  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="image" size={17} /></span>
      <div>
        <strong>{_t("storage.iconCacheTitle")}</strong>
        <p>{_t("storage.iconCacheDesc")}</p>
      </div>
    </div>
    {#if loadingIcons}
      <p class="settings-state">{_t("storage.loadingIcons")}</p>
    {:else if iconFiles.length === 0}
      <p class="settings-state">{_t("storage.noIconFiles")}</p>
    {:else}
      {@const selectableIcons = iconFiles.filter((f) => f.iconName != null)}
      <div class="icon-table-header">
        <span class="icon-col-check"></span>
        <span class="icon-col-app">{_t("storage.iconColApp")}</span>
        <span class="icon-col-icon">{_t("storage.iconColIcon")}</span>
        <span class="icon-col-size">{_t("storage.iconColSize")}</span>
        <span class="icon-col-action">{_t("storage.iconColAction")}</span>
      </div>
      <ul class="icon-file-list">
        {#each iconFiles as file}
          <li class="icon-file-item">
            <span class="icon-col-check">
              {#if file.iconName}
                <label class="row-check">
                  <Checkbox
                    checked={selectedIconFiles.has(file.iconName)}
                    onchange={() => toggleIconFile(file.iconName!)}
                    size={14}
                  />
                </label>
              {/if}
            </span>
            <span class="icon-col-app">
              <span class="icon-app-name">{file.displayName}</span>
              {#if file.appName == null}<span class="icon-orphan-mark">*</span>{/if}
            </span>
            <span class="icon-col-icon">
              {#if file.iconName}
                <img
                  class="icon-preview"
                  src={convertFileSrc(`${iconsDir}/${file.iconName}`.replace(/\\/g, "/"))}
                  alt=""
                  onerror={(e) => {
                    (e.target as HTMLImageElement).style.display = "none";
                  }}
                />
              {:else}
                <span class="icon-letter">{file.firstChar}</span>
              {/if}
            </span>
            <span class="icon-col-size">
              {file.sizeBytes === 0 ? "0" : formatBytes(file.sizeBytes)}
            </span>
            <span class="icon-col-action">
              <button
                type="button"
                class="icon-replace-btn"
                disabled={replacingIcon}
                onclick={() => openReplaceDialog(file)}
              >
                {_t("storage.replaceIcon")}
              </button>
            </span>
          </li>
        {/each}
      </ul>
      <div class="icon-actions">
        <label class="icon-select-all">
          <Checkbox
            checked={selectableIcons.length > 0 &&
              selectedIconFiles.size === selectableIcons.length}
            disabled={selectableIcons.length === 0}
            onchange={(checked) => {
              selectedIconFiles = checked
                ? new Set(selectableIcons.map((f) => f.iconName as string))
                : new Set();
            }}
            size={14}
          />
          <span>{_t("storage.selectAll")}</span>
        </label>
        <span class="icon-file-count">
          {selectedIconFiles.size} / {iconFiles.length}
          {_t("storage.selected")}
        </span>
        <span class="icon-actions-spacer"></span>
        <button
          type="button"
          disabled={selectedIconFiles.size === 0 || deletingIcons}
          onclick={deleteSelectedIcons}
        >
          {deletingIcons
            ? _t("storage.deletingIcons")
            : _t("storage.deleteSelectedIcons", { count: selectedIconFiles.size })}
        </button>
      </div>
    {/if}
  </section>
</div>

{#if replaceTarget}
  <div class="icon-replace-modal" role="presentation" onpointerdown={closeReplaceDialog}>
    <div
      class="icon-replace-dialog"
      role="dialog"
      aria-modal="true"
      aria-label={_t("storage.iconReplaceTitle")}
      tabindex="-1"
      onpointerdown={(e) => e.stopPropagation()}
    >
      <div class="icon-replace-header">
        <strong>{_t("storage.iconReplaceFor", { name: replaceTarget.displayName })}</strong>
        <button
          type="button"
          class="icon-replace-close"
          aria-label={_t("storage.iconReplaceClose")}
          onclick={closeReplaceDialog}
        >
          <AppIcon name="x" size={16} />
        </button>
      </div>
      <p class="icon-replace-hint">{_t("storage.iconReplaceHint")}</p>
      <div class="icon-replace-grid">
        {#each iconReplaceOptions as entry (entry.contentHash)}
          <button
            type="button"
            class="icon-replace-option"
            class:selected={selectedExistingIcon === entry.iconName}
            onclick={() => (selectedExistingIcon = entry.iconName)}
          >
            <span class="icon-replace-thumb">
              {#if entry.iconName}
                <img
                  src={convertFileSrc(`${iconsDir}/${entry.iconName}`.replace(/\\/g, "/"))}
                  alt=""
                  onerror={(e) => {
                    (e.target as HTMLImageElement).style.display = "none";
                  }}
                />
              {/if}
            </span>
            <span class="icon-replace-name" title={entry.iconName!}>
              {entry.iconName!.replace(/\.[^.]+$/, "")}
            </span>
          </button>
        {/each}
      </div>
      <div class="icon-replace-footer">
        <button
          type="button"
          class="icon-replace-file-btn"
          disabled={replacingIcon}
          onclick={chooseReplaceFile}
        >
          {_t("storage.chooseFile")}
        </button>
        <button
          type="button"
          class="icon-replace-confirm-btn"
          disabled={!selectedExistingIcon || replacingIcon}
          onclick={confirmExistingIcon}
        >
          {_t("storage.confirmReplace")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .icon-select-all {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    line-height: 32px;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .icon-file-count {
    font-size: 11px;
    line-height: 32px;
    margin-top: 6px;
    color: var(--text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .icon-file-list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 300px;
    overflow-y: auto;
  }

  .icon-table-header,
  .icon-file-item {
    display: grid;
    grid-template-columns: 16px 1fr 40px 70px minmax(90px, auto);
    column-gap: 10px;
    align-items: center;
  }

  .icon-table-header {
    padding: 6px 14px 6px 12px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--input-bg);
    font-size: 11px;
    color: var(--text-muted);
    text-transform: none;
  }

  .icon-table-header .icon-col-app {
    color: var(--text-muted);
  }

  .icon-file-item {
    border-bottom: 1px solid var(--border-subtle);
    padding: 6px 16px 6px 12px;
    font-size: 12px;
  }

  .icon-file-item:last-child {
    border-bottom: none;
  }

  .icon-file-item:hover {
    background: var(--hover-bg);
  }

  .icon-col-check {
    display: flex;
    align-items: center;
  }

  .row-check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    cursor: pointer;
    user-select: none;
  }

  .icon-col-app {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .icon-app-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .icon-orphan-mark {
    flex-shrink: 0;
    color: var(--danger-color, #e5484d);
    font-weight: 600;
  }

  .icon-preview {
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    border-radius: 4px;
    object-fit: contain;
  }

  .icon-letter {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    border-radius: 5px;
    background: color-mix(in srgb, var(--selection-color) 18%, transparent);
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 600;
  }

  .icon-col-icon {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .icon-col-size {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    text-align: right;
  }

  .icon-col-action {
    display: flex;
    justify-content: center;
  }

  .icon-replace-btn {
    height: 26px;
    box-sizing: border-box;
    padding: 0 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .icon-replace-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .icon-replace-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .icon-replace-modal {
    position: fixed;
    z-index: 60;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 16px;
    background: rgba(5, 5, 5, 0.6);
    backdrop-filter: blur(4px);
  }

  .icon-replace-dialog {
    width: min(520px, 100%);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border-color);
    border-radius: 9px;
    background: var(--surface-bg);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }

  .icon-replace-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .icon-replace-header strong {
    font-size: var(--settings-heading-size, 13px);
  }

  .icon-replace-close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
  }

  .icon-replace-close:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .icon-replace-hint {
    margin: 0;
    padding: 10px 16px 0;
    font-size: var(--settings-description-size, 11px);
    color: var(--text-muted);
  }

  .icon-replace-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(84px, 1fr));
    gap: 8px;
    padding: 12px 16px;
    overflow-y: auto;
  }

  .icon-replace-option {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 8px 6px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-secondary);
    background: var(--hover-bg);
    cursor: pointer;
  }

  .icon-replace-option:hover {
    color: var(--text-primary);
  }

  .icon-replace-option.selected {
    border-color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 14%, transparent);
  }

  .icon-replace-thumb {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
  }

  .icon-replace-thumb img {
    width: 28px;
    height: 28px;
    object-fit: contain;
  }

  .icon-replace-name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--settings-description-size, 11px);
  }

  .icon-replace-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 12px 16px;
    border-top: 1px solid var(--border-subtle);
  }

  .icon-replace-footer button {
    height: 32px;
    box-sizing: border-box;
    padding: 0 14px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    font: inherit;
    font-size: var(--settings-control-size, 11px);
    cursor: pointer;
  }

  .icon-replace-file-btn {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .icon-replace-confirm-btn {
    color: #fff;
    background: var(--selection-color);
    border-color: var(--selection-color);
  }

  .icon-replace-footer button:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .icon-replace-footer button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .icon-actions {
    padding: 8px 14px 8px 12px;
    border-top: 1px solid var(--border-subtle);
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .icon-actions-spacer {
    flex: 1;
  }

  .icon-actions button {
    height: 32px;
    box-sizing: border-box;
    padding: 5px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
    white-space: nowrap;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .icon-actions button:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .icon-actions button:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
