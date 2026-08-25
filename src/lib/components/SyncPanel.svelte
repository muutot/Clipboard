<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import {
    getSyncConfig,
    runSync,
    setSyncConfig,
    testSyncConnection,
    type SyncConfig,
  } from "$lib/services/storage";
  import { fromDisplaySize, toDisplaySize } from "$lib/utils/unit-convert";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    /** Renders the segment/resource limit rows instead of the connection card. */
    advanced?: boolean;
    onfeedback: (message: string, success: boolean) => void;
  }

  let { advanced = false, onfeedback }: Props = $props();

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

  function updateSyncMaxImageFromDisplay() {
    syncMaxImageBytes = fromDisplaySize(syncMaxImageDisplay, syncMaxImageUnit);
  }

  function changeSyncMaxImageUnit(unit: "byte" | "KB" | "MB" | "GB") {
    syncMaxImageUnit = unit;
    syncMaxImageDisplay = toDisplaySize(syncMaxImageBytes, unit);
  }

  function updateSyncMaxFileFromDisplay() {
    syncMaxFileBytes = fromDisplaySize(syncMaxFileDisplay, syncMaxFileUnit);
  }

  function changeSyncMaxFileUnit(unit: "byte" | "KB" | "MB" | "GB") {
    syncMaxFileUnit = unit;
    syncMaxFileDisplay = toDisplaySize(syncMaxFileBytes, unit);
  }
</script>

