<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    configureStorageDirectory,
    setResourceStoragePaths,
    type StorageDirectoryUpdate,
    type StorageStatus,
  } from "$lib/services/storage";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    status: StorageStatus;
    onfeedback: (message: string, success: boolean) => void;
  }

  let { status, onfeedback }: Props = $props();

  let dataDirectory = $state("");
  let pending = $state<StorageDirectoryUpdate | null>(null);
  let saving = $state(false);
  let restartNeeded = $state(false);
  let imageStoragePath = $state("");
  let fileStoragePath = $state("");
  let savingResourceStorage = $state(false);
  let pendingResourceStorage = $state<{
    imageStoragePath: string;
    fileStoragePath: string;
    restartRequired: boolean;
  } | null>(null);
  let resourceStorageRestartNeeded = $state(false);

  $effect(() => {
    dataDirectory = status.dataDirectoryPath ?? "";
  });

  function relativePath(absolute: string): string {
    const bases = [status.dataDirectoryPath, status.storagePath, status.projectPath];
    for (const basePath of bases) {
      if (!basePath) continue;
      const base = basePath.replace(/\\/g, "/");
      const target = absolute.replace(/\\/g, "/");
      if (target === base) return ".";
      if (target.startsWith(base + "/")) return target.slice(base.length + 1);
    }
    return absolute;
  }

  async function saveCustomDirectory() {
    const requested = dataDirectory.trim();
    if (!requested) {
      onfeedback(_t("storage.enterAbsolutePath"), false);
      return;
    }

    await saveDirectory(requested);
  }

  async function restoreDefaultDirectory() {
    await saveDirectory(null);
  }

  async function saveDirectory(directory: string | null) {
    saving = true;
    try {
      pending = await configureStorageDirectory(directory);
      dataDirectory = pending.dataDirectoryPath;
      restartNeeded = pending.restartRequired;
      onfeedback(
        pending.restartRequired ? _t("storage.savedAndRestart") : _t("storage.alreadyUsingDir"),
        true,
      );
    } catch (error) {
      console.error("Unable to configure storage directory", error);
      onfeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      saving = false;
    }
  }

  async function restartApp() {
    try {
      await invoke("restart_app");
    } catch {
      console.error("Unable to restart app");
    }
  }

  async function saveResourceStoragePaths() {
    savingResourceStorage = true;
    try {
      const result = await setResourceStoragePaths(
        imageStoragePath.trim() || null,
        fileStoragePath.trim() || null,
      );
      pendingResourceStorage = result;
      resourceStorageRestartNeeded = result.restartRequired;
      onfeedback(
        result.restartRequired
          ? _t("storage.resourcePathsSavedAndRestart")
          : _t("storage.resourcePathsSaved"),
        true,
      );
    } catch (error) {
      console.error("Unable to save resource storage paths", error);
      onfeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      savingResourceStorage = false;
    }
  }

  async function restoreDefaultResourceStoragePaths() {
    imageStoragePath = "";
    fileStoragePath = "";
    await saveResourceStoragePaths();
  }
</script>

