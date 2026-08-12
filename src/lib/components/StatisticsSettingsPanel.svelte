<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    getPerformanceMetrics,
    type PerformanceMetrics,
    type StorageStatus,
  } from "$lib/services/storage";
  import { getMemoryDiagnostics } from "$lib/services/memory";
  import type { MemoryDiagnostics } from "$lib/types/memory";
  import type { StatisticsTab } from "$lib/settings-navigation";
  import { formatBytes } from "$lib/utils/format";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    activeTab: StatisticsTab;
    status: StorageStatus | null;
    loading: boolean;
    onrefreshStatus: () => Promise<void>;
    onclose: () => void;
  }

  let { activeTab, status, loading, onrefreshStatus, onclose }: Props = $props();

  let perfMetrics = $state<PerformanceMetrics | null>(null);
  let memoryDiagnostics = $state<MemoryDiagnostics | null>(null);
  let memoryLoading = $state(false);
  let memoryError = $state("");

  type BrowserMemorySnapshot = {
    usedBytes: number;
    totalBytes: number;
    limitBytes: number;
  };
  let browserMemory = $state<BrowserMemorySnapshot | null>(null);

  function formatMaybeBytes(bytes: number | null | undefined): string {
    return bytes == null ? "—" : formatBytes(bytes);
  }

  async function loadPerformanceMetrics() {
    try {
      perfMetrics = await getPerformanceMetrics();
    } catch {
      perfMetrics = null;
    }
  }

  function readBrowserMemory(): BrowserMemorySnapshot | null {
    if (typeof performance === "undefined") return null;
    const candidate = performance as Performance & {
      memory?: {
        usedJSHeapSize?: number;
        totalJSHeapSize?: number;
        jsHeapSizeLimit?: number;
      };
    };
    const memory = candidate.memory;
    if (!memory || typeof memory.usedJSHeapSize !== "number") return null;
    return {
      usedBytes: memory.usedJSHeapSize,
      totalBytes: memory.totalJSHeapSize ?? 0,
      limitBytes: memory.jsHeapSizeLimit ?? 0,
    };
  }

  async function loadMemoryDiagnostics() {
    if (memoryLoading) return;
    memoryLoading = true;
    memoryError = "";
    browserMemory = readBrowserMemory();
    try {
      memoryDiagnostics = await getMemoryDiagnostics();
    } catch (error) {
      memoryDiagnostics = null;
      memoryError = error instanceof Error ? error.message : String(error);
    } finally {
      memoryLoading = false;
    }
  }

  $effect(() => {
    let refreshVisibleStatistics: (() => Promise<void>) | undefined;
    if (activeTab === "storage") {
      refreshVisibleStatistics = onrefreshStatus;
    } else if (activeTab === "performance") {
      refreshVisibleStatistics = loadPerformanceMetrics;
    }
    if (!refreshVisibleStatistics) return;

    let disposed = false;
    let refreshTimer: ReturnType<typeof setTimeout> | undefined;
    let unlistenAdd: (() => void) | undefined;
    let unlistenInvalidated: (() => void) | undefined;
    const scheduleRefresh = () => {
      if (refreshTimer !== undefined) clearTimeout(refreshTimer);
      refreshTimer = setTimeout(() => {
        refreshTimer = undefined;
        if (!disposed) void refreshVisibleStatistics();
      }, 250);
    };

    void refreshVisibleStatistics();
    listen("clipboard-item-added", scheduleRefresh).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenAdd = unlisten;
    });
    listen("clipboard-history-invalidated", scheduleRefresh).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenInvalidated = unlisten;
    });
    return () => {
      disposed = true;
      if (refreshTimer !== undefined) clearTimeout(refreshTimer);
      unlistenAdd?.();
      unlistenInvalidated?.();
    };
  });

  $effect(() => {
    if (activeTab !== "memory") return;
    void loadMemoryDiagnostics();
    const interval = setInterval(() => void loadMemoryDiagnostics(), 3000);
    return () => clearInterval(interval);
  });
</script>

