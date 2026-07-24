<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import KeyboardSettingsPanel from "$lib/components/KeyboardSettingsPanel.svelte";
  import IgnoredAppsSettingsPanel from "$lib/components/IgnoredAppsSettingsPanel.svelte";
  import GeneralSettingsPanel from "$lib/components/GeneralSettingsPanel.svelte";
  import CompactSettingsPanel from "$lib/components/CompactSettingsPanel.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import {
    configureStorageDirectory,
    getStorageStatus,
    rebuildSearchIndex,
    getPerformanceMetrics,
    repairDatabase,
    validateSearchIndex,
    type StorageDirectoryUpdate,
    type StorageStatus,
    type PerformanceMetrics,
    type RepairResult,
  } from "$lib/services/storage";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (
    path: string,
    params?: Record<string, string | number>,
  ) => resolvePath($messages, path, params);

  interface Props {
    open: boolean;
    onclose: () => void;
    standalone?: boolean;
  }

  let { open, onclose, standalone = false }: Props = $props();
  let status = $state<StorageStatus | null>(null);
  let pending = $state<StorageDirectoryUpdate | null>(null);
  let dataDirectory = $state("");
  let loading = $state(false);
  let saving = $state(false);
  let rebuilding = $state(false);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let restartNeeded = $state(false);
  let activeSection = $state<"general" | "compact" | "capture" | "storage" | "keyboard" | "ocr">("storage");

  let retentionPeriodDays = $state(90);
  let maxItemCount = $state(10000);
  let recycleBinDays = $state(30);
  let maxFileCopySize = $state(50 * 1024 * 1024);
  let maxFileCopySizeUnit = $state<"byte" | "KB" | "MB" | "GB">("MB");
  let maxFileCopyDisplay = $state(50);

  const unitMultipliers: Record<string, number> = { byte: 1, KB: 1024, MB: 1048576, GB: 1073741824 };

  function toDisplaySize(bytes: number, unit: string): number {
    return Math.round(bytes / (unitMultipliers[unit] || 1));
  }

  function fromDisplaySize(value: number, unit: string): number {
    return Math.round(value * (unitMultipliers[unit] || 1));
  }

  function updateMaxFileSizeFromDisplay() {
    maxFileCopySize = fromDisplaySize(maxFileCopyDisplay, maxFileCopySizeUnit);
  }

  function changeFileSizeUnit(unit: "byte" | "KB" | "MB" | "GB") {
    maxFileCopySizeUnit = unit;
    maxFileCopyDisplay = toDisplaySize(maxFileCopySize, unit);
  }

  function relativePath(absolute: string): string {
    if (!status) return absolute;
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

  let perfMetrics = $state<PerformanceMetrics | null>(null);
  let repairResult = $state<RepairResult | null>(null);
  let repairLoading = $state(false);

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB"];
    const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
    return `${(bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
  }

  let ocrEngine = $state("ppocr");
  let ocrPending = $state(0);
  let ocrCompleted = $state(0);
  let ocrAvailable = $state(false);
  let ocrInstalling = $state(false);
  let ocrProgressLabel = $state("");
  let ocrProgressPct = $state(-1);
  let ocrProgressCurrent = $state(0);
  let ocrProgressTotal = $state(0);
  let modelVariant = $state("tiny");
  let detScoreThreshold = $state(0.3);
  let detBoxThreshold = $state(0.6);
  let detUnclipRatio = $state(1.5);

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

    void loadPerformanceMetrics();
    void loadHistoryConfig();
    void loadOcrStatus();
  }

  async function loadOcrStatus() {
    try {
      const result = await invoke<{ pendingTasks: number; completedTasks: number; engine: string }>("get_ocr_status");
      if (result) {
        ocrPending = result.pendingTasks;
        ocrCompleted = result.completedTasks;
      }
      const status = await invoke<{ available: boolean }>("check_ppocr_status");
      if (status) ocrAvailable = status.available;
    } catch { /* ignore */ }
    try {
      const cfg = await invoke<{ engine: string; detScoreThreshold: number; detBoxThreshold: number; detUnclipRatio: number }>("get_ocr_config");
      if (cfg) {
        ocrEngine = cfg.engine;
        detScoreThreshold = cfg.detScoreThreshold;
        detBoxThreshold = cfg.detBoxThreshold;
        detUnclipRatio = cfg.detUnclipRatio;
      }
    } catch { /* ignore */ }
  }

  async function installPpocr() {
    ocrInstalling = true;
    ocrProgressPct = -1;
    ocrProgressLabel = "";
    ocrProgressCurrent = 0;
    ocrProgressTotal = 0;
    const unlisten = await listen<{
      filename: string;
      label: string;
      current: number;
      total: number;
      percentage: number;
    }>("ppocr-download-progress", (event) => {
      ocrProgressLabel = event.payload.label;
      ocrProgressPct = event.payload.percentage;
      ocrProgressCurrent = event.payload.current;
      ocrProgressTotal = event.payload.total;
    });
    try {
      const msg = await invoke<string>("install_ppocr", { variant: modelVariant });
      feedback = msg;
      feedbackSuccess = true;
      loadOcrStatus();
    } catch (e) {
      feedback = String(e);
    } finally {
      unlisten();
      ocrInstalling = false;
      ocrProgressPct = -1;
    }
  }

  async function loadHistoryConfig() {
    try {
      const result = await invoke<{ maxItems: number; retentionDays: number; recycleBinDays: number }>("get_history_config");
      if (result) {
        maxItemCount = result.maxItems;
        retentionPeriodDays = result.retentionDays;
        recycleBinDays = result.recycleBinDays;
      }
    } catch (error) {
      console.error("Unable to load history config", error);
    }
    try {
      const result = await invoke<{ maxFileCopySizeBytes: number }>("get_storage_config");
      if (result) {
        maxFileCopySize = result.maxFileCopySizeBytes;
        maxFileCopyDisplay = toDisplaySize(result.maxFileCopySizeBytes, maxFileCopySizeUnit);
      }
    } catch (error) {
      console.error("Unable to load storage config", error);
    }
  }

  async function loadPerformanceMetrics() {
    try {
      perfMetrics = await getPerformanceMetrics();
    } catch {
      perfMetrics = null;
    }
  }

  async function doRepair() {
    repairLoading = true;
    repairResult = null;
    feedback = "";
    try {
      repairResult = await repairDatabase();
      if (repairResult) {
        feedbackSuccess = repairResult.integrityOk;
        feedback = repairResult.integrityOk
          ? `Database integrity OK (${repairResult.pageCount} pages, ${repairResult.freelistCount} free)`
          : `Database repair needed: ${repairResult.integrityMessage}`;
      }
    } catch (error) {
      console.error("Database repair failed", error);
      feedback = "Database repair failed: " + (error instanceof Error ? error.message : String(error));
      feedbackSuccess = false;
    } finally {
      repairLoading = false;
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
      restartNeeded = pending.restartRequired;
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

  async function restartApp() {
    try {
      await invoke("restart_app");
    } catch {
      console.error("Unable to restart app");
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

  async function saveOcrEngine(engine: string) {
    try {
      await invoke("set_ocr_config", { engine });
      ocrEngine = engine;
      await invoke("restart_ocr_engine");
      feedback = `OCR 引擎已切换为 ${engine === 'ppocr' ? 'PP-OCRv6' : 'Tesseract'}，立即生效`;
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to save OCR config", error);
      feedback = error instanceof Error ? error.message : String(error);
    }
  }

  async function saveDetConfig() {
    try {
      await invoke("set_ocr_config", {
        engine: ocrEngine,
        detScoreThreshold,
        detBoxThreshold,
        detUnclipRatio,
      });
      await invoke("restart_ocr_engine");
      feedback = "检测参数已保存并生效";
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to save detection config", error);
      feedback = error instanceof Error ? error.message : String(error);
    }
  }

  async function saveHistoryConfig() {
    try {
      await invoke("set_history_config", {
        maxItems: maxItemCount,
        retentionDays: retentionPeriodDays,
        recycleBinDays: recycleBinDays,
      });
    } catch (error) {
      console.error("Unable to save history config", error);
    }
  }

  async function saveMaxFileCopySize() {
    try {
      await invoke("set_storage_config", { maxFileCopySizeBytes: maxFileCopySize });
    } catch (error) {
      console.error("Unable to save storage config", error);
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
  {@render backdropWrap()}
{/if}

{#snippet backdropWrap()}
  {#if standalone}
    <div
      class="settings-dialog settings-dialog--standalone"
      role="dialog"
      aria-labelledby="settings-title"
      tabindex="-1"
    >
      {@render dialogContent()}
    </div>
  {:else}
    <div class="settings-backdrop">
      <div
        class="settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        tabindex="-1"
      >
        {@render dialogContent()}
      </div>
    </div>
  {/if}
{/snippet}

{#snippet dialogContent()}
  <aside class="settings-sidebar" data-tauri-drag-region>
    <div class="settings-brand">
      <span class="brand-icon"><AppIcon name="clipboard" size={18} /></span>
      <div>
        <strong>Clipboard</strong>
        <small>0.1.0</small>
      </div>
    </div>

    <nav aria-label="设置分类">
      <button
        class:active={activeSection === "general"}
        type="button"
        onclick={() => (activeSection = "general")}
      >
        <AppIcon name="sliders" size={16} />
        <span>{_t("storage.generalTab")}</span>
      </button>
      <button
        class:active={activeSection === "compact"}
        type="button"
        onclick={() => (activeSection = "compact")}
      >
        <AppIcon name="grid" size={16} />
        <span>{_t("storage.compactTab")}</span>
      </button>
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
      <button
        class:active={activeSection === "ocr"}
        type="button"
        onclick={() => (activeSection = "ocr")}
      >
        <AppIcon name="eye" size={16} />
        <span>OCR</span>
      </button>
    </nav>

    <div class="sidebar-foot">
      <span>配置固定位置</span>
      <code>{activeSection === "keyboard" ? "conf/keyboard.json" : "conf/conf.json"}</code>
    </div>
  </aside>

  <div class="settings-content">
    {#if activeSection === "general"}
      <GeneralSettingsPanel {onclose} />
    {:else if activeSection === "compact"}
      <CompactSettingsPanel {onclose} />
    {:else if activeSection === "capture"}
      <IgnoredAppsSettingsPanel configPath={status?.configPath} {onclose} />
    {:else if activeSection === "keyboard"}
      <KeyboardSettingsPanel configPath={status?.keyboardConfigPath} {onclose} />
    {:else if activeSection === "ocr"}
      <header>
        <div>
          <span class="eyebrow">设置 / OCR</span>
          <h2>文字识别</h2>
          <p>OCR 引擎选择与状态</p>
        </div>
        <button class="close-button" type="button" aria-label="关闭设置" onclick={onclose}>×</button>
      </header>
      <div class="settings-scroll">
        <section class="setting-card">
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
            <div>
              <strong>OCR 引擎</strong>
              <p>PP-OCRv6 需安装 Python + paddleocr，Tesseract 需手动安装。</p>
            </div>
          </div>
          <div class="setting-actions" style="flex-wrap: wrap;">
            <button class:primary={ocrEngine === 'ppocr'} type="button" onclick={() => saveOcrEngine('ppocr')}>
              PP-OCRv6
            </button>
            <button class:primary={ocrEngine === 'tesseract'} type="button" onclick={() => saveOcrEngine('tesseract')}>
              Tesseract
            </button>
          </div>
        </section>

        <section class="setting-card">
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="download" size={17} /></span>
            <div>
              <strong>模型下载</strong>
              <p>{ocrAvailable ? 'PP-OCRv6 模型已就绪' : '下载 ONNX 模型到本地存储'}</p>
            </div>
          </div>
          {#if ocrAvailable}
            <div style="padding:8px 0; color:#51b96b; font-size:13px;">✓ 模型已下载到 storage/ppocr-models/</div>
          {:else}
            <div style="display:flex; gap:8px; align-items:stretch;">
              <div style="flex:1; min-width:0;">
                <label for="model-variant" style="display:block; font-size:11.5px; color:#8a8a8a; margin-bottom:4px;">模型规格</label>
                <select bind:value={modelVariant} class="model-select">
                  <option value="tiny">tiny (快速, ~5MB)</option>
                  <option value="medium">medium (平衡, ~15MB)</option>
                  <option value="large">large (高精度, ~30MB)</option>
                </select>
              </div>
              <button type="button" disabled={ocrInstalling} onclick={() => installPpocr()} style="align-self:flex-end; white-space:nowrap; padding:9px 14px; border:1px solid #343434; border-radius:7px; background:#252525; color:#d7d7d7; cursor:pointer; font-size:13px;">
                {ocrInstalling ? (ocrProgressPct >= 0 ? `${ocrProgressLabel} ${Math.round(ocrProgressPct)}%` : '下载中...') : '下载模型'}
              </button>
            </div>
            {#if ocrInstalling && ocrProgressPct >= 0}
              <div style="margin-top:2px; color:#888; font-size:10px;">{formatBytes(ocrProgressCurrent)} / {formatBytes(ocrProgressTotal)}</div>
              <div style="margin-top:4px; height:4px; background:#2a2a2a; border-radius:2px; overflow:hidden;">
                <div style="height:100%; width:{Math.min(100, Math.max(0, ocrProgressPct))}%; background:#4a90d9; border-radius:2px; transition:width 0.2s ease;"></div>
              </div>
            {/if}
          {/if}
        </section>

        <section class="setting-card">
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="search" size={17} /></span>
            <div>
              <strong>检测参数</strong>
              <p>调整文本区域检测参数，影响空格与换行的识别</p>
            </div>
          </div>
          <div style="display:grid; gap:12px;">
            <div>
              <label style="display:flex; justify-content:space-between; font-size:12px; color:#8a8a8a; margin-bottom:4px;">
                <span>分数阈值 (score)</span>
                <span style="color:#d7d7d7;">{detScoreThreshold.toFixed(2)}</span>
              </label>
              <input type="range" min="0.05" max="0.95" step="0.05" bind:value={detScoreThreshold} onchange={() => saveDetConfig()} style="width:100%; accent-color:#4a90d9;" />
              <div style="display:flex; justify-content:space-between; font-size:10.5px; color:#555; margin-top:2px;">
                <span>低 (更多区域)</span><span>高 (更少区域)</span>
              </div>
            </div>
            <div>
              <label style="display:flex; justify-content:space-between; font-size:12px; color:#8a8a8a; margin-bottom:4px;">
                <span>框阈值 (box)</span>
                <span style="color:#d7d7d7;">{detBoxThreshold.toFixed(2)}</span>
              </label>
              <input type="range" min="0.1" max="0.95" step="0.05" bind:value={detBoxThreshold} onchange={() => saveDetConfig()} style="width:100%; accent-color:#4a90d9;" />
              <div style="display:flex; justify-content:space-between; font-size:10.5px; color:#555; margin-top:2px;">
                <span>低 (更多区域)</span><span>高 (更少区域)</span>
              </div>
            </div>
            <div>
              <label style="display:flex; justify-content:space-between; font-size:12px; color:#8a8a8a; margin-bottom:4px;">
                <span>扩展比例 (unclip)</span>
                <span style="color:#d7d7d7;">{detUnclipRatio.toFixed(1)}</span>
              </label>
              <input type="range" min="1.0" max="4.0" step="0.1" bind:value={detUnclipRatio} onchange={() => saveDetConfig()} style="width:100%; accent-color:#4a90d9;" />
              <div style="display:flex; justify-content:space-between; font-size:10.5px; color:#555; margin-top:2px;">
                <span>小 (区域更紧凑)</span><span>大 (区域更宽松, 合并空格)</span>
            </div>
          </section>

        <section class="setting-card">
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="search" size={17} /></span>
            <div>
              <strong>任务状态</strong>
              <p>当前 OCR 队列与已识别统计</p>
            </div>
          </div>
          <div style="display:grid; grid-template-columns:1fr 1fr; gap:12px;">
            <div class="stat-item"><span class="stat-value">{ocrPending}</span><span class="stat-label">待处理</span></div>
            <div class="stat-item"><span class="stat-value">{ocrCompleted}</span><span class="stat-label">已完成</span></div>
          </div>
        </section>

        <p class="auto-save-note">修改即时生效，无需手动保存</p>
      </div>
    {:else}
      <header>
        <div>
          <span class="eyebrow">{_t("storage.settings")}</span>
          <h2 id="settings-title">{_t("storage.dataStorage")}</h2>
          <p>{_t("storage.configPath")}</p>
        </div>
        {#if !standalone}
          <button
            class="close-button"
            type="button"
            aria-label={_t("actions.close")}
            onclick={onclose}>×</button
          >
        {/if}
      </header>

      {#if loading}
        <div class="settings-state">{_t("storage.readingConfig")}</div>
      {:else if status}
        <div class="settings-scroll">
          <section class="setting-card setting-card-row">
            <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
            <span class="setting-label">常规配置文件</span>
            <span class="config-path">{relativePath(status!.configPath)}</span>
            <button type="button" class="open-btn" onclick={() => invoke("open_external_url", { url: status!.configPath })}>
              <AppIcon name="file" size={14} /> 打开
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
              <button type="button" disabled={saving} onclick={restoreDefaultDirectory}>{_t("storage.restoreDefault")}</button>
              <button type="button" disabled={saving} onclick={saveCustomDirectory}>{saving ? _t("storage.saving") : _t("storage.saveDirectory")}</button>
            </div>

            {#if pending}
              <div class="pending-path">
                <span>下次启动</span>
                <code title={pending.storagePath}>{pending.storagePath}</code>
                {#if restartNeeded}
                  <button class="restart-btn" type="button" onclick={restartApp}>立即重启</button>
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
            <pre>storage/
├─ image/
│  └─ previews/
├─ files/
└─ database/
   ├─ clipboard.sqlite3
   └─ search-index/</pre>
          </section>

          <section class="setting-card">
            <div class="setting-heading">
              <span class="setting-icon"><AppIcon name="search" size={17} /></span>
              <div>
                <strong>{_t("storage.searchIndexTitle")}</strong>
                <p>{_t("storage.searchIndexDesc")}</p>
              </div>
            </div>
            <div class="path-button-row">
              <code class="path-value-inline" title={status.searchIndexPath}>{relativePath(status.searchIndexPath)}</code>
              <button type="button" disabled={rebuilding} onclick={rebuildIndex}>
                {rebuilding ? _t("storage.rebuilding") : _t("storage.rebuildIndex")}
              </button>
            </div>
          </section>

          <section class="setting-card">
            <div class="setting-heading">
              <span class="setting-icon"><AppIcon name="bar-chart" size={17} /></span>
              <div>
                <strong>{_t("statistics.title")}</strong>
                <p>剪贴板数据统计概览</p>
              </div>
            </div>
            <div class="stats-grid">
              <div class="stat-item">
                <span class="stat-value">{status.itemCount}</span>
                <span class="stat-label">{_t("statistics.totalRecords")}</span>
              </div>
              <div class="stat-item">
                <span class="stat-value">{formatBytes(status.databaseSizeBytes)}</span>
                <span class="stat-label">{_t("statistics.dbSize")}</span>
              </div>
              <div class="stat-item">
                <span class="stat-value">{status.textCount + status.linkCount}</span>
                <span class="stat-label">文本/链接</span>
              </div>
              <div class="stat-item">
                <span class="stat-value">{status.imageCount} {_t("statistics.image")}</span>
                <span class="stat-label">{formatBytes(status.imageSizeBytes)}</span>
              </div>
              <div class="stat-item">
                <span class="stat-value">{status.fileCount} {_t("statistics.file")}</span>
                <span class="stat-label">{formatBytes(status.fileSizeBytes)}</span>
              </div>
              <div class="stat-item">
                <span class="stat-value">{formatBytes(status.searchIndexSizeBytes)}</span>
                <span class="stat-label">{_t("statistics.indexSize")}</span>
              </div>
            </div>
          </section>

          <section class="setting-card setting-card-row">
            <span class="setting-icon"><AppIcon name="filter" size={17} /></span>
            <span class="setting-label">{_t("captureSettings.retentionPeriod")}</span>
            <input
              type="number"
              bind:value={retentionPeriodDays}
              min="1"
              max="365"
              onchange={saveHistoryConfig}
            />
            <span class="number-suffix">{_t("captureSettings.days")}</span>
          </section>

          <section class="setting-card setting-card-row">
            <span class="setting-icon"><AppIcon name="file" size={17} /></span>
            <span class="setting-label">{_t("captureSettings.maxItemCount")}</span>
            <input
              type="number"
              bind:value={maxItemCount}
              min="100"
              step="100"
              onchange={saveHistoryConfig}
            />
            <span class="number-suffix">条</span>
          </section>

          <section class="setting-card setting-card-row">
            <span class="setting-icon"><AppIcon name="trash" size={17} /></span>
            <span class="setting-label">{_t("captureSettings.recycleBinDays")}</span>
            <input
              type="number"
              bind:value={recycleBinDays}
              min="0"
              max="365"
              onchange={saveHistoryConfig}
            />
            <span class="number-suffix">{_t("captureSettings.days")}</span>
          </section>

          <section class="setting-card setting-card-row">
            <span class="setting-icon"><AppIcon name="download" size={17} /></span>
            <span class="setting-label">{_t("captureSettings.maxFileCopySize")}</span>
            <input
              type="number"
              bind:value={maxFileCopyDisplay}
              min="1"
              oninput={updateMaxFileSizeFromDisplay}
              onchange={saveMaxFileCopySize}
            />
            <select class="unit-select" bind:value={maxFileCopySizeUnit} onchange={() => changeFileSizeUnit(maxFileCopySizeUnit)}>
              <option value="byte">B</option>
              <option value="KB">KB</option>
              <option value="MB">MB</option>
              <option value="GB">GB</option>
            </select>
          </section>

          <div class="storage-summary">
            <span>{_t("storage.searchIndexVersion", { version: status.searchIndexVersion })}</span>
            <span>{_t("storage.recordCount", { count: status.itemCount })}</span>
            <span title={relativePath(status.databasePath)}>{_t("storage.sqliteConnected")}</span>
          </div>

          {#if perfMetrics}
        <section class="setting-card">
              <div class="setting-heading">
                <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
                <div>
                  <strong>性能</strong>
                  <p>启动 {perfMetrics.startup.totalStartupMs}ms · DB {perfMetrics.startup.dbOpenMs}ms · 搜索 {perfMetrics.startup.searchInitMs}ms · 运行 {perfMetrics.memory.uptimeSeconds}s · 峰值 {Math.round(perfMetrics.memory.peakBytes / 1048576)} MB</p>
                </div>
              </div>
              {#if perfMetrics.searchLatency.searchesRecorded > 0}
                <div class="perf-grid">
                  <div class="perf-item">
                    <strong>{perfMetrics.searchLatency.searchesRecorded}</strong>
                    <span>搜索次数</span>
                  </div>
                  <div class="perf-item">
                    <strong>{perfMetrics.searchLatency.averageMs?.toFixed(1) ?? '-'}ms</strong>
                    <span>平均延迟</span>
                  </div>
                  <div class="perf-item">
                    <strong>{perfMetrics.searchLatency.p95Ms ?? '-'}ms</strong>
                    <span>p95</span>
                  </div>
                  <div class="perf-item">
                    <strong>{perfMetrics.searchLatency.p99Ms ?? '-'}ms</strong>
                    <span>p99</span>
                  </div>
                </div>
              {/if}
            </section>
          {/if}

          <section class="setting-card setting-card-row">
            <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
            <span class="setting-label">数据库维护</span>
            <button type="button" disabled={repairLoading} onclick={doRepair}>
              {repairLoading ? '检查中...' : '修复数据库'}
            </button>
          </section>
          {#if repairResult}
            <div class="repair-result">
              <span class:ok={repairResult.integrityOk} class:fail={!repairResult.integrityOk}>
                {repairResult.integrityOk ? '完整性正常' : '发现问题'}
              </span>
              <code>{repairResult.integrityMessage}</code>
            </div>
          {/if}

          <p class="auto-save-note">修改即时生效，无需手动保存</p>
        </div>
      {:else}
        <div class="settings-state">{feedback || _t("storage.storageUnavailable")}</div>
      {/if}

      {#if feedback && status}
        <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
      {/if}
    {/if}
  </div>
{/snippet}

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
    width: min(728px, 100%);
    height: min(570px, 100%);
    overflow: hidden;
    border: 1px solid #323232;
    border-radius: 13px;
    background: #191919;
    box-shadow: 0 24px 80px rgba(0, 0, 0, 0.58);
  }

  .settings-dialog--standalone {
    width: 100%;
    height: 100%;
    border-radius: 0;
    border: none;
    box-shadow: none;
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
    font-size: 13.5px;
  }
  .settings-brand small {
    margin-top: 2px;
    color: #6f6f6f;
    font-size: 11px;
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
    padding: 8px 10px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: #999;
    background: #1a1a1a;
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
    transition: background 100ms ease, color 100ms ease, border-color 100ms ease;
  }

  nav button:hover {
    color: #ccc;
    background: #252525;
    border-color: #3a3a3a;
  }

  nav button.active {
    border-color: #5a5a5a;
    color: #f0f0f0;
    background: #333;
  }

  nav button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .sidebar-foot {
    display: grid;
    gap: 5px;
    margin-top: auto;
    padding: 10px 6px 0;
    color: #606060;
    font-size: 11px;
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
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 {
    margin: 5px 0 4px;
    color: #efefef;
    font-size: 19px;
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
    font-size: 12px;
  }

  .close-button {
    width: 28px;
    height: 28px;
    border: 1px solid #353535;
    border-radius: 7px;
    color: #999;
    background: #222;
    font-size: 19px;
    line-height: 1;
    cursor: pointer;
  }

  .settings-scroll {
    display: grid;
    gap: 8px;
    min-height: 0;
    padding: 14px 18px 48px;
    overflow: auto;
    scrollbar-color: #9a9a9a transparent;
    scrollbar-width: thin;
  }

  .settings-scroll::-webkit-scrollbar {
    width: 7px;
  }

  .settings-scroll::-webkit-scrollbar-track {
    background: transparent;
  }

  .settings-scroll::-webkit-scrollbar-thumb {
    border-radius: 10px;
    background: #858585;
  }

  .setting-card {
    padding: 10px 13px;
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
    font-size: 13px;
    font-weight: 560;
  }

  .setting-heading p {
    margin-top: 2px;
    font-size: 11px;
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
    font-size: 11px;
  }

  .directory-badge {
    flex: 0 0 auto;
    padding: 3px 7px;
    border: 1px solid #393939;
    border-radius: 999px;
    color: #888;
    font-size: 10.5px;
  }

  .directory-badge.custom {
    border-color: rgba(112, 154, 255, 0.36);
    color: #9eb9ff;
    background: rgba(72, 111, 206, 0.12);
  }

  .inline-badge {
    display: inline-block;
    margin-left: 8px;
    padding: 2px 7px;
    border: 1px solid #393939;
    border-radius: 999px;
    color: #888;
    font-size: 10px;
    font-weight: 500;
    vertical-align: middle;
  }

  .inline-badge.custom {
    border-color: rgba(112, 154, 255, 0.36);
    color: #9eb9ff;
    background: rgba(72, 111, 206, 0.12);
  }

  label {
    display: block;
    margin: 12px 0 6px;
    color: #8a8a8a;
    font-size: 11px;
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    outline: none;
    color: #d7d7d7;
    background: #1a1a1a;
    font:
      12px "Cascadia Code",
      "SFMono-Regular",
      Consolas,
      monospace;
    transition: border-color 120ms ease;
  }

  input:focus {
    border-color: #555;
  }

  select,
  .model-select {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 28px 8px 10px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    outline: none;
    color: #d7d7d7;
    background: #1a1a1a;
    font-size: 13px;
    cursor: pointer;
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23888' d='M3 5l3 3 3-3'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 8px center;
    transition: border-color 120ms ease;
  }

  select:focus,
  .model-select:focus {
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
    font-size: 11.5px;
    cursor: pointer;
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
    font-size: 11px;
  }

  .pending-path code {
    font-size: 11px;
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

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 8px;
    margin-top: 12px;
  }

  .stat-item {
    padding: 10px;
    border: 1px solid #2e2e2e;
    border-radius: 7px;
    background: #141414;
    text-align: center;
  }

  .stat-value {
    display: block;
    color: #e4e4e4;
    font-size: 17px;
    font-weight: 600;
    margin-bottom: 4px;
  }

  .stat-label {
    color: #777;
    font-size: 11px;
  }

  .number-input-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
  }

  .number-input-row input {
    width: 120px;
  }

  .number-suffix {
    color: #888;
    font-size: 12px;
    flex-shrink: 0;
  }

  .storage-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
    padding: 1px 3px;
    color: #666;
    font-size: 11px;
  }

  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: #777;
    font-size: 12.5px;
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
    font-size: 11.5px;
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

  .perf-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
    margin-top: 10px;
  }

  .perf-item {
    text-align: center;
    padding: 6px 4px;
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    background: #181818;
  }

  .perf-item strong {
    display: block;
    color: #dedede;
    font-size: 14.5px;
    font-weight: 560;
  }

  .perf-item span {
    color: #6f6f6f;
    font-size: 10.5px;
  }

  .perf-mem {
    margin-top: 8px;
    color: #6f6f6f;
    font-size: 11px;
  }

  .repair-result {
    margin-top: 10px;
    padding: 8px 9px;
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    background: #181818;
    font-size: 11px;
  }

  .repair-result span.ok {
    color: #9dc6aa;
  }

  .repair-result span.fail {
    color: #d59c9c;
  }

  .repair-result code {
    display: block;
    margin-top: 4px;
    color: #a7a7a7;
    font-size: 10.5px;
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: #666;
    font-size: 11.5px;
    text-align: center;
  }

  .setting-card-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 13px;
  }

  .setting-card-row .setting-icon {
    flex-shrink: 0;
  }

  .setting-label {
    flex: 1;
    min-width: 0;
    color: #dedede;
    font-size: 11.5px;
    font-weight: 560;
  }

  .setting-card-row input {
    width: 100px;
    flex-shrink: 0;
    padding: 7px 10px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #d7d7d7;
    background: #1a1a1a;
    font: 12px inherit;
    outline: none;
    transition: border-color 120ms ease;
    -moz-appearance: textfield;
  }

  .setting-card-row input::-webkit-outer-spin-button,
  .setting-card-row input::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  .setting-card-row button {
    padding: 5px 12px;
    border: 1px solid #3a3a3a;
    border-radius: 5px;
    color: #a3a3a3;
    background: #252525;
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background 100ms ease, color 100ms ease;
  }

  .setting-card-row button:hover {
    color: #ccc;
    background: #2e2e2e;
  }

  .setting-card-row button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .setting-card-row input:focus {
    border-color: #555;
  }

  .setting-card-row .number-suffix {
    color: #888;
    font-size: 11px;
    flex-shrink: 0;
  }

  .config-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: #777;
    font-size: 10.5px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .open-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: 1px solid #3a3a3a;
    border-radius: 5px;
    color: #999;
    background: #222;
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background 100ms ease, color 100ms ease;
  }

  .open-btn:hover {
    color: #ccc;
    background: #2e2e2e;
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
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #d7d7d7;
    background: #1a1a1a;
    font: 12px "Cascadia Code", Consolas, monospace;
    outline: none;
    transition: border-color 120ms ease;
  }

  .dir-input-row input:focus {
    border-color: #555;
  }

  .dir-input-row button {
    padding: 7px 12px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #a3a3a3;
    background: #252525;
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background 100ms ease, color 100ms ease;
  }

  .dir-input-row button:hover {
    color: #ccc;
    background: #2e2e2e;
  }

  .dir-input-row button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .path-button-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
  }

  .path-value-inline {
    flex: 1;
    min-width: 0;
    padding: 6px 9px;
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    color: #a7a7a7;
    background: #181818;
    font: 10.5px "Cascadia Code", Consolas, monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .path-button-row button {
    padding: 6px 12px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #a3a3a3;
    background: #252525;
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background 100ms ease, color 100ms ease;
  }

  .path-button-row button:hover {
    color: #ccc;
    background: #2e2e2e;
  }

  .path-button-row button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .unit-select {
    width: 56px;
    padding: 7px 2px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #d7d7d7;
    background: #1a1a1a;
    font-size: 11px;
    cursor: pointer;
    flex-shrink: 0;
    text-align: center;
    text-align-last: center;
    outline: none;
    transition: border-color 120ms ease;
  }

  .unit-select:focus {
    border-color: #555;
  }

  .restart-btn {
    margin-left: auto;
    padding: 5px 12px;
    border: 1px solid #4aa8ff;
    border-radius: 6px;
    color: #4aa8ff;
    background: rgba(74, 168, 255, 0.1);
    font-size: 11px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .restart-btn:hover {
    background: rgba(74, 168, 255, 0.2);
  }
</style>