<div class="settings-scroll">
  <section class="setting-card setting-card-row">
    <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
    <span class="setting-label">{_t("storage.currentProfile")}</span>
    <span class="config-path">{relativePath(status.configPath)}</span>
    <button
      type="button"
      class="open-btn"
      onclick={() => invoke("open_external_url", { url: status.configPath })}
    >
      <AppIcon name="file" size={14} />
      {_t("storage.open")}
    </button>
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="file" size={17} /></span>
      <div>
        <strong>
          {_t("storage.dataDirectoryTitle")}
          <span class:custom={status.usesCustomDataDirectory} class="inline-badge">
            {status.usesCustomDataDirectory ? _t("storage.custom") : _t("storage.default")}
          </span>
        </strong>
        <p>{_t("storage.dataDirectoryDesc")}</p>
      </div>
    </div>
    <div class="dir-input-row">
      <input
        id="data-directory"
        bind:value={dataDirectory}
        autocomplete="off"
        spellcheck="false"
        placeholder={_t("storage.placeholderPath")}
      />
      <button type="button" disabled={saving} onclick={restoreDefaultDirectory}
        >{_t("storage.restoreDefault")}</button
      >
      <button type="button" disabled={saving} onclick={saveCustomDirectory}
        >{saving ? _t("storage.saving") : _t("storage.saveDirectory")}</button
      >
    </div>

    {#if pending}
      <div class="pending-path">
        <span>{_t("storage.nextLaunch")}</span>
        <code title={pending.storagePath}>{pending.storagePath}</code>
        {#if restartNeeded}
          <button class="restart-btn" type="button" onclick={restartApp}
            >{_t("storage.restartNow")}</button
          >
        {/if}
      </div>
    {/if}
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="file" size={17} /></span>
      <div>
        <strong>{_t("storage.resourcePathsTitle")}</strong>
        <p>{_t("storage.resourcePathsDesc")}</p>
      </div>
    </div>
    <div class="resource-path-grid">
      <label for="image-storage-path">
        <span>{_t("storage.imageStoragePath")}</span>
        <input
          id="image-storage-path"
          bind:value={imageStoragePath}
          autocomplete="off"
          spellcheck="false"
          placeholder={status.imagePath}
        />
      </label>
      <label for="file-storage-path">
        <span>{_t("storage.fileStoragePath")}</span>
        <input
          id="file-storage-path"
          bind:value={fileStoragePath}
          autocomplete="off"
          spellcheck="false"
          placeholder={status.filesPath}
        />
      </label>
    </div>
    <div class="dir-input-row resource-path-actions">
      <span>{_t("storage.resourcePathsRestartHint")}</span>
      <button
        type="button"
        disabled={savingResourceStorage}
        onclick={restoreDefaultResourceStoragePaths}>{_t("storage.restoreDefault")}</button
      >
      <button type="button" disabled={savingResourceStorage} onclick={saveResourceStoragePaths}
        >{savingResourceStorage ? _t("storage.saving") : _t("storage.saveDirectory")}</button
      >
    </div>
    {#if !status.imageCleanupEnabled || !status.fileCleanupEnabled}
      <div class="resource-path-warning">
        <AppIcon name="info" size={14} />
        <span>{_t("storage.resourcePathsCleanupDisabled")}</span>
      </div>
    {/if}
    {#if pendingResourceStorage}
      <div class="resource-path-summary">
        <code title={pendingResourceStorage.imageStoragePath}
          >{_t("storage.imageStoragePath")}: {pendingResourceStorage.imageStoragePath}</code
        >
        <code title={pendingResourceStorage.fileStoragePath}
          >{_t("storage.fileStoragePath")}: {pendingResourceStorage.fileStoragePath}</code
        >
        {#if resourceStorageRestartNeeded}
          <button class="restart-btn" type="button" onclick={restartApp}>
            {_t("storage.restartNow")}
          </button>
        {/if}
      </div>
    {/if}
  </section>

  <section class="setting-card directory-tree-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
      <div>
        <strong>{_t("storage.directoryTreeTitle")}</strong>
        <p>{_t("storage.directoryTreeDesc")}</p>
      </div>
    </div>
    <pre>{_t("storage.directoryTree")}</pre>
  </section>
</div>

<style>
  .config-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text-muted);
    font-size: var(--settings-note-size);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .open-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-muted);
    background: var(--card-bg);
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .open-btn:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .dir-input-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
  }

  .dir-input-row input {
    flex: 1;
    min-width: 0;
    padding: 7px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-primary);
    background: var(--input-bg);
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: var(--settings-control-size);
    outline: none;
    transition: border-color 120ms ease;
  }

  .dir-input-row input:focus {
    border-color: var(--text-faint);
  }

  .dir-input-row button {
    padding: 7px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .dir-input-row button:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .dir-input-row button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .inline-badge {
    display: inline-block;
    margin-left: 8px;
    padding: 2px 7px;
    border: 1px solid var(--border-color);
    border-radius: 999px;
    color: var(--text-muted);
    font-size: var(--settings-note-size);
    font-weight: 500;
    vertical-align: middle;
  }

  .inline-badge.custom {
    border-color: color-mix(in srgb, var(--selection-color) 36%, transparent);
    color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 12%, transparent);
  }

  label {
    display: block;
    margin: 12px 0 6px;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    outline: none;
    color: var(--text-primary);
    background: var(--input-bg);
    font:
      12px "Cascadia Code",
      "SFMono-Regular",
      Consolas,
      monospace;
    transition: border-color 120ms ease;
  }

  input:focus {
    border-color: var(--text-faint);
  }

  .pending-path {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding-top: 9px;
    border-top: 1px solid var(--border-subtle);
    color: var(--text-faint);
    font-size: var(--settings-description-size);
  }

  .pending-path code {
    display: block;
    overflow: hidden;
    color: var(--text-secondary);
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--settings-description-size);
  }

  .directory-tree-card pre {
    margin: 11px 0 0;
    padding: 10px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    color: var(--text-muted);
    background: var(--input-bg);
    font:
      11px/1.55 "Cascadia Code",
      "SFMono-Regular",
      Consolas,
      monospace;
  }

  .resource-path-grid {
    display: grid;
    gap: 9px;
    margin-top: 10px;
  }

  .resource-path-grid label {
    margin: 0;
  }

  .resource-path-grid label span {
    display: block;
    margin-bottom: 5px;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .resource-path-actions span {
    flex: 1;
    min-width: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .resource-path-warning {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    margin-top: 10px;
    padding: 9px 10px;
    border: 1px solid color-mix(in srgb, var(--warning-color) 35%, transparent);
    border-radius: var(--settings-card-radius);
    color: color-mix(in srgb, var(--warning-color) 75%, white);
    background: color-mix(in srgb, var(--warning-color) 12%, var(--surface-bg));
    font-size: var(--settings-description-size);
    line-height: 1.45;
  }

  .resource-path-summary {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 5px 10px;
    align-items: center;
    margin-top: 10px;
    padding-top: 9px;
    border-top: 1px solid var(--border-subtle);
  }

  .resource-path-summary code {
    min-width: 0;
    overflow: hidden;
    color: var(--text-secondary);
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    font-size: var(--settings-description-size);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .resource-path-summary .restart-btn {
    grid-column: 2;
    grid-row: 1 / span 2;
  }

  .restart-btn {
    margin-left: auto;
    padding: 5px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font-size: var(--settings-control-size);
    cursor: pointer;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .restart-btn:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
    border-color: var(--text-faint);
  }
</style>
