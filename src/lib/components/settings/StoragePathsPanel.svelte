<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    configureStorageDirectory,
    getStorageConfig,
    setResourceOwnership,
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
  let resourcePathsLoaded = $state(false);
  let resourcePathsLoadFailed = $state(false);
  let savingResourceStorage = $state(false);
  let pendingResourceStorage = $state<{
    imageStoragePath: string;
    fileStoragePath: string;
    restartRequired: boolean;
  } | null>(null);
  let resourceStorageRestartNeeded = $state(false);
  let ownershipEnabled = $state(false);
  let togglingOwnership = $state(false);
  let markerRestartNeeded = $state(false);

  onMount(() => {
    let disposed = false;
    void getStorageConfig()
      .then((config) => {
        if (disposed) return;
        // Persisted paths may already target the next launch; status describes
        // the currently active roots and must only supply the placeholders.
        imageStoragePath = config.imageStoragePath ?? "";
        fileStoragePath = config.fileStoragePath ?? "";
        resourcePathsLoaded = true;
      })
      .catch((error) => {
        if (disposed) return;
        resourcePathsLoadFailed = true;
        onfeedback(error instanceof Error ? error.message : String(error), false);
      });
    return () => {
      disposed = true;
    };
  });

  $effect(() => {
    dataDirectory = status?.dataDirectoryPath ?? "";
  });

  $effect(() => {
    ownershipEnabled = Boolean(status?.resourceOwned);
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
    } catch (error) {
      // Debug builds refuse the self-restart (it would orphan the dev
      // server); surface the guidance instead of failing silently.
      const blocked = String(error).includes("restart_blocked_in_dev");
      if (blocked) onfeedback(_t("app.restartBlockedInDev"), false);
      else console.error("Unable to restart app");
    }
  }

  async function saveResourceStoragePaths() {
    await saveResourcePaths(imageStoragePath.trim(), fileStoragePath.trim());
  }

  async function saveResourcePaths(imagePath: string, filePath: string) {
    if (!resourcePathsLoaded || savingResourceStorage) return;
    savingResourceStorage = true;
    try {
      const result = await setResourceStoragePaths(imagePath || null, filePath || null);
      imageStoragePath = imagePath;
      fileStoragePath = filePath;
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
    await saveResourcePaths("", "");
  }

  async function toggleOwnership(next: boolean) {
    if (!status?.resourceOwnershipRequired || togglingOwnership) return;
    if (next && !window.confirm(_t("storage.ownershipMarkerEnableConfirm"))) return;
    togglingOwnership = true;
    try {
      const result = await setResourceOwnership(next);
      ownershipEnabled = next;
      markerRestartNeeded = result.restartRequired;
      onfeedback(
        next
          ? result.restartRequired
            ? _t("storage.ownershipMarkerEnabledFeedback")
            : _t("storage.ownershipMarkerEnabledFeedbackLive")
          : result.restartRequired
            ? _t("storage.ownershipMarkerDisabledFeedback")
            : _t("storage.ownershipMarkerDisabledFeedbackLive"),
        true,
      );
    } catch (error) {
      console.error("Unable to change resource ownership markers", error);
      onfeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      togglingOwnership = false;
    }
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
      onclick={() => {
        const configPath = status?.configPath;
        if (!configPath) return;
        invoke("reveal_in_explorer", { path: configPath }).catch((error) =>
          onfeedback(String(error), false),
        );
      }}
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
    {#if resourcePathsLoaded}
      <div class="resource-path-grid">
        <label for="image-storage-path">
          <span>{_t("storage.imageStoragePath")}</span>
          <input
            id="image-storage-path"
            disabled={savingResourceStorage}
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
            disabled={savingResourceStorage}
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
    {:else}
      <p class="settings-state" role="status">
        {_t(resourcePathsLoadFailed ? "storage.storageUnavailable" : "storage.readingConfig")}
      </p>
    {/if}
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="lock" size={17} /></span>
      <div>
        <strong>{_t("storage.ownershipMarkerTitle")}</strong>
        <p>
          {!status?.resourceOwnershipRequired
            ? _t("storage.ownershipMarkerDefaultDesc")
            : ownershipEnabled
              ? _t("storage.ownershipMarkerEnabledDesc")
              : _t("storage.ownershipMarkerDisabledDesc")}
        </p>
      </div>
    </div>
    <div class="ownership-controls">
      {#if markerRestartNeeded}
        <button class="restart-btn danger" type="button" onclick={restartApp}>
          {_t("storage.restartNow")}
        </button>
      {/if}
      <button
        type="button"
        class="toggle-switch"
        class:active={ownershipEnabled}
        aria-checked={ownershipEnabled}
        aria-label={_t("storage.ownershipMarkerTitle")}
        role="switch"
        disabled={!status?.resourceOwnershipRequired || togglingOwnership}
        onclick={() => void toggleOwnership(!ownershipEnabled)}
      >
        <span class="toggle-knob"></span>
      </button>
    </div>
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