<div class="settings-scroll stats-scroll">
  {#if activeTab === "storage"}
    {#if status}
      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="clipboard" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("statistics.totalRecords")}</strong>
            <p>{_t("storage.totalRecordsDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{status.itemCount}</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="text" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("statistics.text")}</strong>
            <p>{_t("storage.textDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{status.textCount}</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="link" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("statistics.link")}</strong>
            <p>{_t("storage.linkDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{status.linkCount}</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="image" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("statistics.image")}</strong>
            <p>{_t("storage.imageDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value"
          >{status.imageCount}{_t("storage.imageCountUnit")} · {formatBytes(
            status.imageSizeBytes,
          )}</span
        >
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="file" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("statistics.file")}</strong>
            <p>{_t("storage.fileDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value"
          >{status.fileCount}{_t("storage.fileCountUnit")} · {formatBytes(
            status.fileSizeBytes,
          )}</span
        >
      </section>

      <section
        class="setting-card stats-metric-card"
        data-settings-search-id="statistics.storage.database"
      >
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="file" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("statistics.database")}</strong>
            <p>{_t("storage.dbDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{formatBytes(status.databaseSizeBytes)}</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="search" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("statistics.indexSize")}</strong>
            <p>{_t("storage.searchIndexDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{formatBytes(status.searchIndexSizeBytes)}</span>
      </section>
    {:else}
      <div class="settings-state stats-empty-state">
        {loading ? _t("storage.statsLoading") : _t("storage.statsUnavailable")}
      </div>
    {/if}
    <p class="auto-save-note">{_t("storage.statsNote")}</p>
  {:else if activeTab === "performance"}
    {#if perfMetrics}
      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.startupTime")}</strong>
            <p>{_t("storage.startupTimeDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{perfMetrics.startup.totalStartupMs}ms</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="file" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.dbOpenTime")}</strong>
            <p>{_t("storage.dbOpenTimeDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{perfMetrics.startup.dbOpenMs}ms</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="search" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.searchInitTime")}</strong>
            <p>{_t("storage.searchInitTimeDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{perfMetrics.startup.searchInitMs}ms</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.dbMigrateTime")}</strong>
            <p>{_t("storage.dbMigrateTimeDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{perfMetrics.startup.migrationsMs}ms</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.uptime")}</strong>
            <p>{_t("storage.uptimeDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value">{perfMetrics.memory.uptimeSeconds}s</span>
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="bar-chart" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.memoryPeak")}</strong>
            <p>{_t("storage.memoryPeakDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value"
          >{Math.round(perfMetrics.memory.peakBytes / 1048576)} MB</span
        >
      </section>

      {#if perfMetrics.searchLatency.searchesRecorded > 0}
        <section class="setting-card stats-metric-card">
          <div class="setting-heading stats-metric-heading">
            <span class="setting-icon"><AppIcon name="search" size={17} /></span>
            <div class="stats-metric-copy">
              <strong>{_t("storage.searchCount")}</strong>
              <p>{_t("storage.searchCountDesc")}</p>
            </div>
          </div>
          <span class="stats-metric-value">{perfMetrics.searchLatency.searchesRecorded}</span>
        </section>

        <section class="setting-card stats-metric-card">
          <div class="setting-heading stats-metric-heading">
            <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
            <div class="stats-metric-copy">
              <strong>{_t("storage.searchAvgTime")}</strong>
              <p>{_t("storage.searchAvgTimeDesc")}</p>
            </div>
          </div>
          <span class="stats-metric-value"
            >{perfMetrics.searchLatency.averageMs?.toFixed(1) ?? "-"}ms</span
          >
        </section>

        <section class="setting-card stats-metric-card">
          <div class="setting-heading stats-metric-heading">
            <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
            <div class="stats-metric-copy">
              <strong>{_t("storage.searchP95Time")}</strong>
              <p>{_t("storage.searchP95TimeDesc")}</p>
            </div>
          </div>
          <span class="stats-metric-value">{perfMetrics.searchLatency.p95Ms ?? "-"}ms</span>
        </section>

        <section class="setting-card stats-metric-card">
          <div class="setting-heading stats-metric-heading">
            <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
            <div class="stats-metric-copy">
              <strong>{_t("storage.searchP99Time")}</strong>
              <p>{_t("storage.searchP99TimeDesc")}</p>
            </div>
          </div>
          <span class="stats-metric-value">{perfMetrics.searchLatency.p99Ms ?? "-"}ms</span>
        </section>
      {/if}
    {:else}
      <div class="settings-state stats-empty-state">{_t("storage.perfUnavailable")}</div>
    {/if}
    <p class="auto-save-note">{_t("storage.perfNote")}</p>
  {:else}
    {#if memoryDiagnostics}
      <div class="memory-toolbar">
        <span class="memory-sampled-at"
          >{_t("storage.sampleTime")}{new Date(
            memoryDiagnostics.sampledAtMs,
          ).toLocaleTimeString()}</span
        >
        <button type="button" class="memory-refresh" onclick={() => void loadMemoryDiagnostics()}>
          {memoryLoading ? _t("storage.reading") : _t("storage.refreshNow")}
        </button>
      </div>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="bar-chart" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.processWorkingSet")}</strong>
            <p>{_t("storage.processWorkingSetDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value"
          >{formatMaybeBytes(memoryDiagnostics.currentProcess.workingSetBytes)}</span
        >
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="bar-chart" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.processPrivateMem")}</strong>
            <p>{_t("storage.processPrivateMemDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value"
          >{formatMaybeBytes(memoryDiagnostics.currentProcess.privateBytes)}</span
        >
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.processGroupWorkingSet")}</strong>
            <p>{_t("storage.processGroupWorkingSetDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value"
          >{formatBytes(memoryDiagnostics.processGroup.workingSetBytes)}</span
        >
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.systemAvailableMemory")}</strong>
            <p>{_t("storage.systemAvailableMemoryDesc")}</p>
          </div>
        </div>
        <span class="stats-metric-value"
          >{formatMaybeBytes(memoryDiagnostics.system.availableBytes)} / {formatMaybeBytes(
            memoryDiagnostics.system.totalBytes,
          )}</span
        >
      </section>

      {#if browserMemory}
        <section class="setting-card stats-metric-card">
          <div class="setting-heading stats-metric-heading">
            <span class="setting-icon"><AppIcon name="code" size={17} /></span>
            <div class="stats-metric-copy">
              <strong>{_t("storage.jsHeapTitle")}</strong>
              <p>{_t("storage.jsHeapDesc")}</p>
            </div>
          </div>
          <span class="stats-metric-value"
            >{formatBytes(browserMemory.usedBytes)}{browserMemory.limitBytes
              ? ` / ${formatBytes(browserMemory.limitBytes)}`
              : ""}</span
          >
        </section>
      {/if}

      <section class="setting-card memory-process-card">
        <div class="setting-heading">
          <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
          <div>
            <strong>{_t("storage.processDetail")}</strong>
            <p>{_t("storage.processDetailDesc")}</p>
          </div>
        </div>
        <div class="memory-process-list">
          {#each memoryDiagnostics.processGroup.processes as process (process.pid)}
            <div class="memory-process-row">
              <span class="memory-process-name">{process.role || process.name}</span>
              <span class="memory-process-pid">PID {process.pid}</span>
              <span class="memory-process-size">{formatMaybeBytes(process.workingSetBytes)}</span>
            </div>
          {/each}
        </div>
      </section>

      {#if memoryDiagnostics.ocr}
        <section
          class="setting-card stats-metric-card"
          data-settings-search-id="statistics.memory.ocr-model"
        >
          <div class="setting-heading stats-metric-heading">
            <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
            <div class="stats-metric-copy">
              <strong>{_t("storage.ocrModelLabel")}</strong>
              <p>
                {memoryDiagnostics.ocr.engine} / {memoryDiagnostics.ocr.modelVariant}
                {memoryDiagnostics.ocr.loaded
                  ? ` · ${_t("statistics.ocrEngineAvailable")}`
                  : ` · ${_t("storage.ocrModelNotInstalled")}`}
              </p>
            </div>
          </div>
          <span class="stats-metric-value"
            >{formatBytes(memoryDiagnostics.ocr.modelBytes)} · {memoryDiagnostics.ocr
              .modelFileCount}
            {_t("storage.ocrModelFileCount")}</span
          >
        </section>
      {/if}
    {:else}
      <div class="settings-state stats-empty-state">
        {#if memoryError}
          {_t("storage.memoryDiagUnavailable")}{memoryError}
        {:else}
          {memoryLoading ? _t("storage.memoryDiagLoading") : _t("storage.memoryDiagEmpty")}
        {/if}
      </div>
    {/if}
    <p class="auto-save-note">{_t("storage.memoryNote")}</p>
  {/if}
</div>

<style>
  .stats-scroll {
    align-content: start;
  }

  .stats-metric-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }

  .stats-metric-heading {
    min-width: 0;
    flex: 1 1 auto;
  }

  .stats-metric-copy {
    min-width: 0;
  }

  .stats-metric-copy strong,
  .stats-metric-copy p {
    overflow-wrap: anywhere;
  }

  .stats-empty-state {
    min-height: 140px;
  }

  .stats-metric-value {
    min-width: 0;
    max-width: 42%;
    flex: 0 1 auto;
    color: var(--text-secondary);
    font-size: var(--settings-control-size);
    font-variant-numeric: tabular-nums;
    font-weight: 560;
    line-height: 1.45;
    text-align: right;
    overflow-wrap: anywhere;
  }

  .memory-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 28px;
    color: var(--text-muted);
    font-size: var(--settings-note-size);
  }

  .memory-sampled-at {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .memory-refresh {
    flex: 0 0 auto;
    padding: 5px 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
  }

  .memory-refresh:hover {
    border-color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 15%, var(--hover-bg));
  }

  .memory-process-card {
    min-width: 0;
  }

  .memory-process-list {
    display: grid;
    gap: 5px;
    margin-top: 10px;
    padding-top: 9px;
    border-top: 1px solid var(--border-subtle);
  }

  .memory-process-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .memory-process-name {
    min-width: 0;
    overflow: hidden;
    color: var(--text-secondary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .memory-process-pid {
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .memory-process-size {
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
</style>
