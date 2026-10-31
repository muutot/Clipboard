<script lang="ts">
  import { untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    getPerformanceMetrics,
    type PerformanceMetrics,
    type StorageStatus,
  } from "$lib/services/storage";
  import { getMemoryDiagnostics } from "$lib/services/memory";
  import type { MemoryDiagnostics, OcrMemoryDiagnostics } from "$lib/types/memory";
  import type { StatisticsTab } from "$lib/settings-navigation";
  import EntryHeading from "./settings-entries/EntryHeading.svelte";
  import { formatBytes } from "$lib/utils/content/format";

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

  /**
   * The OCR model card must never present an engine that is not in effect.
   * A configured PP-OCR variant without its model files cannot run, so the
   * card reports "no engine selected" instead of the configured engine id.
   */
  function ocrModelState(ocr: OcrMemoryDiagnostics): string {
    if (ocr.engine !== "ppocr") {
      return `${ocr.engine} · ${_t("storage.ocrExternalEngine")}`;
    }
    if (!ocr.modelInstalled) {
      return `${_t("storage.ocrEngineNotSelected")} · ${_t("storage.ocrModelNotInstalled")}`;
    }
    return `${ocr.engine} / ${ocr.modelVariant} · ${_t("storage.ocrModelInstalledState")}`;
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

    void untrack(() => refreshVisibleStatistics?.());
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
    void untrack(() => loadMemoryDiagnostics());
    const interval = setInterval(() => void loadMemoryDiagnostics(), 3000);
    return () => clearInterval(interval);
  });
</script>

