<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import KeyboardSettingsPanel from "$lib/components/KeyboardSettingsPanel.svelte";
  import IgnoredAppsSettingsPanel from "$lib/components/IgnoredAppsSettingsPanel.svelte";
  import GeneralSettingsPanel from "$lib/components/GeneralSettingsPanel.svelte";
  import CompactSettingsPanel from "$lib/components/CompactSettingsPanel.svelte";
  import FontSizeSettingsPanel from "$lib/components/FontSizeSettingsPanel.svelte";
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

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

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

  $effect(() => {
    if (feedback) {
      const t = setTimeout(() => {
        feedback = "";
      }, 2000);
      return () => clearTimeout(t);
    }
  });
  let restartNeeded = $state(false);
  let activeSection = $state<
    "general" | "compact" | "font" | "capture" | "storage" | "keyboard" | "ocr" | "statistics"
  >("storage");

  let retentionPeriodDays = $state(90);
  let maxItemCount = $state(10000);
  let recycleBinDays = $state(30);
  let maxFileCopySize = $state(50 * 1024 * 1024);
  let maxFileCopySizeUnit = $state<"byte" | "KB" | "MB" | "GB">("MB");
  let maxFileCopyDisplay = $state(50);

  const unitMultipliers: Record<string, number> = {
    byte: 1,
    KB: 1024,
    MB: 1048576,
    GB: 1073741824,
  };

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
  let installedVariant = $state<string>("");
  let activeVariant = $state<string>("");
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
      const result = await invoke<{ pendingTasks: number; completedTasks: number; engine: string }>(
        "get_ocr_status",
      );
      if (result) {
        ocrPending = result.pendingTasks;
        ocrCompleted = result.completedTasks;
      }
      const status = await invoke<{ available: boolean }>("check_ppocr_status");
      if (status) {
        ocrAvailable = status.available;
        if (status.available) installedVariant = installedVariant || modelVariant;
      }
    } catch {
      /* ignore */
    }
    try {
      const cfg = await invoke<{
        engine: string;
        ppocrModelVariant: string;
        detScoreThreshold: number;
        detBoxThreshold: number;
        detUnclipRatio: number;
      }>("get_ocr_config");
      if (cfg) {
        ocrEngine = cfg.engine;
        detScoreThreshold = cfg.detScoreThreshold;
        detBoxThreshold = cfg.detBoxThreshold;
        detUnclipRatio = cfg.detUnclipRatio;
        if (cfg.ppocrModelVariant) {
          installedVariant = cfg.ppocrModelVariant;
          activeVariant = cfg.ppocrModelVariant;
          modelVariant = cfg.ppocrModelVariant;
        }
      }
    } catch {
      /* ignore */
    }
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
      installedVariant = modelVariant;
      activeVariant = modelVariant;
      loadOcrStatus();
    } catch (e) {
      feedback = String(e);
    } finally {
      unlisten();
      ocrInstalling = false;
      ocrProgressPct = -1;
    }
  }

  async function applyModel() {
    if (activeVariant === modelVariant) {
      feedback = "模型已应用";
      feedbackSuccess = true;
      return;
    }
    try {
      await saveOcrEngine("ppocr");
      activeVariant = modelVariant;
      feedback = "切换成功";
      feedbackSuccess = true;
    } catch (e) {
      feedback = String(e);
    }
  }

  async function loadHistoryConfig() {
    try {
      const result = await invoke<{
        maxItems: number;
        retentionDays: number;
        recycleBinDays: number;
      }>("get_history_config");
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
      feedback =
        "Database repair failed: " + (error instanceof Error ? error.message : String(error));
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
      feedback = `OCR 引擎已切换为 ${engine === "ppocr" ? "PP-OCRv6" : "Tesseract"}，立即生效`;
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

    <nav class="settings-primary-nav" aria-label="设置分类">
      <button
        class:active={activeSection === "general" ||
          activeSection === "compact" ||
          activeSection === "font"}
        type="button"
        onclick={() => (activeSection = "general")}
      >
        <AppIcon name="sliders" size={16} />
        <span>{_t("storage.generalTab")}</span>
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
      <button
        class:active={activeSection === "statistics"}
        type="button"
        onclick={() => (activeSection = "statistics")}
      >
        <AppIcon name="bar-chart" size={16} />
        <span>统计</span>
      </button>
    </nav>

    <div class="sidebar-foot">
      <span>配置固定位置</span>
      <code>{activeSection === "keyboard" ? "conf/keyboard.json" : "conf/conf.json"}</code>
    </div>
  </aside>

  <div class="settings-content">
    {#if activeSection === "general" || activeSection === "compact" || activeSection === "font"}
      <nav class="settings-subnav" aria-label={_t("storage.generalTab")}>
        <button
          type="button"
          class:active={activeSection === "general"}
          aria-current={activeSection === "general" ? "page" : undefined}
          onclick={() => (activeSection = "general")}
        >
          {_t("storage.generalTab")}
        </button>
        <button
          type="button"
          class:active={activeSection === "compact"}
          aria-current={activeSection === "compact" ? "page" : undefined}
          onclick={() => (activeSection = "compact")}
        >
          {_t("storage.compactTab")}
        </button>
        <button
          type="button"
          class:active={activeSection === "font"}
          aria-current={activeSection === "font" ? "page" : undefined}
          onclick={() => (activeSection = "font")}
        >
          {_t("general.fontSize")}
        </button>
      </nav>
    {/if}
    {#if activeSection === "general"}
      <GeneralSettingsPanel {onclose} />
    {:else if activeSection === "compact"}
      <CompactSettingsPanel {onclose} />
    {:else if activeSection === "font"}
      <FontSizeSettingsPanel {onclose} />
    {:else if activeSection === "capture"}
      <IgnoredAppsSettingsPanel iconsDir={status?.iconsDir} {onclose} />
    {:else if activeSection === "keyboard"}
      <KeyboardSettingsPanel configPath={status?.keyboardConfigPath} {onclose} />
    {:else if activeSection === "ocr"}
      <header>
        <div>
          <span class="eyebrow">设置 / OCR</span>
          <h2>文字识别</h2>
          <p>OCR 引擎选择与状态</p>
        </div>
        <button class="close-button" type="button" aria-label="关闭设置" onclick={onclose}>×</button
        >
      </header>
      <div class="settings-scroll">
        <section class="setting-card setting-card-row">
          <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
          <span class="setting-label">OCR 引擎</span>
          <select
            class="model-select"
            style="flex:1; max-width:180px;"
            bind:value={ocrEngine}
            onchange={() => saveOcrEngine(ocrEngine)}
          >
            <option value="ppocr">PP-OCRv6</option>
            <option value="tesseract">Tesseract</option>
          </select>
        </section>

        <section class="setting-card setting-card-row">
          <span class="setting-icon"><AppIcon name="download" size={17} /></span>
          <span class="setting-label">模型</span>
          <select bind:value={modelVariant} class="model-select" style="flex:1; max-width:200px;">
            <option value="tiny">tiny (~5MB){installedVariant === "tiny" ? " ✓" : ""}</option>
            <option value="medium">medium (~15MB){installedVariant === "medium" ? " ✓" : ""}</option
            >
            <option value="large">large (~30MB){installedVariant === "large" ? " ✓" : ""}</option>
          </select>
          {#if installedVariant === modelVariant}
            <button type="button" onclick={applyModel}>应用</button>
          {:else}
            <button type="button" disabled={ocrInstalling} onclick={() => installPpocr()}>
              {ocrInstalling
                ? ocrProgressPct >= 0
                  ? `${ocrProgressLabel} ${Math.round(ocrProgressPct)}%`
                  : "下载中..."
                : "下载"}
            </button>
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
              <label
                for="det-score"
                style="display:flex; justify-content:space-between; font-size:var(--font-size-secondary,11px); color:#8a8a8a; margin-bottom:4px;"
              >
                <span>分数阈值 (score)</span>
                <span style="color:#d7d7d7;">{detScoreThreshold.toFixed(2)}</span>
              </label>
              <input
                id="det-score"
                type="range"
                min="0.05"
                max="0.95"
                step="0.05"
                bind:value={detScoreThreshold}
                onchange={() => saveDetConfig()}
                style="width:100%; accent-color:#4a90d9;"
              />
              <div
                style="display:flex; justify-content:space-between; font-size:var(--font-size-tiny,10px); color:#555; margin-top:2px;"
              >
                <span>低 (更多区域)</span><span>高 (更少区域)</span>
              </div>
            </div>
            <div>
              <label
                for="det-box"
                style="display:flex; justify-content:space-between; font-size:var(--font-size-secondary,11px); color:#8a8a8a; margin-bottom:4px;"
              >
                <span>框阈值 (box)</span>
                <span style="color:#d7d7d7;">{detBoxThreshold.toFixed(2)}</span>
              </label>
              <input
                id="det-box"
                type="range"
                min="0.1"
                max="0.95"
                step="0.05"
                bind:value={detBoxThreshold}
                onchange={() => saveDetConfig()}
                style="width:100%; accent-color:#4a90d9;"
              />
              <div
                style="display:flex; justify-content:space-between; font-size:var(--font-size-tiny,10px); color:#555; margin-top:2px;"
              >
                <span>低 (更多区域)</span><span>高 (更少区域)</span>
              </div>
            </div>
            <div>
              <label
                for="det-unclip"
                style="display:flex; justify-content:space-between; font-size:var(--font-size-secondary,11px); color:#8a8a8a; margin-bottom:4px;"
              >
                <span>扩展比例 (unclip)</span>
                <span style="color:#d7d7d7;">{detUnclipRatio.toFixed(1)}</span>
              </label>
              <input
                id="det-unclip"
                type="range"
                min="1.0"
                max="4.0"
                step="0.1"
                bind:value={detUnclipRatio}
                onchange={() => saveDetConfig()}
                style="width:100%; accent-color:#4a90d9;"
              />
              <div
                style="display:flex; justify-content:space-between; font-size:var(--font-size-tiny,10px); color:#555; margin-top:2px;"
              >
                <span>小 (区域更紧凑)</span><span>大 (区域更宽松, 合并空格)</span>
              </div>
            </div>
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
            <div class="stat-item">
              <span class="stat-value">{ocrPending}</span><span class="stat-label">待处理</span>
            </div>
            <div class="stat-item">
              <span class="stat-value">{ocrCompleted}</span><span class="stat-label">已完成</span>
            </div>
          </div>
        </section>

        <p class="auto-save-note">修改即时生效，无需手动保存</p>
      </div>
      {#if feedback}
        <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
      {/if}
    {:else if activeSection === "statistics"}
      <div class="settings-scroll">
        <header>
          <div>
            <span class="eyebrow">设置 / 统计</span>
            <h2>数据统计</h2>
            <p>存储分布与性能</p>
          </div>
        </header>

        {#if status}
          <section class="setting-card">
            <div class="setting-heading">
              <span class="setting-icon"><AppIcon name="bar-chart" size={17} /></span>
              <div>
                <strong>存储分布</strong>
                <p>各项数据占用空间</p>
              </div>
            </div>
            <div class="stats-rows">
              <div class="stat-row">
                <span>总记录数</span>
                <span>{status.itemCount}</span>
              </div>
              <div class="stat-row">
                <span>文本 / 链接</span>
                <span>{status.textCount + status.linkCount}</span>
              </div>
              <div class="stat-row">
                <span>图片</span>
                <span>{status.imageCount} 张 · {formatBytes(status.imageSizeBytes)}</span>
              </div>
              <div class="stat-row">
                <span>文件</span>
                <span>{status.fileCount} 个 · {formatBytes(status.fileSizeBytes)}</span>
              </div>
              <div class="stat-row">
                <span>数据库</span>
                <span>{formatBytes(status.databaseSizeBytes)}</span>
              </div>
              <div class="stat-row">
                <span>搜索索引</span>
                <span>{formatBytes(status.searchIndexSizeBytes)}</span>
              </div>
            </div>
          </section>
        {/if}

        {#if perfMetrics}
          <section class="setting-card">
            <div class="setting-heading">
              <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
              <div>
                <strong>性能</strong>
                <p>应用启动与搜索耗时统计</p>
              </div>
            </div>
            <div class="stats-rows">
              <div class="stat-row">
                <span>启动总耗时</span>
                <span>{perfMetrics.startup.totalStartupMs}ms</span>
              </div>
              <div class="stat-row">
                <span>数据库打开</span>
                <span>{perfMetrics.startup.dbOpenMs}ms</span>
              </div>
              <div class="stat-row">
                <span>搜索初始化</span>
                <span>{perfMetrics.startup.searchInitMs}ms</span>
              </div>
              <div class="stat-row">
                <span>数据库迁移</span>
                <span>{perfMetrics.startup.migrationsMs}ms</span>
              </div>
              <div class="stat-row">
                <span>运行时长</span>
                <span>{perfMetrics.memory.uptimeSeconds}s</span>
              </div>
              <div class="stat-row">
                <span>内存峰值</span>
                <span>{Math.round(perfMetrics.memory.peakBytes / 1048576)} MB</span>
              </div>
              {#if perfMetrics.searchLatency.searchesRecorded > 0}
                <div class="stat-row">
                  <span>搜索次数</span>
                  <span>{perfMetrics.searchLatency.searchesRecorded}</span>
                </div>
                <div class="stat-row">
                  <span>平均搜索耗时</span>
                  <span>{perfMetrics.searchLatency.averageMs?.toFixed(1) ?? "-"}ms</span>
                </div>
                <div class="stat-row">
                  <span>P95 搜索耗时</span>
                  <span>{perfMetrics.searchLatency.p95Ms ?? "-"}ms</span>
                </div>
                <div class="stat-row">
                  <span>P99 搜索耗时</span>
                  <span>{perfMetrics.searchLatency.p99Ms ?? "-"}ms</span>
                </div>
              {/if}
            </div>
          </section>
        {/if}

        <p class="auto-save-note">启动性能为应用初始化耗时，搜索延迟需触发搜索后统计</p>
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
            <button
              type="button"
              class="open-btn"
              onclick={() => invoke("open_external_url", { url: status!.configPath })}
            >
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
              <button type="button" disabled={saving} onclick={restoreDefaultDirectory}
                >{_t("storage.restoreDefault")}</button
              >
              <button type="button" disabled={saving} onclick={saveCustomDirectory}
                >{saving ? _t("storage.saving") : _t("storage.saveDirectory")}</button
              >
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
            <pre>data/
├─ conf/                           ← 配置文件
│  ├─ conf.json                    ← 常规设置
│  └─ keyboard.json                ← 快捷键
├─ models/                         ← OCR 模型
│  └─ ppocr/
├─ image/                          ← 图片原图
│  └─ previews/                    ← 缩略图
├─ files/                          ← 文件副本
├─ icons/                          ← 应用图标缓存
└─ database/
   ├─ clipboard.sqlite3            ← 剪贴板数据库
   ├─ clipboard.sqlite3-wal        ← 预写日志
   └─ search-index/                ← 全文搜索索引</pre>
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
              <code class="path-value-inline" title={status.searchIndexPath}
                >{relativePath(status.searchIndexPath)}</code
              >
              <button type="button" disabled={rebuilding} onclick={rebuildIndex}>
                {rebuilding ? _t("storage.rebuilding") : _t("storage.rebuildIndex")}
              </button>
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
            <select
              class="unit-select"
              bind:value={maxFileCopySizeUnit}
              onchange={() => changeFileSizeUnit(maxFileCopySizeUnit)}
            >
              <option value="byte">B</option>
              <option value="KB">KB</option>
              <option value="MB">MB</option>
              <option value="GB">GB</option>
            </select>
          </section>

          <section class="setting-card setting-card-row">
            <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
            <span class="setting-label">数据库维护</span>
            <button type="button" disabled={repairLoading} onclick={doRepair}>
              {repairLoading ? "检查中..." : "修复数据库"}
            </button>
          </section>
          {#if repairResult}
            <div class="repair-result">
              <span class:ok={repairResult.integrityOk} class:fail={!repairResult.integrityOk}>
                {repairResult.integrityOk ? "完整性正常" : "发现问题"}
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
    --settings-page-title-size: calc(var(--font-size-base, 14px) + 4px);
    --settings-heading-size: var(--font-size-cardTitle, 13px);
    --settings-description-size: var(--font-size-secondary, 11px);
    --settings-note-size: var(--font-size-tiny, 10px);
    --settings-control-size: var(--font-size-secondary, 11px);
    --settings-card-radius: 9px;
    --settings-control-radius: 6px;
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
  .setting-heading {
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
    font-size: var(--font-size-base, 14px);
  }
  .settings-brand small {
    margin-top: 2px;
    color: #6f6f6f;
    font-size: var(--font-size-secondary, 11px);
  }

  .settings-primary-nav {
    display: grid;
    gap: 4px;
  }

  .settings-primary-nav button {
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
    font-size: var(--settings-control-size);
    text-align: left;
    cursor: pointer;
    transition:
      background 100ms ease,
      color 100ms ease,
      border-color 100ms ease;
  }

  .settings-primary-nav button:hover {
    color: #ccc;
    background: #252525;
    border-color: #3a3a3a;
  }

  .settings-primary-nav button.active {
    border-color: #5a5a5a;
    color: #f0f0f0;
    background: #333;
  }

  .settings-primary-nav button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .sidebar-foot {
    display: grid;
    gap: 5px;
    margin-top: auto;
    padding: 10px 6px 0;
    color: #606060;
    font-size: var(--font-size-secondary, 11px);
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

  .settings-subnav {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 10px 18px 0;
    background: #191919;
  }

  .settings-subnav button {
    min-height: 28px;
    padding: 5px 12px;
    border: 1px solid transparent;
    border-radius: var(--settings-control-radius);
    color: #888;
    background: transparent;
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
    transition:
      color 100ms ease,
      background 100ms ease,
      border-color 100ms ease;
  }

  .settings-subnav button:hover {
    border-color: #343434;
    color: #cfcfcf;
    background: #222;
  }

  .settings-subnav button.active {
    border-color: #3d5a80;
    color: #e8e8e8;
    background: #252f3d;
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
    font-size: var(--settings-note-size);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 {
    margin: 5px 0 4px;
    color: #efefef;
    font-size: var(--settings-page-title-size);
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
    font-size: var(--settings-description-size);
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
    border-radius: var(--settings-card-radius);
    background: #1e1e1e;
  }

  .setting-heading {
    gap: 10px;
  }

  .setting-icon {
    width: 29px;
    height: 29px;
    border-radius: 7px;
  }

  .setting-heading strong {
    display: block;
    color: #dedede;
    font-size: var(--settings-heading-size);
    font-weight: 560;
  }

  .setting-heading p {
    margin-top: 2px;
    font-size: var(--settings-description-size);
  }

  .pending-path code {
    display: block;
    overflow: hidden;
    color: #a7a7a7;
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .inline-badge {
    display: inline-block;
    margin-left: 8px;
    padding: 2px 7px;
    border: 1px solid #393939;
    border-radius: 999px;
    color: #888;
    font-size: var(--font-size-tiny, 10px);
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
    font-size: var(--font-size-secondary, 11px);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    border: 1px solid #3a3a3a;
    border-radius: var(--settings-control-radius);
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
    padding: 8px 30px 8px 12px;
    border: 1px solid #3a3a3a;
    border-radius: var(--settings-control-radius);
    outline: none;
    color: #d7d7d7;
    background: #1a1a1a;
    font-size: var(--settings-control-size);
    cursor: pointer;
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10' viewBox='0 0 10 10'%3E%3Cpath fill='%23999' d='M2 3l3 4 3-4'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 10px center;
    transition:
      border-color 120ms ease,
      background-color 120ms ease;
  }

  select:hover,
  .model-select:hover {
    border-color: #555;
  }

  select:focus,
  .model-select:focus,
  .unit-select:focus {
    border-color: #555;
  }

  select option,
  .model-select option,
  .unit-select option {
    background: #1e1e1e;
    color: #d7d7d7;
    padding: 6px 10px;
    font-size: var(--font-size-secondary, 11px);
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
    font-size: var(--font-size-secondary, 11px);
  }

  .pending-path code {
    font-size: var(--font-size-secondary, 11px);
  }

  .directory-tree-card pre {
    margin: 11px 0 0;
    padding: 10px 12px;
    border: 1px solid #2e2e2e;
    border-radius: 7px;
    color: #999;
    background: #181818;
    font:
      11px/1.55 "Cascadia Code",
      "SFMono-Regular",
      Consolas,
      monospace;
  }

  .stats-rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
  }

  .stat-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    border: 1px solid #2b2b2b;
    border-radius: 6px;
    background: #161616;
    font-size: var(--font-size-secondary, 11px);
  }

  .stat-row span:first-child {
    color: #999;
  }

  .stat-row span:last-child {
    color: #d8d8d8;
    font-weight: 560;
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
    font-size: var(--font-size-secondary, 11px);
  }

  .number-suffix {
    color: #888;
    font-size: var(--font-size-secondary, 11px);
    flex-shrink: 0;
  }

  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: #777;
    font-size: var(--font-size-secondary, 11px);
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
    font-size: var(--settings-description-size);
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

  .repair-result {
    margin-top: 10px;
    padding: 8px 9px;
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    background: #181818;
    font-size: var(--font-size-secondary, 11px);
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
    font-size: var(--font-size-tiny, 10px);
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: #666;
    font-size: var(--font-size-secondary, 11px);
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
    font-size: var(--font-size-secondary, 11px);
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
    text-align: right;
    outline: none;
    transition: border-color 120ms ease;
    appearance: textfield;
    -moz-appearance: textfield;
  }

  .setting-card-row input::-webkit-outer-spin-button,
  .setting-card-row input::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  .setting-card-row button {
    height: 34px;
    box-sizing: border-box;
    padding: 5px 12px;
    border: 1px solid #3a3a3a;
    border-radius: var(--settings-control-radius);
    color: #a3a3a3;
    background: #252525;
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .setting-card-row select,
  .setting-card-row .model-select {
    height: 34px;
    box-sizing: border-box;
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
    font-size: var(--font-size-secondary, 11px);
    flex-shrink: 0;
  }

  .config-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: #777;
    font-size: var(--font-size-tiny, 10px);
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
    font-size: var(--font-size-tiny, 10px);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
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
    font:
      12px "Cascadia Code",
      Consolas,
      monospace;
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
    font-size: var(--font-size-secondary, 11px);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
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
    font:
      10.5px "Cascadia Code",
      Consolas,
      monospace;
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
    font-size: var(--font-size-secondary, 11px);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
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
    width: 64px;
    padding: 8px 6px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #d7d7d7;
    background: #1a1a1a;
    font-size: var(--font-size-secondary, 11px);
    cursor: pointer;
    flex-shrink: 0;
    text-align: center;
    text-align-last: center;
    outline: none;
    transition:
      border-color 120ms ease,
      background-color 120ms ease;
  }

  .unit-select:hover {
    border-color: #555;
  }

  .restart-btn {
    margin-left: auto;
    padding: 5px 12px;
    border: 1px solid #4a4a4a;
    border-radius: 6px;
    color: #d8d8d8;
    background: #2a2a2a;
    font-size: var(--font-size-secondary, 11px);
    cursor: pointer;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .restart-btn:hover {
    color: #fff;
    background: #383838;
    border-color: #5a5a5a;
  }
</style>
