<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import SelectEntry from "$lib/components/settings-entries/SelectEntry.svelte";
  import TextEntry from "$lib/components/settings-entries/TextEntry.svelte";
  import ToggleEntry from "$lib/components/settings-entries/ToggleEntry.svelte";
  import NumberEntry from "$lib/components/settings-entries/NumberEntry.svelte";
  import SizeEntry from "$lib/components/settings-entries/SizeEntry.svelte";
  import HeadingEntry from "$lib/components/settings-entries/HeadingEntry.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { fromDisplaySize, toDisplaySize } from "$lib/utils/unit-convert";
  import {
    getSyncConfig,
    runSync,
    setSyncConfig,
    testSyncConnection,
    type SyncConfig,
  } from "$lib/services/storage";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    /** Renders the S3-connection settings instead of the provider + limits. */
    s3?: boolean;
    onfeedback: (message: string, success: boolean) => void;
  }

  let { s3 = false, onfeedback }: Props = $props();

  let syncProvider = $state<"off" | "s3">("off");
  let syncEndpoint = $state("");
  let syncRemotePath = $state("");
  let syncTesting = $state(false);
  let syncTestResult = $state<{ success: boolean; message: string } | null>(null);
  let syncing = $state(false);
  let syncLastMs = $state<number | null>(null);
  let syncStatus = $state<string | null>(null);
  let syncPendingEntries = $state(0);
  let syncAutoSync = $state(false);
  let syncAutoInterval = $state(300);
  let syncSegmentMaxEntries = $state(512);
  let syncMaxImageBytes = $state(5242880);
  let syncMaxImageUnit = $state<"byte" | "KB" | "MB" | "GB">("MB");
  let syncMaxImageDisplay = $state(5);
  let syncMaxFileBytes = $state(10485760);
  let syncMaxFileUnit = $state<"byte" | "KB" | "MB" | "GB">("MB");
  let syncMaxFileDisplay = $state(10);
  let syncS3Region = $state("");
  let syncS3Bucket = $state("");
  let syncS3AccessKey = $state("");
  let syncS3SecretKey = $state("");
  let syncEncryptPassword = $state("");
  let syncHasS3SecretKey = $state(false);
  let syncHasEncryptionPassword = $state(false);

  $effect(() => {
    void loadSyncConfig();
  });

  function updateSyncMaxImageFromDisplay() {
    syncMaxImageBytes = fromDisplaySize(syncMaxImageDisplay, syncMaxImageUnit);
  }

  function updateSyncMaxFileFromDisplay() {
    syncMaxFileBytes = fromDisplaySize(syncMaxFileDisplay, syncMaxFileUnit);
  }

  async function loadSyncConfig() {
    if (!isTauriRuntime()) return;
    try {
      const cfg: SyncConfig = await getSyncConfig();
      syncProvider = cfg.provider;
      syncEndpoint = cfg.endpoint ?? "";
      syncRemotePath = cfg.remotePath ?? "";
      syncLastMs = cfg.lastSyncMs ?? null;
      syncStatus = cfg.lastSyncStatus ?? null;
      syncPendingEntries = cfg.pendingEntries ?? 0;
      syncAutoSync = cfg.autoSync ?? false;
      syncAutoInterval = cfg.autoSyncIntervalSecs ?? 300;
      syncSegmentMaxEntries = cfg.segmentMaxEntries ?? 512;
      syncMaxImageBytes = cfg.maxSyncImageBytes ?? 5242880;
      syncMaxImageDisplay = toDisplaySize(syncMaxImageBytes, syncMaxImageUnit);
      syncMaxFileBytes = cfg.maxSyncFileBytes ?? 10485760;
      syncMaxFileDisplay = toDisplaySize(syncMaxFileBytes, syncMaxFileUnit);
      syncS3Region = cfg.s3Region ?? "";
      syncS3Bucket = cfg.s3Bucket ?? "";
      syncS3AccessKey = cfg.s3AccessKey ?? "";
      syncHasS3SecretKey = cfg.hasS3SecretKey;
      syncHasEncryptionPassword = cfg.hasSyncPassword;
      syncS3SecretKey = "";
      syncEncryptPassword = "";
    } catch (e) {
      console.error("Failed to load sync config", e);
    }
  }

  async function persistSyncConfig() {
    if (!isTauriRuntime()) return;
    await setSyncConfig({
      provider: syncProvider,
      endpoint: syncEndpoint || null,
      remotePath: syncRemotePath || null,
      autoSync: syncAutoSync,
      autoSyncIntervalSecs: syncAutoInterval,
      segmentMaxEntries: syncSegmentMaxEntries,
      maxSyncImageBytes: syncMaxImageBytes,
      maxSyncFileBytes: syncMaxFileBytes,
      s3Region: syncS3Region || null,
      s3Bucket: syncS3Bucket || null,
      s3AccessKey: syncS3AccessKey || null,
      s3SecretKey: syncS3SecretKey || null,
      syncPassword: syncEncryptPassword || null,
    });
    if (syncS3SecretKey) syncHasS3SecretKey = true;
    if (syncEncryptPassword) syncHasEncryptionPassword = true;
    syncS3SecretKey = "";
    syncEncryptPassword = "";
  }

  async function saveSyncConfig() {
    try {
      await persistSyncConfig();
    } catch (e) {
      console.error("Failed to save sync config", e);
    }
  }

  async function handleTestConnection() {
    if (!isTauriRuntime() || syncTesting) return;
    syncTesting = true;
    syncTestResult = null;
    try {
      await persistSyncConfig();
      const result = await testSyncConnection();
      syncTestResult = { success: result.success, message: result.message };
    } catch (e) {
      syncTestResult = { success: false, message: String(e) };
    } finally {
      syncTesting = false;
    }
  }

  async function handleSyncNow() {
    if (!isTauriRuntime() || syncing) return;
    syncing = true;
    try {
      await persistSyncConfig();
      const result = await runSync();
      if (result.failedPeers > 0) {
        onfeedback(
          _t("storage.syncRunPartial", {
            failed: String(result.failedPeers),
            uploaded: String(result.uploadedEntries),
            downloaded: String(result.downloadedEntries),
            applied: String(result.appliedEntries),
          }),
          false,
        );
      } else if (
        result.uploadedEntries === 0 &&
        result.downloadedEntries === 0 &&
        result.deletedRemoteObjects === 0
      ) {
        onfeedback(_t("storage.syncNoChanges"), true);
      } else {
        onfeedback(
          _t("storage.syncRunSummary", {
            uploaded: String(result.uploadedEntries),
            downloaded: String(result.downloadedEntries),
            applied: String(result.appliedEntries),
            bytesUp: (result.bytesUploaded / 1024).toFixed(1),
            bytesDown: (result.bytesDownloaded / 1024).toFixed(1),
          }),
          true,
        );
      }
      syncLastMs = Date.now();
      syncStatus = result.failedPeers === 0 ? "success" : "partial";
      syncPendingEntries = 0;
    } catch (e) {
      onfeedback(_t("storage.syncRunFailed") + `: ${String(e)}`, false);
      syncStatus = "failed";
    } finally {
      syncing = false;
    }
  }
