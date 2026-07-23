<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import KeyboardSettingsPanel from "$lib/components/KeyboardSettingsPanel.svelte";
  import IgnoredAppsSettingsPanel from "$lib/components/IgnoredAppsSettingsPanel.svelte";
  import {
    configureStorageDirectory,
    getStorageStatus,
    rebuildSearchIndex,
    type StorageDirectoryUpdate,
    type StorageStatus,
  } from "$lib/services/storage";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (
    path: string,
    params?: Record<string, string | number>,
  ) => resolvePath($messages, path, params);

  interface Props {
    open: boolean;
    onclose: () => void;
  }

  let { open, onclose }: Props = $props();
  let status = $state<StorageStatus | null>(null);
  let pending = $state<StorageDirectoryUpdate | null>(null);
  let dataDirectory = $state("");
  let loading = $state(false);
  let saving = $state(false);
  let rebuilding = $state(false);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let activeSection = $state<"capture" | "storage" | "keyboard">("storage");

  $effect(() => {
    if (open) {
      void loadStatus();
    }
  });

  async function loadStatus() {
    loading = true;
    pending = null;
    feedback = "";
    feedbackSuccess = false;

    try {
      status = await getStorageStatus();
      dataDirectory = status?.dataDirectoryPath ?? "";
      if (!status) {
        feedback = _t("storage.systemMessage");
      }
    } catch (error) {
      console.error("Unable to load storage settings", error);
      status = null;
      feedback = _t("storage.writeFailed");
    } finally {
      loading = false;
    }
  }

  async function saveCustomDirectory() {
    const requested = dataDirectory.trim();
    if (!requested) {
      feedback = _t("storage.enterAbsolutePath");
      return;
    }

    await saveDirectory(requested);
  }

  async function restoreDefaultDirectory() {
    await saveDirectory(null);
  }

  async function saveDirectory(directory: string | null) {
    saving = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      pending = await configureStorageDirectory(directory);
      dataDirectory = pending.dataDirectoryPath;
      feedback = pending.restartRequired
        ? _t("storage.savedAndRestart")
        : _t("storage.alreadyUsingDir");
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to configure storage directory", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }

  async function rebuildIndex() {
    rebuilding = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      const summary = await rebuildSearchIndex();
      status = await getStorageStatus();
      feedback = _t("storage.rebuildComplete", {
        events: summary.processedEvents,
        docs: summary.upsertedDocuments,
      });
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to rebuild search index", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      rebuilding = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="settings-backdrop">
    <div
      class="settings-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
      tabindex="-1"
    >
      <aside class="settings-sidebar">
        <div class="settings-brand">
          <span class="brand-icon"><AppIcon name="clipboard" size={18} /></span>
          <div>
            <strong>Clipboard</strong>
            <small>0.1.0</small>
          </div>
        </div>

        <nav aria-label="设置分类">
          <button
            class:active={activeSection === "capture"}
            type="button"
            onclick={() => (activeSection = "capture")}
          >
            <AppIcon name="filter" size={16} />
            <span>采集</span>
          </button>
          <button
            class:active={activeSection === "storage"}
            type="button"
            onclick={() => (activeSection = "storage")}
          >
            <AppIcon name="file" size={16} />
            <span>{_t("storage.storageTab")}</span>
          </button>
          <button
            class:active={activeSection === "keyboard"}
            type="button"
            onclick={() => (activeSection = "keyboard")}
          >
            <AppIcon name="keyboard" size={16} />
            <span>{_t("storage.keyboardTab")}</span>
          </button>
          <button type="button" disabled>
            <AppIcon name="settings" size={16} />
            <span>{_t("storage.generalTab")}</span>
          </button>
        </nav>

        <div class="sidebar-foot">
          <span>配置固定位置</span>
          <code>{activeSection === "keyboard" ? "conf/keyboard.json" : "conf/conf.json"}</code>
        </div>
      </aside>

      <div class="settings-content">
        {#if activeSection === "capture"}
          <IgnoredAppsSettingsPanel configPath={status?.configPath} {onclose} />
        {:else if activeSection === "keyboard"}
          <KeyboardSettingsPanel configPath={status?.keyboardConfigPath} {onclose} />
        {:else}
          <header>
            <div>
              <span class="eyebrow">{_t("storage.settings")}</span>
              <h2 id="settings-title">{_t("storage.dataStorage")}</h2>
              <p>{_t("storage.configPath")}</p>
            </div>
            <button
              class="close-button"
              type="button"
              aria-label={_t("actions.close")}
              onclick={onclose}>×</button
            >
          </header>

          {#if loading}
            <div class="settings-state">{_t("storage.readingConfig")}</div>
          {:else if status}
            <div class="settings-scroll">
              <section class="setting-card">
                <div class="setting-heading">
                  <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
                  <div>
                    <strong>{_t("storage.configSectionTitle")}</strong>
                    <p>{_t("storage.configSectionDesc")}</p>
                  </div>
                </div>
                <code class="path-value" title={status.configPath}>{status.configPath}</code>
              </section>

              <section class="setting-card">
                <div class="setting-heading split-heading">
                  <div class="heading-copy">
                    <span class="setting-icon"><AppIcon name="file" size={17} /></span>
                    <div>
                      <strong>{_t("storage.dataDirectoryTitle")}</strong>
                      <p>{_t("storage.dataDirectoryDesc")}</p>
                    </div>
                  </div>
                  <span class:custom={status.usesCustomDataDirectory} class="directory-badge">
                    {status.usesCustomDataDirectory ? _t("storage.custom") : _t("storage.default")}
                  </span>
                </div>

                <label for="data-directory">{_t("storage.directoryPath")}</label>
                <input
                  id="data-directory"
                  bind:value={dataDirectory}
                  autocomplete="off"
                  spellcheck="false"
                  placeholder={_t("storage.placeholderPath")}
                />

                <div class="setting-actions">
                  <button type="button" disabled={saving} onclick={restoreDefaultDirectory}
                    >{_t("storage.restoreDefault")}</button
                  >
                  <button
                    class="primary"
                    type="button"
                    disabled={saving}
                    onclick={saveCustomDirectory}
                  >
                    {saving ? _t("storage.saving") : _t("storage.saveDirectory")}
                  </button>
                </div>

                {#if pending}
                  <div class="pending-path">
                    <span>下次启动</span>
                    <code title={pending.storagePath}>{pending.storagePath}</code>
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
                <pre>storage/
├─ image/
�? └─ previews/
├─ files/
└─ database/
   ├─ clipboard.sqlite3
   └─ search-index/</pre>
              </section>

              <section class="setting-card">
                <div class="setting-heading split-heading">
                  <div class="heading-copy">
                    <span class="setting-icon"><AppIcon name="search" size={17} /></span>
                    <div>
                      <strong>{_t("storage.searchIndexTitle")}</strong>
                      <p>{_t("storage.searchIndexDesc")}</p>
                    </div>
                  </div>
                  <span class:custom={!status.searchIndexRebuildRequired} class="directory-badge">
                    {status.searchIndexRebuildRequired
                      ? _t("storage.rebuildRequired")
                      : _t("storage.ready", { version: status.searchIndexVersion })}
                  </span>
                </div>
                <code class="path-value" title={status.searchIndexPath}
                  >{status.searchIndexPath}</code
                >
                <div class="setting-actions">
                  <button type="button" disabled={rebuilding} onclick={rebuildIndex}>
                    {rebuilding ? _t("storage.rebuilding") : _t("storage.rebuildIndex")}
                  </button>
                </div>
              </section>

              <div class="storage-summary">
                <span>{_t("storage.databaseVersion", { version: status.schemaVersion })}</span>
                <span>{_t("storage.searchIndexVersion", { version: status.searchIndexVersion })}</span>
                <span>{_t("storage.recordCount", { count: status.itemCount })}</span>
                <span title={status.databasePath}>{_t("storage.sqliteConnected")}</span>
              </div>
            </div>
          {:else}
            <div class="settings-state">{feedback || _t("storage.storageUnavailable")}</div>
          {/if}

          {#if feedback && status}
            <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
          {/if}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .settings-backdrop {
    position: fixed;
    z-index: 50;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 12px;
    background: rgba(5, 5, 5, 0.72);
    backdrop-filter: blur(7px);
  }

  .settings-dialog {
    display: grid;
    grid-template-columns: 168px minmax(0, 1fr);
    width: min(900px, 100%);
    height: min(570px, 100%);
    overflow: hidden;
    border: 1px solid #323232;
    border-radius: 13px;
    background: #191919;
    box-shadow: 0 24px 80px rgba(0, 0, 0, 0.58);
  }

  .settings-sidebar {
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 16px 12px 13px;
    border-right: 1px solid #2c2c2c;
    background: #151515;
  }

  .settings-brand,
  .setting-heading,
  .heading-copy {
    display: flex;
    align-items: center;
  }

  .settings-brand {
    gap: 10px;
    padding: 2px 5px 18px;
  }

  .brand-icon,
  .setting-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    border: 1px solid #363636;
    color: #d2d2d2;
    background: #242424;
  }

  .brand-icon {
    width: 32px;
    height: 32px;
    border-radius: 9px;
  }

  .settings-brand strong,
  .settings-brand small {
    display: block;
  }

  .settings-brand strong {
    font-size: 12px;
  }
  .settings-brand small {
    margin-top: 2px;
    color: #6f6f6f;
    font-size: 10px;
  }

  nav {
    display: grid;
    gap: 4px;
  }

  nav button {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 9px 10px;
    border: 0;
    border-radius: 7px;
    color: #777;
    background: transparent;
    font: inherit;
    font-size: 11.5px;
    text-align: left;
  }

  nav button.active {
    color: #e4e4e4;
    background: #292929;
  }

  nav button:disabled {
    opacity: 0.45;
  }

  .sidebar-foot {
    display: grid;
    gap: 5px;
    margin-top: auto;
    padding: 10px 6px 0;
    color: #606060;
    font-size: 9.5px;
  }

  .sidebar-foot code {
    overflow: hidden;
    color: #858585;
    font: inherit;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .settings-content {
    position: relative;
    display: flex;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
  }

  .settings-content > header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 20px 22px 15px;
    border-bottom: 1px solid #292929;
  }

  .eyebrow {
    color: #777;
    font-size: 9.5px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 {
    margin: 5px 0 4px;
    color: #efefef;
    font-size: 18px;
    font-weight: 590;
  }

  header p,
  .setting-heading p {
    margin: 0;
    color: #777;
    line-height: 1.5;
  }

  header p {
    max-width: 430px;
    font-size: 10.5px;
  }

  .close-button {
    width: 28px;
    height: 28px;
    border: 1px solid #353535;
    border-radius: 7px;
    color: #999;
    background: #222;
    font-size: 18px;
    line-height: 1;
  }

  .settings-scroll {
    display: grid;
    gap: 10px;
    min-height: 0;
    padding: 14px 18px 48px;
    overflow: auto;
  }

  .setting-card {
    padding: 13px;
    border: 1px solid #303030;
    border-radius: 9px;
    background: #1e1e1e;
  }

  .setting-heading {
    gap: 10px;
  }
  .split-heading {
    justify-content: space-between;
  }
  .heading-copy {
    gap: 10px;
    min-width: 0;
  }

  .setting-icon {
    width: 29px;
    height: 29px;
    border-radius: 7px;
  }

  .setting-heading strong {
    display: block;
    color: #dedede;
    font-size: 11.5px;
    font-weight: 560;
  }

  .setting-heading p {
    margin-top: 2px;
    font-size: 9.8px;
  }

  .path-value,
  .pending-path code {
    display: block;
    overflow: hidden;
    color: #a7a7a7;
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .path-value {
    margin-top: 11px;
    padding: 8px 9px;
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    background: #181818;
    font-size: 9.5px;
  }

  .directory-badge {
    flex: 0 0 auto;
    padding: 3px 7px;
    border: 1px solid #393939;
    border-radius: 999px;
    color: #888;
    font-size: 9px;
  }

  .directory-badge.custom {
    border-color: rgba(112, 154, 255, 0.36);
    color: #9eb9ff;
    background: rgba(72, 111, 206, 0.12);
  }

  label {
    display: block;
    margin: 12px 0 6px;
    color: #8a8a8a;
    font-size: 9.5px;
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 9px 10px;
    border: 1px solid #343434;
    border-radius: 7px;
    outline: none;
    color: #d7d7d7;
    background: #171717;
    font:
      10.5px "Cascadia Code",
      "SFMono-Regular",
      Consolas,
      monospace;
  }

  input:focus {
    border-color: #555;
  }

  .setting-actions {
    display: flex;
    justify-content: flex-end;
    gap: 7px;
    margin-top: 9px;
  }

  .setting-actions button {
    padding: 7px 10px;
    border: 1px solid #383838;
    border-radius: 6px;
    color: #a3a3a3;
    background: #252525;
    font: inherit;
    font-size: 10px;
  }

  .setting-actions button.primary {
    border-color: #e3e3e3;
    color: #1c1c1c;
    background: #e3e3e3;
  }

  .setting-actions button:disabled {
    cursor: wait;
    opacity: 0.55;
  }

  .pending-path {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding-top: 9px;
    border-top: 1px solid #2d2d2d;
    color: #6f6f6f;
    font-size: 9.5px;
  }

  .pending-path code {
    font-size: 9.5px;
  }

  .directory-tree-card pre {
    margin: 11px 0 0;
    padding: 10px 12px;
    border: 1px solid #2e2e2e;
    border-radius: 7px;
    color: #999;
    background: #181818;
    font:
      9.5px/1.55 "Cascadia Code",
      "SFMono-Regular",
      Consolas,
      monospace;
  }

  .storage-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
    padding: 1px 3px;
    color: #666;
    font-size: 9.5px;
  }

  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: #777;
    font-size: 11px;
  }

  .settings-feedback {
    position: absolute;
    right: 18px;
    bottom: 13px;
    left: 18px;
    padding: 8px 10px;
    border: 1px solid #553434;
    border-radius: 7px;
    color: #d59c9c;
    background: rgba(48, 27, 27, 0.96);
    font-size: 10px;
  }

  .settings-feedback.success {
    border-color: #35513f;
    color: #9dc6aa;
    background: rgba(27, 45, 33, 0.96);
  }

  button {
    cursor: pointer;
  }

  @media (max-width: 560px) {
    .settings-dialog {
      grid-template-columns: 1fr;
    }
    .settings-sidebar {
      display: none;
    }
  }
</style>
