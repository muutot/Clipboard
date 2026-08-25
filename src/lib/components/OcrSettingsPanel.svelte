<script lang="ts">
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import SelectEntry from "$lib/components/settings-entries/SelectEntry.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { updateSliderTrack } from "$lib/utils/format";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onfeedback: (message: string, success: boolean) => void;
  }

  let { onfeedback }: Props = $props();

  let ocrEngine = $state("ppocr");
  let ocrEngineAvailable = $state(false);
  let ocrHasEngine = $state(false);
  let ocrStatusLoading = $state(false);
  let ocrTotal = $state(0);
  let ocrPending = $state(0);
  let ocrCompleted = $state(0);
  let ocrFailed = $state(0);
  let installedVariants = $state<string[]>([]);
  let activeVariant = $state<string>("");
  let ocrInstalling = $state(false);
  let ocrProgressLabel = $state("");
  let ocrProgressPct = $state(-1);
  let ocrProgressCurrent = $state(0);
  let ocrProgressTotal = $state(0);
  let modelVariant = $state("small");
  let ocrDownloadUnlisten: (() => void) | undefined;
  let ocrInstallRequestId = 0;
  let destroyed = false;
  let detScoreThreshold = $state(0.3);
  let detBoxThreshold = $state(0.6);
  let detUnclipRatio = $state(1.5);
  let detScoreSlider = $state<HTMLInputElement | null>(null);
  let detBoxSlider = $state<HTMLInputElement | null>(null);
  let detUnclipSlider = $state<HTMLInputElement | null>(null);

  interface OcrStatusResult {
    totalTasks: number;
    pendingTasks: number;
    completedTasks: number;
    failedTasks: number;
    engine: string;
    engineAvailable: boolean;
    hasEngine: boolean;
    ppocrModelVariant: string;
    installedVariants: string[];
  }

  interface OcrConfigResult {
    engine: string;
    ppocrModelVariant: string;
    detScoreThreshold: number;
    detBoxThreshold: number;
    detUnclipRatio: number;
  }

  function releaseOcrDownloadListener(): void {
    if (!ocrDownloadUnlisten) return;
    ocrDownloadUnlisten();
    ocrDownloadUnlisten = undefined;
  }

  onDestroy(() => {
    destroyed = true;
    ocrInstallRequestId += 1;
    releaseOcrDownloadListener();
  });

  $effect(() => {
    detScoreThreshold;
    detBoxThreshold;
    detUnclipRatio;
    updateSliderTrack(detScoreSlider);
    updateSliderTrack(detBoxSlider);
    updateSliderTrack(detUnclipSlider);
  });

  $effect(() => {
    void loadOcrStatus();
    const interval = setInterval(() => void loadOcrTaskStatus(), 2000);
    return () => clearInterval(interval);
  });

  function applyOcrTaskStatus(result: OcrStatusResult): void {
    ocrTotal = result.totalTasks;
    ocrPending = result.pendingTasks;
    ocrCompleted = result.completedTasks;
    ocrFailed = result.failedTasks;
  }

  async function loadOcrTaskStatus() {
    if (ocrStatusLoading) return;
    ocrStatusLoading = true;
    try {
      applyOcrTaskStatus(await invoke<OcrStatusResult>("get_ocr_status"));
    } catch {
      /* ignore */
    } finally {
      ocrStatusLoading = false;
    }
  }

  async function loadOcrStatus() {
    if (ocrStatusLoading) return;
    ocrStatusLoading = true;
    try {
      const [statusResult, configResult] = await Promise.allSettled([
        invoke<OcrStatusResult>("get_ocr_status"),
        invoke<OcrConfigResult>("get_ocr_config"),
      ]);
      if (statusResult.status === "fulfilled") {
        const result = statusResult.value;
        applyOcrTaskStatus(result);
        ocrEngine = result.engine;
        ocrEngineAvailable = result.engineAvailable;
        ocrHasEngine = result.hasEngine;
        installedVariants = result.installedVariants;
        activeVariant = result.ppocrModelVariant;
        if (!modelVariant) modelVariant = result.ppocrModelVariant;
      } else {
        installedVariants = [];
      }
      if (configResult.status === "fulfilled") {
        const cfg = configResult.value;
        ocrEngine = cfg.engine;
        detScoreThreshold = cfg.detScoreThreshold;
        detBoxThreshold = cfg.detBoxThreshold;
        detUnclipRatio = cfg.detUnclipRatio;
        if (cfg.ppocrModelVariant) {
          activeVariant = cfg.ppocrModelVariant;
          if (!modelVariant) modelVariant = cfg.ppocrModelVariant;
        }
      }
    } finally {
      ocrStatusLoading = false;
    }
  }

  async function installPpocr() {
    const requestId = ++ocrInstallRequestId;
    releaseOcrDownloadListener();
    ocrInstalling = true;
    ocrProgressPct = -1;
    ocrProgressLabel = "";
    ocrProgressCurrent = 0;
    ocrProgressTotal = 0;
    try {
      const unlisten = await listen<{
        filename: string;
        label: string;
        current: number;
        total: number;
        percentage: number;
      }>("ppocr-download-progress", (event) => {
        if (destroyed || requestId !== ocrInstallRequestId) return;
        ocrProgressLabel = event.payload.label;
        ocrProgressPct = event.payload.percentage;
        ocrProgressCurrent = event.payload.current;
        ocrProgressTotal = event.payload.total;
      });
      if (destroyed || requestId !== ocrInstallRequestId) {
        unlisten();
        return;
      }
      ocrDownloadUnlisten = unlisten;
      await invoke<string>("install_ppocr", { variant: modelVariant });
      if (destroyed || requestId !== ocrInstallRequestId) return;
      onfeedback(_t("storage.ocrModelInstalled", { variant: modelVariant }), true);
      await loadOcrStatus();
    } catch (e) {
      if (!destroyed && requestId === ocrInstallRequestId) {
        onfeedback(_t("storage.ocrModelInstallFailed", { error: String(e) }), false);
      }
    } finally {
      if (requestId === ocrInstallRequestId) {
        releaseOcrDownloadListener();
        if (!destroyed) {
          ocrInstalling = false;
          ocrProgressPct = -1;
        }
      }
    }
  }

  async function applyModel() {
    if (activeVariant === modelVariant) {
      onfeedback(_t("storage.ocrModelAlreadyApplied"), true);
      return;
    }
    try {
      await invoke("set_ocr_config", {
        settings: {
          engine: "ppocr",
          ppocrModelVariant: modelVariant,
        },
      });
      await loadOcrStatus();
      ocrEngine = "ppocr";
      onfeedback(_t("storage.ocrModelApplied"), true);
    } catch (e) {
      await loadOcrStatus();
      onfeedback(_t("storage.ocrModelApplyFailed", { error: String(e) }), false);
    }
  }

  async function saveOcrEngine(engine: string) {
    try {
      await invoke("set_ocr_config", {
        settings: {
          engine,
          ...(engine === "ppocr" ? { ppocrModelVariant: modelVariant } : {}),
        },
      });
      ocrEngine = engine;
      await loadOcrStatus();
      onfeedback(
        _t("storage.ocrEngineChanged", {
          engine: engine === "ppocr" ? "PP-OCRv6" : "Tesseract",
        }),
        true,
      );
    } catch (error) {
      console.error("Unable to save OCR config", error);
      await loadOcrStatus();
      onfeedback(_t("storage.ocrEngineChangeFailed", { error: String(error) }), false);
    }
  }

  async function saveDetConfig() {
    try {
      await invoke("set_ocr_config", {
        settings: {
          engine: ocrEngine,
          detScoreThreshold,
          detBoxThreshold,
          detUnclipRatio,
        },
      });
      onfeedback(_t("storage.ocrDetectionSaved"), true);
    } catch (error) {
      console.error("Unable to save detection config", error);
      await loadOcrStatus();
      onfeedback(_t("storage.ocrDetectionSaveFailed", { error: String(error) }), false);
    }
  }
