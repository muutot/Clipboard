<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import {
    configureStorageDirectory,
    getStorageStatus,
    rebuildSearchIndex,
    type StorageDirectoryUpdate,
    type StorageStatus,
  } from "$lib/services/storage";

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
        feedback = "浏览器预览无法读取桌面端存储配置";
      }
    } catch (error) {
      console.error("Unable to load storage settings", error);
      status = null;
      feedback = "读取存储配置失败";
    } finally {
      loading = false;
    }
  }

  async function saveCustomDirectory() {
    const requested = dataDirectory.trim();
    if (!requested) {
      feedback = "请输入绝对路径，或使用“恢复默认”";
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
        ? "已写入 conf/conf.json，重启应用后使用新目录"
        : "当前已经使用这个数据目录";
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
      feedback = `索引重建完成：处理 ${summary.processedEvents} 个事件，写入 ${summary.upsertedDocuments} 条记录`;
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
    <div class="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" tabindex="-1">
      <aside class="settings-sidebar">
        <div class="settings-brand">
          <span class="brand-icon"><AppIcon name="clipboard" size={18} /></span>
          <div>
            <strong>Clipboard</strong>
            <small>0.1.0</small>
          </div>
        </div>

        <nav aria-label="设置分类">
          <button class="active" type="button">
            <AppIcon name="file" size={16} />
            <span>存储</span>
          </button>
          <button type="button" disabled>
            <AppIcon name="settings" size={16} />
            <span>常规</span>
          </button>
        </nav>

        <div class="sidebar-foot">
          <span>配置固定位置</span>
          <code>conf/conf.json</code>
        </div>
      </aside>

      <div class="settings-content">
        <header>
          <div>
            <span class="eyebrow">设置 / 存储</span>
            <h2 id="settings-title">数据存储</h2>
            <p>配置文件固定在项目目录；图片、文件和数据库可切换到其他数据目录。</p>
          </div>
          <button class="close-button" type="button" aria-label="关闭设置" onclick={onclose}>×</button>
        </header>

        {#if loading}
          <div class="settings-state">正在读取本地配置…</div>
        {:else if status}
          <div class="settings-scroll">
            <section class="setting-card">
              <div class="setting-heading">
                <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
                <div>
                  <strong>统一配置文件</strong>
                  <p>所有用户设置都写入这一个 JSON 文件，切换数据目录时它不会迁移。</p>
                </div>
              </div>
              <code class="path-value" title={status.configPath}>{status.configPath}</code>
            </section>

            <section class="setting-card">
              <div class="setting-heading split-heading">
                <div class="heading-copy">
                  <span class="setting-icon"><AppIcon name="file" size={17} /></span>
                  <div>
                    <strong>数据目录</strong>
                    <p>所选目录下始终创建统一的 storage 子目录结构。</p>
                  </div>
                </div>
                <span class:custom={status.usesCustomDataDirectory} class="directory-badge">
                  {status.usesCustomDataDirectory ? "自定义" : "默认"}
                </span>
              </div>

              <label for="data-directory">目录绝对路径</label>
              <input
                id="data-directory"
                bind:value={dataDirectory}
                autocomplete="off"
                spellcheck="false"
                placeholder="例如 D:\ClipboardData"
              />

              <div class="setting-actions">
                <button type="button" disabled={saving} onclick={restoreDefaultDirectory}>恢复默认</button>
                <button class="primary" type="button" disabled={saving} onclick={saveCustomDirectory}>
                  {saving ? "保存中…" : "保存目录"}
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
                  <strong>目录结构</strong>
                  <p>数据库索引作为数据库的派生数据放在同一目录内。</p>
                </div>
              </div>
              <pre>storage/
├─ image/
│  └─ previews/
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
                    <strong>全文搜索索引</strong>
                    <p>中文 N-gram 索引是 SQLite 数据的派生结果，可随时安全重建。</p>
                  </div>
                </div>
                <span class:custom={!status.searchIndexRebuildRequired} class="directory-badge">
                  {status.searchIndexRebuildRequired ? "待重建" : `v${status.searchIndexVersion} 就绪`}
                </span>
              </div>
              <code class="path-value" title={status.searchIndexPath}>{status.searchIndexPath}</code>
              <div class="setting-actions">
                <button type="button" disabled={rebuilding} onclick={rebuildIndex}>
                  {rebuilding ? "重建中…" : "一键重建索引"}
                </button>
              </div>
            </section>

            <div class="storage-summary">
              <span>数据库版本 {status.schemaVersion}</span>
              <span>搜索索引 v{status.searchIndexVersion}</span>
              <span>{status.itemCount} 条记录</span>
              <span title={status.databasePath}>SQLite 已连接</span>
            </div>
          </div>
        {:else}
          <div class="settings-state">{feedback || "桌面端存储服务不可用"}</div>
        {/if}

        {#if feedback && status}
          <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
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
    width: min(680px, 100%);
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

  .settings-brand strong { font-size: 12px; }
  .settings-brand small { margin-top: 2px; color: #6f6f6f; font-size: 10px; }

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

  nav button:disabled { opacity: 0.45; }

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

  header p { max-width: 430px; font-size: 10.5px; }

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

  .setting-heading { gap: 10px; }
  .split-heading { justify-content: space-between; }
  .heading-copy { gap: 10px; min-width: 0; }

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

  .setting-heading p { margin-top: 2px; font-size: 9.8px; }

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
    font: 10.5px "Cascadia Code", "SFMono-Regular", Consolas, monospace;
  }

  input:focus { border-color: #555; }

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

  .setting-actions button:disabled { cursor: wait; opacity: 0.55; }

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

  .pending-path code { font-size: 9.5px; }

  .directory-tree-card pre {
    margin: 11px 0 0;
    padding: 10px 12px;
    border: 1px solid #2e2e2e;
    border-radius: 7px;
    color: #999;
    background: #181818;
    font: 9.5px/1.55 "Cascadia Code", "SFMono-Regular", Consolas, monospace;
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

  button { cursor: pointer; }

  @media (max-width: 560px) {
    .settings-dialog { grid-template-columns: 1fr; }
    .settings-sidebar { display: none; }
  }
</style>
