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
    dataDirectory = status?.dataDirectoryPath ?? "";
  });

  function relativePath(absolute: string): string {
    const bases = [status?.dataDirectoryPath, status?.storagePath, status?.projectPath];
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
    <span class="config-path">{relativePath(status?.configPath ?? "")}</span>
    <button
      type="button"
      class="open-btn"
      onclick={() => invoke("open_external_url", { url: status?.configPath ?? "" })}
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
          <span class:custom={status?.usesCustomDataDirectory} class="inline-badge">
            {status?.usesCustomDataDirectory ? _t("storage.custom") : _t("storage.default")}
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
          placeholder={status?.imagePath ?? ""}
        />
      </label>
      <label for="file-storage-path">
        <span>{_t("storage.fileStoragePath")}</span>
        <input
          id="file-storage-path"
          bind:value={fileStoragePath}
          autocomplete="off"
          spellcheck="false"
          placeholder={status?.filesPath ?? ""}
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
    {#if !status?.imageCleanupEnabled || !status?.fileCleanupEnabled}
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