{#if !advanced}
  <section class="setting-card setting-card-row">
    <span class="setting-icon"><AppIcon name="cloud" size={17} /></span>
    <span class="setting-label">{_t("storage.syncProvider")}</span>
    <CustomSelect
      value={syncProvider}
      ariaLabel={_t("storage.syncProvider")}
      options={[
        { value: "off", label: _t("storage.syncProviderOff") },
        { value: "s3", label: _t("storage.syncProviderS3") },
      ]}
      onchange={(v) => {
        syncProvider = v as "off" | "s3";
        void saveSyncConfig();
      }}
    />
  </section>

  {#if syncProvider === "s3"}
    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="cloud" size={17} /></span>
        <div><strong>{_t("storage.syncS3Title")}</strong></div>
      </div>
      <div class="setting-row">
        <label for="sync-endpoint">{_t("storage.syncEndpoint")}</label>
        <input
          id="sync-endpoint"
          type="url"
          bind:value={syncEndpoint}
          placeholder="http://127.0.0.1:9000"
          onblur={saveSyncConfig}
        />
      </div>
      <div class="setting-row">
        <label for="sync-remote-path">{_t("storage.syncRemotePath")}</label>
        <input
          id="sync-remote-path"
          type="text"
          bind:value={syncRemotePath}
          placeholder="clipboard-sync"
          onblur={saveSyncConfig}
        />
      </div>
      <div class="setting-row">
        <label for="sync-s3-region">{_t("storage.syncS3Region")}</label>
        <input
          id="sync-s3-region"
          type="text"
          bind:value={syncS3Region}
          placeholder="us-east-1"
          onblur={saveSyncConfig}
        />
      </div>
      <div class="setting-row">
        <label for="sync-s3-bucket">{_t("storage.syncS3Bucket")}</label>
        <input
          id="sync-s3-bucket"
          type="text"
          bind:value={syncS3Bucket}
          placeholder="clipboard"
          onblur={saveSyncConfig}
        />
      </div>
      <div class="setting-row">
        <label for="sync-s3-access-key">{_t("storage.syncS3AccessKey")}</label>
        <input
          id="sync-s3-access-key"
          type="text"
          bind:value={syncS3AccessKey}
          onblur={saveSyncConfig}
        />
      </div>
      <div class="setting-row">
        <label for="sync-s3-secret-key">{_t("storage.syncS3SecretKey")}</label>
        <input
          id="sync-s3-secret-key"
          type="password"
          bind:value={syncS3SecretKey}
          placeholder={syncHasS3SecretKey ? _t("storage.syncSecretStored") : ""}
          onblur={saveSyncConfig}
        />
      </div>
      <div class="setting-row setting-actions-row">
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

    <section class="setting-card setting-card-row">
      <span class="setting-icon"><AppIcon name="lock" size={17} /></span>
      <span class="setting-label">{_t("storage.syncEncryption")}</span>
      <input
        type="password"
        bind:value={syncEncryptPassword}
        placeholder={syncHasEncryptionPassword
          ? _t("storage.syncEncryptionStored")
          : _t("storage.syncEncryptionPlaceholder")}
        onblur={saveSyncConfig}
        class="sync-encrypt-input"
      />
    </section>

    <section class="setting-card setting-card-row">
      <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
      <span class="setting-label">{_t("storage.syncAutoSync")}</span>
      <button
        type="button"
        class="toggle-switch"
        class:active={syncAutoSync}
        onclick={() => {
          syncAutoSync = !syncAutoSync;
          void saveSyncConfig();
        }}
        aria-checked={syncAutoSync}
        aria-label={_t("storage.syncAutoSyncEnable")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    {#if syncAutoSync}
      <section class="setting-card setting-card-row">
        <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
        <span class="setting-label">{_t("storage.syncAutoInterval")}</span>
        <input
          type="number"
          bind:value={syncAutoInterval}
          min="10"
          max="86400"
          onblur={saveSyncConfig}
        />
        <span class="number-suffix">{_t("storage.syncSecondsUnit")}</span>
      </section>
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
{:else}
  <section class="setting-card setting-card-row">
    <span class="setting-icon"><AppIcon name="file" size={17} /></span>
    <span class="setting-label">{_t("storage.syncSegmentMaxEntries")}</span>
    <input
      type="number"
      bind:value={syncSegmentMaxEntries}
      min="16"
      max="10000"
      onblur={saveSyncConfig}
    />
    <span class="number-suffix">{_t("storage.syncEntriesUnit")}</span>
  </section>

  <section class="setting-card setting-card-row">
    <span class="setting-icon"><AppIcon name="image" size={17} /></span>
    <span class="setting-label">{_t("storage.syncMaxImageBytes")}</span>
    <input
      type="number"
      bind:value={syncMaxImageDisplay}
      min="0"
      oninput={updateSyncMaxImageFromDisplay}
      onchange={saveSyncConfig}
    />
    <CustomSelect
      className="unit-select"
      value={syncMaxImageUnit}
      options={[
        { value: "byte", label: "B" },
        { value: "KB", label: "KB" },
        { value: "MB", label: "MB" },
        { value: "GB", label: "GB" },
      ]}
      onchange={(v) => changeSyncMaxImageUnit(v as "byte" | "KB" | "MB" | "GB")}
    />
  </section>

  <section class="setting-card setting-card-row">
    <span class="setting-icon"><AppIcon name="file" size={17} /></span>
    <span class="setting-label">{_t("storage.syncMaxFileBytes")}</span>
    <input
      type="number"
      bind:value={syncMaxFileDisplay}
      min="0"
      oninput={updateSyncMaxFileFromDisplay}
      onchange={saveSyncConfig}
    />
    <CustomSelect
      className="unit-select"
      value={syncMaxFileUnit}
      options={[
        { value: "byte", label: "B" },
        { value: "KB", label: "KB" },
        { value: "MB", label: "MB" },
        { value: "GB", label: "GB" },
      ]}
      onchange={(v) => changeSyncMaxFileUnit(v as "byte" | "KB" | "MB" | "GB")}
    />
  </section>
{/if}

<style>
  /* The shell's scoped label/input base rules do not reach into this lazy
     panel, so the S3 form re-declares them here (same values as the shell). */
  .setting-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
  }

  .setting-row label {
    flex: 0 0 auto;
    min-width: 110px;
    margin: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .setting-row input {
    flex: 1;
    min-width: 0;
    padding: 7px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    outline: none;
    color: var(--text-primary);
    background: var(--input-bg);
    font-size: var(--settings-control-size);
    transition: border-color 120ms ease;
  }

  .setting-row input:focus {
    border-color: var(--text-faint);
  }

  .setting-actions-row {
    margin-top: 10px;
  }

  .sync-encrypt-input {
    flex: 1;
    min-width: 0;
  }

  .sync-now-info {
    flex: 1;
  }

  .sync-now-desc {
    margin: 2px 0 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }

  .sync-last-info {
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }
</style>