</script>

<div class="settings-scroll">
  <SelectEntry
    config={{
      type: "select",
      variant: "row",
      icon: "cloud",
      label: _t("storage.syncProvider"),
      ariaLabel: _t("storage.syncProvider"),
      options: [
        { value: "off", label: _t("storage.syncProviderOff") },
        { value: "s3", label: _t("storage.syncProviderS3") },
      ],
      get: () => syncProvider,
      set: (v) => {
        syncProvider = v as "off" | "s3";
        void saveSyncConfig();
      },
    }}
  />

  {#if !s3}
    <HeadingEntry
      config={{
        type: "heading",
        icon: "sliders",
        label: _t("storage.syncAdvancedTab"),
        desc: _t("storage.syncAdvancedDesc"),
      }}
    />
    <NumberEntry
      config={{
        type: "number",
        variant: "row",
        icon: "file",
        label: _t("storage.syncSegmentMaxEntries"),
        min: 16,
        max: 10000,
        suffix: _t("storage.syncEntriesUnit"),
        get: () => syncSegmentMaxEntries,
        set: (v) => (syncSegmentMaxEntries = v),
        onblur: saveSyncConfig,
      }}
    />

    <SizeEntry
      config={{
        type: "size",
        icon: "image",
        label: _t("storage.syncMaxImageBytes"),
        min: 0,
        get: () => syncMaxImageDisplay,
        set: (v) => (syncMaxImageDisplay = v),
        getUnit: () => syncMaxImageUnit,
        setUnit: (u) => {
          syncMaxImageUnit = u;
          updateSyncMaxImageFromDisplay();
        },
        onchange: saveSyncConfig,
      }}
    />

    <SizeEntry
      config={{
        type: "size",
        icon: "file",
        label: _t("storage.syncMaxFileBytes"),
        min: 0,
        get: () => syncMaxFileDisplay,
        set: (v) => (syncMaxFileDisplay = v),
        getUnit: () => syncMaxFileUnit,
        setUnit: (u) => {
          syncMaxFileUnit = u;
          updateSyncMaxFileFromDisplay();
        },
        onchange: saveSyncConfig,
      }}
    />
  {:else}
    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="cloud" size={17} /></span>
        <div><strong>{_t("storage.syncS3Title")}</strong></div>
      </div>
      <TextEntry
        config={{
          type: "text",
          variant: "row",
          icon: "globe",
          label: _t("storage.syncEndpoint"),
          inputType: "url",
          placeholder: "http://127.0.0.1:9000",
          get: () => syncEndpoint,
          set: (v) => (syncEndpoint = v),
          onblur: saveSyncConfig,
        }}
      />
      <TextEntry
        config={{
          type: "text",
          variant: "row",
          icon: "link",
          label: _t("storage.syncRemotePath"),
          placeholder: "clipboard-sync",
          get: () => syncRemotePath,
          set: (v) => (syncRemotePath = v),
          onblur: saveSyncConfig,
        }}
      />
      <TextEntry
        config={{
          type: "text",
          variant: "row",
          icon: "cloud",
          label: _t("storage.syncS3Region"),
          placeholder: "us-east-1",
          get: () => syncS3Region,
          set: (v) => (syncS3Region = v),
          onblur: saveSyncConfig,
        }}
      />
      <TextEntry
        config={{
          type: "text",
          variant: "row",
          icon: "layers",
          label: _t("storage.syncS3Bucket"),
          placeholder: "clipboard",
          get: () => syncS3Bucket,
          set: (v) => (syncS3Bucket = v),
          onblur: saveSyncConfig,
        }}
      />
      <TextEntry
        config={{
          type: "text",
          variant: "row",
          icon: "type",
          label: _t("storage.syncS3AccessKey"),
          get: () => syncS3AccessKey,
          set: (v) => (syncS3AccessKey = v),
          onblur: saveSyncConfig,
        }}
      />
      <TextEntry
        config={{
          type: "text",
          variant: "row",
          icon: "lock",
          label: _t("storage.syncS3SecretKey"),
          inputType: "password",
          placeholder: syncHasS3SecretKey ? _t("storage.syncSecretStored") : "",
          get: () => syncS3SecretKey,
          set: (v) => (syncS3SecretKey = v),
          onblur: saveSyncConfig,
        }}
      />
      <div class="setting-actions-row">
        <button
          type="button"
          class="settings-action-btn"
          disabled={syncTesting || syncing}
          onclick={handleTestConnection}
        >
          {syncTesting ? _t("storage.syncTesting") : _t("storage.syncTest")}
        </button>
        {#if syncTestResult}
          <span class="sync-last-info">{syncTestResult.message}</span>
        {/if}
      </div>
    </section>

    <TextEntry
      config={{
        type: "text",
        variant: "row",
        icon: "lock",
        label: _t("storage.syncEncryption"),
        inputType: "password",
        placeholder: syncHasEncryptionPassword
          ? _t("storage.syncEncryptionStored")
          : _t("storage.syncEncryptionPlaceholder"),
        get: () => syncEncryptPassword,
        set: (v) => (syncEncryptPassword = v),
        onblur: saveSyncConfig,
      }}
    />

    <ToggleEntry
      config={{
        type: "toggle",
        variant: "row",
        icon: "settings",
        label: _t("storage.syncAutoSync"),
        ariaLabel: _t("storage.syncAutoSyncEnable"),
        get: () => syncAutoSync,
        set: (v) => (syncAutoSync = v),
        onchange: () => void saveSyncConfig(),
      }}
    />

    {#if syncAutoSync}
      <NumberEntry
        config={{
          type: "number",
          variant: "row",
          icon: "clock",
          label: _t("storage.syncAutoInterval"),
          min: 10,
          max: 86400,
          suffix: _t("storage.syncSecondsUnit"),
          get: () => syncAutoInterval,
          set: (v) => (syncAutoInterval = v),
          onblur: saveSyncConfig,
        }}
      />
    {/if}

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="upload" size={17} /></span>
        <div class="sync-now-info">
          <strong>{_t("storage.syncNow")}</strong>
          <p class="sync-now-desc">
            {_t("storage.syncPendingCount", {
              count: syncPendingEntries,
            })}{#if syncLastMs}
              | {_t("storage.syncLastTime", {
                time: new Date(syncLastMs).toLocaleString(),
              })}{/if}{#if syncStatus}
              | {syncStatus === "success"
                ? _t("storage.syncStatusSuccess")
                : syncStatus === "partial"
                  ? _t("storage.syncStatusPartial")
                  : _t("storage.syncStatusFailed")}{/if}
          </p>
        </div>
        <button
          type="button"
          class="settings-action-btn"
          disabled={syncing || syncTesting}
          onclick={handleSyncNow}
        >
          {syncing ? _t("storage.syncing") : _t("storage.syncNow")}
        </button>
      </div>
    </section>
  {/if}
</div>