<div class="settings-scroll stats-scroll">
  {#if activeTab === "storage"}
    {#if status}
      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="clipboard"
          label={_t("statistics.totalRecords")}
          desc={_t("storage.totalRecordsDesc")}
        />
        <span class="stats-metric-value">{status.itemCount}</span>
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="text"
          label={_t("statistics.text")}
          desc={_t("storage.textDesc")}
        />
        <span class="stats-metric-value">{status.textCount}</span>
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="link"
          label={_t("statistics.link")}
          desc={_t("storage.linkDesc")}
        />
        <span class="stats-metric-value">{status.linkCount}</span>
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="image"
          label={_t("statistics.image")}
          desc={_t("storage.imageDesc")}
        />
        <span class="stats-metric-value"
          >{status.imageCount}{_t("storage.imageCountUnit")} · {formatBytes(
            status.imageSizeBytes,
          )}</span
        >
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="file"
          label={_t("statistics.file")}
          desc={_t("storage.fileDesc")}
        />
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
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="file"
          label={_t("statistics.database")}
          desc={_t("storage.dbDesc")}
        />
        <span class="stats-metric-value">{formatBytes(status.databaseSizeBytes)}</span>
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="search"
          label={_t("statistics.indexSize")}
          desc={_t("storage.searchIndexDesc")}
        />
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
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="clock"
          label={_t("storage.startupTime")}
          desc={_t("storage.startupTimeDesc")}
        />
        <span class="stats-metric-value">{perfMetrics.startup.totalStartupMs}ms</span>
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="file"
          label={_t("storage.dbOpenTime")}
          desc={_t("storage.dbOpenTimeDesc")}
        />
        <span class="stats-metric-value">{perfMetrics.startup.dbOpenMs}ms</span>
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="search"
          label={_t("storage.searchInitTime")}
          desc={_t("storage.searchInitTimeDesc")}
        />
        <span class="stats-metric-value">{perfMetrics.startup.searchInitMs}ms</span>
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="clock"
          label={_t("storage.uptime")}
          desc={_t("storage.uptimeDesc")}
        />
        <span class="stats-metric-value">{perfMetrics.memory.uptimeSeconds}s</span>
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="bar-chart"
          label={_t("storage.memoryPeak")}
          desc={_t("storage.memoryPeakDesc")}
        />
        <span class="stats-metric-value"
          >{Math.round(perfMetrics.memory.peakBytes / 1048576)} MB</span
        >
      </section>

      <section class="setting-card stats-metric-card">
        <div class="setting-heading stats-metric-heading">
          <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
          <div class="stats-metric-copy">
            <strong>{_t("storage.searchInputTime")}</strong>
            <p>{_t("storage.searchInputTimeDesc")}</p>
            <p>
              {_t("storage.searchInputSamples", {
                count: perfMetrics.searchInputLatency.searchesRecorded,
              })}
            </p>
          </div>
        </div>
        <span class="stats-metric-value"
          >{perfMetrics.searchInputLatency.averageMs?.toFixed(1) ?? "-"}ms</span
        >
      </section>
      {#each [{ label: "P95", value: perfMetrics.searchInputLatency.p95Ms }, { label: "P99", value: perfMetrics.searchInputLatency.p99Ms }] as metric}
        <section class="setting-card stats-metric-card">
          <div class="setting-heading stats-metric-heading">
            <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
            <strong>{_t("storage.searchInputTime")} · {metric.label}</strong>
          </div>
          <span class="stats-metric-value">{metric.value ?? "-"}ms</span>
        </section>
      {/each}

      {#if perfMetrics.searchLatency.searchesRecorded > 0}
        <section class="setting-card stats-metric-card">
          <EntryHeading
            class="stats-metric-heading"
            textClass="stats-metric-copy"
            icon="search"
            label={_t("storage.searchCount")}
            desc={_t("storage.searchCountDesc")}
          />
          <span class="stats-metric-value">{perfMetrics.searchLatency.searchesRecorded}</span>
        </section>

        <section class="setting-card stats-metric-card">
          <EntryHeading
            class="stats-metric-heading"
            textClass="stats-metric-copy"
            icon="clock"
            label={_t("storage.searchAvgTime")}
            desc={_t("storage.searchAvgTimeDesc")}
          />
          <span class="stats-metric-value"
            >{perfMetrics.searchLatency.averageMs?.toFixed(1) ?? "-"}ms</span
          >
        </section>

        <section class="setting-card stats-metric-card">
          <EntryHeading
            class="stats-metric-heading"
            textClass="stats-metric-copy"
            icon="clock"
            label={_t("storage.searchP95Time")}
            desc={_t("storage.searchP95TimeDesc")}
          />
          <span class="stats-metric-value">{perfMetrics.searchLatency.p95Ms ?? "-"}ms</span>
        </section>

        <section class="setting-card stats-metric-card">
          <EntryHeading
            class="stats-metric-heading"
            textClass="stats-metric-copy"
            icon="clock"
            label={_t("storage.searchP99Time")}
            desc={_t("storage.searchP99TimeDesc")}
          />
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

      <section
        class="setting-card stats-metric-card"
        data-settings-search-id="statistics.memory.process-group-private-working-set"
      >
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="grid"
          label={memoryDiagnostics.processGroup.privateWorkingSetBytes == null
            ? _t("storage.processGroupWorkingSet")
            : _t("storage.processGroupPrivateWorkingSet")}
          desc={memoryDiagnostics.processGroup.privateWorkingSetBytes == null
            ? _t("storage.processGroupWorkingSetDesc")
            : _t("storage.processGroupPrivateWorkingSetDesc")}
        />
        <span class="stats-metric-value">
          {formatMaybeBytes(
            memoryDiagnostics.processGroup.privateWorkingSetBytes ??
              memoryDiagnostics.processGroup.workingSetBytes,
          )}
        </span>
      </section>

      {#if memoryDiagnostics.processGroup.privateWorkingSetBytes != null}
        <section
          class="setting-card stats-metric-card"
          data-settings-search-id="statistics.memory.process-group-working-set"
        >
          <EntryHeading
            class="stats-metric-heading"
            textClass="stats-metric-copy"
            icon="grid"
            label={_t("storage.processGroupWorkingSet")}
            desc={_t("storage.processGroupWorkingSetDesc")}
          />
          <span class="stats-metric-value"
            >{formatBytes(memoryDiagnostics.processGroup.workingSetBytes)}</span
          >
        </section>
      {/if}

      <section
        class="setting-card stats-metric-card"
        data-settings-search-id="statistics.memory.current-working-set"
      >
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="bar-chart"
          label={_t("storage.processWorkingSet")}
          desc={_t("storage.processWorkingSetDesc")}
        />
        <span class="stats-metric-value"
          >{formatMaybeBytes(memoryDiagnostics.currentProcess.workingSetBytes)}</span
        >
      </section>

      <section
        class="setting-card stats-metric-card"
        data-settings-search-id="statistics.memory.current-private-bytes"
      >
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="bar-chart"
          label={_t("storage.processPrivateMem")}
          desc={_t("storage.processPrivateMemDesc")}
        />
        <span class="stats-metric-value"
          >{formatMaybeBytes(memoryDiagnostics.currentProcess.privateBytes)}</span
        >
      </section>

      <section class="setting-card stats-metric-card">
        <EntryHeading
          class="stats-metric-heading"
          textClass="stats-metric-copy"
          icon="grid"
          label={_t("storage.systemAvailableMemory")}
          desc={_t("storage.systemAvailableMemoryDesc")}
        />
        <span class="stats-metric-value"
          >{formatMaybeBytes(memoryDiagnostics.system.availableBytes)} / {formatMaybeBytes(
            memoryDiagnostics.system.totalBytes,
          )}</span
        >
      </section>

      {#if browserMemory}
        <section class="setting-card stats-metric-card">
          <EntryHeading
            class="stats-metric-heading"
            textClass="stats-metric-copy"
            icon="code"
            label={_t("storage.jsHeapTitle")}
            desc={_t("storage.jsHeapDesc")}
          />
          <span class="stats-metric-value"
            >{formatBytes(browserMemory.usedBytes)}{browserMemory.limitBytes
              ? ` / ${formatBytes(browserMemory.limitBytes)}`
              : ""}</span
          >
        </section>
      {/if}

      <section class="setting-card memory-process-card">
        <EntryHeading
          icon="settings"
          label={_t("storage.processDetail")}
          desc={_t("storage.processDetailDesc")}
        />
        <div class="memory-process-list">
          <div class="memory-process-row memory-process-header">
            <span>{_t("storage.processColumnProcess")}</span>
            <span>PID</span>
            <span>{_t("storage.processColumnPrivateWorkingSet")}</span>
            <span>{_t("storage.processColumnWorkingSet")}</span>
          </div>
          {#each memoryDiagnostics.processGroup.processes as process (process.pid)}
            <div class="memory-process-row">
              <span class="memory-process-name">{process.role || process.name}</span>
              <span class="memory-process-pid">PID {process.pid}</span>
              <span class="memory-process-private"
                >{formatMaybeBytes(process.privateWorkingSetBytes)}</span
              >
              <span class="memory-process-working-set"
                >{formatMaybeBytes(process.workingSetBytes)}</span
              >
            </div>
          {/each}
        </div>
      </section>

      {#if memoryDiagnostics.ocr}
        <section
          class="setting-card stats-metric-card"
          data-settings-search-id="statistics.memory.ocr-model"
        >
          <EntryHeading
            class="stats-metric-heading"
            textClass="stats-metric-copy"
            icon="eye"
            label={_t("storage.ocrModelLabel")}
            desc={ocrModelState(memoryDiagnostics.ocr)}
          />
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

  /* The heading wrapper and copy block render inside EntryHeading, so the
     panel classes need :global() to reach them. */
  .stats-metric-card :global(.stats-metric-copy) {
    min-width: 0;
  }

  .stats-metric-card :global(.stats-metric-copy strong),
  .stats-metric-card :global(.stats-metric-copy p) {
    overflow-wrap: anywhere;
  }

  .stats-empty-state {
    min-height: 140px;
  }

  .stats-metric-value {
    flex: 0 0 auto;
    color: var(--text-secondary);
    font-size: var(--settings-control-size);
    font-variant-numeric: tabular-nums;
    font-weight: 560;
    line-height: 1.45;
    text-align: right;
    white-space: nowrap;
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
    background: transparent;
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
    grid-template-columns: minmax(0, 1fr) auto minmax(62px, auto) minmax(76px, auto);
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .memory-process-header {
    align-items: end;
    color: var(--text-muted);
    font-size: var(--settings-note-size);
    line-height: 1.3;
  }

  .memory-process-header span:not(:first-child) {
    text-align: right;
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

  .memory-process-private,
  .memory-process-working-set {
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }

  .memory-process-private {
    color: var(--text-primary);
    font-weight: 560;
  }

  .memory-process-working-set {
    color: var(--text-muted);
  }
</style>