</script>

<div class="settings-scroll">
  <SelectEntry
    searchId="ocr.engine"
    config={{
      type: "select",
      variant: "row",
      icon: "eye",
      label: _t("storage.ocrEngineLabel"),
      options: [
        { value: "ppocr", label: "PP-OCRv6" },
        { value: "tesseract", label: "Tesseract" },
      ],
      get: () => ocrEngine,
      set: (v) => saveOcrEngine(v as string),
    }}
  />

  <section class="setting-card setting-card-row" data-settings-search-id="ocr.model">
    <span class="setting-icon"><AppIcon name="download" size={17} /></span>
    <span class="setting-label">{_t("storage.ocrModelLabel")}</span>
    <CustomSelect
      className="ocr-model-select"
      value={modelVariant}
      disabled={ocrInstalling}
      options={[
        {
          value: "tiny",
          label: `tiny (~6MB)${installedVariants.includes("tiny") ? " ?" : ""}`,
        },
        {
          value: "small",
          label: `small (~30MB)${installedVariants.includes("small") ? " ?" : ""}`,
        },
        {
          value: "medium",
          label: `medium (~135MB)${installedVariants.includes("medium") ? " ?" : ""}`,
        },
      ]}
      onchange={(v) => (modelVariant = v as string)}
    />
    {#if installedVariants.includes(modelVariant)}
      <button
        type="button"
        disabled={ocrInstalling || activeVariant === modelVariant}
        onclick={applyModel}
      >
        {activeVariant === modelVariant
          ? _t("storage.ocrModelApplied")
          : _t("storage.ocrModelApply")}
      </button>
    {:else}
      <button type="button" disabled={ocrInstalling} onclick={() => installPpocr()}>
        {ocrInstalling
          ? ocrProgressPct >= 0
            ? `${ocrProgressLabel} ${Math.round(ocrProgressPct)}%`
            : _t("storage.ocrModelInstalling")
          : _t("storage.ocrModelDownload")}
      </button>
    {/if}
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="search" size={17} /></span>
      <div>
        <strong>{_t("storage.ocrDetectionTitle")}</strong>
        <p>{_t("storage.ocrDetectionDesc")}</p>
      </div>
    </div>
    <div class="ocr-parameter-grid">
      <div class="ocr-parameter">
        <label class="ocr-parameter-label" for="det-score">
          <span>{_t("storage.ocrScoreThreshold")}</span>
          <span class="ocr-parameter-value">{detScoreThreshold.toFixed(2)}</span>
        </label>
        <input
          id="det-score"
          class="transparency-slider"
          type="range"
          min="0.05"
          max="0.95"
          step="0.05"
          bind:value={detScoreThreshold}
          bind:this={detScoreSlider}
          onchange={() => saveDetConfig()}
        />
        <div class="ocr-parameter-scale">
          <span>{_t("storage.ocrLow")}</span><span>{_t("storage.ocrHigh")}</span>
        </div>
      </div>
      <div class="ocr-parameter">
        <label class="ocr-parameter-label" for="det-box">
          <span>{_t("storage.ocrBoxThreshold")}</span>
          <span class="ocr-parameter-value">{detBoxThreshold.toFixed(2)}</span>
        </label>
        <input
          id="det-box"
          class="transparency-slider"
          type="range"
          min="0.1"
          max="0.95"
          step="0.05"
          bind:value={detBoxThreshold}
          bind:this={detBoxSlider}
          onchange={() => saveDetConfig()}
        />
        <div class="ocr-parameter-scale">
          <span>{_t("storage.ocrLow")}</span><span>{_t("storage.ocrHigh")}</span>
        </div>
      </div>
      <div class="ocr-parameter">
        <label class="ocr-parameter-label" for="det-unclip">
          <span>{_t("storage.ocrUnclip")}</span>
          <span class="ocr-parameter-value">{detUnclipRatio.toFixed(1)}</span>
        </label>
        <input
          id="det-unclip"
          class="transparency-slider"
          type="range"
          min="1.0"
          max="4.0"
          step="0.1"
          bind:value={detUnclipRatio}
          bind:this={detUnclipSlider}
          onchange={() => saveDetConfig()}
        />
        <div class="ocr-parameter-scale">
          <span>{_t("storage.ocrSmall")}</span><span>{_t("storage.ocrLarge")}</span>
        </div>
      </div>
    </div>
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="search" size={17} /></span>
      <div>
        <strong>{_t("storage.ocrTaskStatus")}</strong>
        <p>{_t("storage.ocrTaskStatusDesc")}</p>
      </div>
    </div>
    <div class="ocr-stat-grid">
      <div class="stat-item">
        <span class="stat-value">{ocrTotal}</span><span class="stat-label"
          >{_t("statistics.ocrTotal")}</span
        >
      </div>
      <div class="stat-item">
        <span class="stat-value">{ocrPending}</span><span class="stat-label"
          >{_t("statistics.ocrPending")}</span
        >
      </div>
      <div class="stat-item">
        <span class="stat-value">{ocrCompleted}</span><span class="stat-label"
          >{_t("statistics.ocrCompleted")}</span
        >
      </div>
      <div class="stat-item">
        <span class="stat-value">{ocrFailed}</span><span class="stat-label"
          >{_t("statistics.ocrFailed")}</span
        >
      </div>
    </div>
    <div class:available={ocrEngineAvailable} class="ocr-engine-status">
      <span class="ocr-engine-status-label">{_t("statistics.ocrEngine")}</span>
      <strong>{ocrEngine === "ppocr" ? "PP-OCRv6" : "Tesseract"}</strong>
      <span class="ocr-engine-status-state">
        {ocrEngineAvailable
          ? _t("statistics.ocrEngineAvailable")
          : ocrHasEngine
            ? _t("statistics.ocrEngineUnavailable")
            : _t("statistics.ocrNoEngine")}
      </span>
    </div>
  </section>

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>

<style>
  :global(.ocr-model-select) {
    flex: 1;
    max-width: 200px;
  }

  .ocr-parameter-grid {
    display: grid;
    gap: 12px;
  }

  .ocr-parameter-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .ocr-parameter-value {
    flex-shrink: 0;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .ocr-parameter-scale {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-top: 8px;
    color: var(--text-faint);
    font-size: var(--settings-note-size);
  }

  .transparency-slider {
    box-sizing: border-box;
    padding: 0;
    border: 0;
  }

  .ocr-stat-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }

  .ocr-engine-status {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
    padding: 9px 10px;
    border: 1px solid color-mix(in srgb, var(--danger-color) 35%, transparent);
    border-radius: var(--settings-card-radius);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, var(--surface-bg));
    font-size: var(--settings-description-size);
  }

  .ocr-engine-status.available {
    border-color: color-mix(in srgb, var(--success-color) 35%, transparent);
    color: color-mix(in srgb, var(--success-color) 75%, white);
    background: color-mix(in srgb, var(--success-color) 12%, var(--surface-bg));
  }

  .ocr-engine-status-label {
    color: var(--text-muted);
  }

  .ocr-engine-status strong {
    color: var(--text-primary);
    font-size: var(--settings-control-size);
    font-weight: 560;
  }

  .ocr-engine-status-state {
    margin-left: auto;
  }

  .stat-item {
    min-width: 0;
    padding: 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius);
    background: var(--input-bg);
    text-align: center;
  }

  .stat-value {
    display: block;
    min-width: 0;
    color: var(--text-primary);
    font-size: 17px;
    font-weight: 600;
    margin-bottom: 4px;
    overflow-wrap: anywhere;
  }

  .stat-label {
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  @media (max-width: 760px) {
    .ocr-stat-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
