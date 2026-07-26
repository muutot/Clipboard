<script lang="ts">
  import { tick } from "svelte";
  import { generalSettings } from "$lib/services/settings";
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
    getStorageConfig,
    getStorageStatus,
    rebuildSearchIndex,
    getPerformanceMetrics,
    repairDatabase,
    setResourceStoragePaths,
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
  let activeStatisticsTab = $state<"storage" | "performance">("storage");

  const settingsBreadcrumb = $derived.by(() => {
    switch (activeSection) {
      case "general":
      case "compact":
      case "font":
        return _t("general.eyebrow");
      case "capture":
        return _t("capture.settings");
      case "storage":
        return _t("storage.settings");
      case "keyboard":
        return _t("keyboard.settings");
      case "ocr":
        return _t("storage.ocrSettings");
      case "statistics":
        return _t("storage.statisticsSettings");
    }
  });

  const settingsSectionTitle = $derived.by(() => {
    switch (activeSection) {
      case "general":
        return _t("storage.basicTab");
      case "compact":
        return _t("storage.compactTab");
      case "font":
        return _t("storage.fontTab");
      case "capture":
        return _t("capture.title");
      case "storage":
        return _t("storage.dataStorage");
      case "keyboard":
        return _t("keyboard.title");
      case "ocr":
        return _t("storage.ocrTitle");
      case "statistics":
        return activeStatisticsTab === "storage"
          ? _t("statistics.storageTab")
          : _t("statistics.performanceTab");
    }
  });

  const settingsSectionDescription = $derived.by(() => {
    switch (activeSection) {
      case "general":
        return _t("general.description");
      case "compact":
        return _t("compact.description");
      case "font":
        return _t("general.fontSizeDescription");
      case "capture":
        return _t("capture.description");
      case "storage":
        return _t("storage.configPath");
      case "keyboard":
        return _t("keyboard.description");
      case "ocr":
        return _t("storage.ocrDescription");
      case "statistics":
        return activeStatisticsTab === "storage"
          ? _t("statistics.storageDescription")
          : _t("statistics.performanceDescription");
    }
  });

  let settingsSearch = $state("");
  let settingsContent = $state<HTMLElement | null>(null);
  let settingsItemCount = $state(0);
  let settingsMatchCount = $state(0);
  let settingsSearchReady = $state(false);

  function normalizeSettingsSearch(value: string): string {
    return value.trim().replace(/\s+/g, " ").toLocaleLowerCase();
  }

  function searchableSettingsText(item: HTMLElement): string {
    const labels = item.querySelectorAll<HTMLElement>(
      "strong, p, label, .setting-label, .config-path, .path-value-inline, .column-heading, code",
    );
    const text = Array.from(labels)
      .map((element) => element.textContent ?? "")
      .join(" ");
    return normalizeSettingsSearch(text || item.textContent || "");
  }

  function applySettingsSearch(): void {
    if (!settingsContent) return;

    const items = Array.from(
      settingsContent.querySelectorAll<HTMLElement>(".setting-card, .filter-board"),
    );
    const query = normalizeSettingsSearch(settingsSearch);
    let matches = 0;

    for (const item of items) {
      const matchesQuery = !query || searchableSettingsText(item).includes(query);
      item.hidden = !matchesQuery;
      if (matchesQuery) matches += 1;
    }

    settingsItemCount = items.length;
    settingsMatchCount = matches;
    settingsSearchReady = true;
  }

  async function refreshSettingsSearch(): Promise<void> {
    await tick();
    applySettingsSearch();
  }

  function clearSettingsSearch(): void {
    settingsSearch = "";
  }

  $effect(() => {
    const root = settingsContent;
    if (!root || typeof MutationObserver === "undefined") return;

    applySettingsSearch();
    const observer = new MutationObserver(() => applySettingsSearch());
    observer.observe(root, { childList: true, characterData: true, subtree: true });
    return () => observer.disconnect();
  });

  $effect(() => {
    settingsSearch;
    activeSection;
    activeStatisticsTab;
    void refreshSettingsSearch();
  });

  let retentionPeriodDays = $state(90);
  let maxItemCount = $state(10000);
  let recycleBinDays = $state(30);
  let maxFileCopySize = $state(50 * 1024 * 1024);
  let maxFileCopySizeUnit = $state<"byte" | "KB" | "MB" | "GB">("MB");
  let maxFileCopyDisplay = $state(50);
  let imageStoragePath = $state("");
  let fileStoragePath = $state("");
  let resourceStorageRestartNeeded = $state(false);
  let savingResourceStorage = $state(false);
  let pendingResourceStorage = $state<{
    imageStoragePath: string;
    fileStoragePath: string;
  } | null>(null);

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
  let detScoreThreshold = $state(0.3);
  let detBoxThreshold = $state(0.6);
  let detUnclipRatio = $state(1.5);
  let detScoreSlider = $state<HTMLInputElement | null>(null);
  let detBoxSlider = $state<HTMLInputElement | null>(null);
  let detUnclipSlider = $state<HTMLInputElement | null>(null);

  function updateSliderTrack(el: HTMLInputElement | null) {
    if (!el) return;
    const pct = ((Number(el.value) - Number(el.min)) / (Number(el.max) - Number(el.min))) * 100;
    el.style.setProperty("--slider-pct", `${pct}%`);
  }

  $effect(() => {
    if (activeSection !== "ocr") return;
    detScoreThreshold;
    detBoxThreshold;
    detUnclipRatio;
    updateSliderTrack(detScoreSlider);
    updateSliderTrack(detBoxSlider);
    updateSliderTrack(detUnclipSlider);
  });

  $effect(() => {
    if (open) {
      void loadStatus();
    }
  });

  $effect(() => {
    if (!open || activeSection !== "ocr") return;

    void loadOcrStatus();
    const interval = setInterval(() => void loadOcrStatus(), 2000);
    return () => clearInterval(interval);
  });

  async function loadStatus() {
    loading = true;
    pending = null;
    pendingResourceStorage = null;
    resourceStorageRestartNeeded = false;
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
  }

  async function loadOcrStatus() {
    if (ocrStatusLoading) return;
    ocrStatusLoading = true;
    try {
      const result = await invoke<{
        totalTasks: number;
        pendingTasks: number;
        completedTasks: number;
        failedTasks: number;
        engine: string;
        engineAvailable: boolean;
        hasEngine: boolean;
      }>("get_ocr_status");
      if (result) {
        ocrTotal = result.totalTasks;
        ocrPending = result.pendingTasks;
        ocrCompleted = result.completedTasks;
        ocrFailed = result.failedTasks;
        ocrEngine = result.engine;
        ocrEngineAvailable = result.engineAvailable;
        ocrHasEngine = result.hasEngine;
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
          activeVariant = cfg.ppocrModelVariant;
          modelVariant = cfg.ppocrModelVariant;
        }
      }
    } catch {
      /* ignore */
    }
    try {
      const modelStatus = await invoke<{
        activeVariant: string;
        installedVariants: string[];
      }>("check_ppocr_status");
      installedVariants = modelStatus.installedVariants;
      activeVariant = modelStatus.activeVariant;
      if (!modelVariant) modelVariant = modelStatus.activeVariant;
    } catch {
      installedVariants = [];
    } finally {
      ocrStatusLoading = false;
    }
  }

  async function installPpocr() {
    ocrInstalling = true;
    feedbackSuccess = false;
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
      await invoke<string>("install_ppocr", { variant: modelVariant });
      feedback = _t("storage.ocrModelInstalled", { variant: modelVariant });
      feedbackSuccess = true;
      await loadOcrStatus();
    } catch (e) {
      feedback = _t("storage.ocrModelInstallFailed", { error: String(e) });
    } finally {
      unlisten();
      ocrInstalling = false;
      ocrProgressPct = -1;
    }
  }

  async function applyModel() {
    if (activeVariant === modelVariant) {
      feedback = _t("storage.ocrModelAlreadyApplied");
      feedbackSuccess = true;
      return;
    }
    feedbackSuccess = false;
    try {
      await invoke("set_ocr_config", {
        settings: {
          engine: "ppocr",
          ppocrModelVariant: modelVariant,
        },
      });
      await loadOcrStatus();
      ocrEngine = "ppocr";
      feedback = _t("storage.ocrModelApplied");
      feedbackSuccess = true;
    } catch (e) {
      await loadOcrStatus();
      feedback = _t("storage.ocrModelApplyFailed", { error: String(e) });
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
      const result = await getStorageConfig();
      if (result) {
        maxFileCopySize = result.maxFileCopySizeBytes;
        maxFileCopyDisplay = toDisplaySize(result.maxFileCopySizeBytes, maxFileCopySizeUnit);
        imageStoragePath = result.imageStoragePath ?? "";
        fileStoragePath = result.fileStoragePath ?? "";
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

  async function saveResourceStoragePaths() {
    savingResourceStorage = true;
    feedback = "";
    feedbackSuccess = false;
    try {
      const result = await setResourceStoragePaths(
        imageStoragePath.trim() || null,
        fileStoragePath.trim() || null,
      );
      pendingResourceStorage = result;
      resourceStorageRestartNeeded = result.restartRequired;
      feedback = result.restartRequired
        ? _t("storage.resourcePathsSavedAndRestart")
        : _t("storage.resourcePathsSaved");
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to save resource storage paths", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      savingResourceStorage = false;
    }
  }

  async function restoreDefaultResourceStoragePaths() {
    imageStoragePath = "";
    fileStoragePath = "";
    await saveResourceStoragePaths();
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
    feedbackSuccess = false;
    try {
      await invoke("set_ocr_config", {
        settings: {
          engine,
          ...(engine === "ppocr" ? { ppocrModelVariant: modelVariant } : {}),
        },
      });
      ocrEngine = engine;
      await loadOcrStatus();
      feedback = _t("storage.ocrEngineChanged", {
        engine: engine === "ppocr" ? "PP-OCRv6" : "Tesseract",
      });
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to save OCR config", error);
      await loadOcrStatus();
      feedback = _t("storage.ocrEngineChangeFailed", { error: String(error) });
    }
  }

  async function saveDetConfig() {
    feedbackSuccess = false;
    try {
      await invoke("set_ocr_config", {
        settings: {
          engine: ocrEngine,
          detScoreThreshold,
          detBoxThreshold,
          detUnclipRatio,
        },
      });
      feedback = _t("storage.ocrDetectionSaved");
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to save detection config", error);
      await loadOcrStatus();
      feedback = _t("storage.ocrDetectionSaveFailed", { error: String(error) });
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

  <div id="settings-content" class="settings-content" bind:this={settingsContent}>
    <section class="settings-section-header" aria-labelledby="settings-title">
      <div class="settings-section-heading-row">
        <div id="settings-title" class="settings-breadcrumb">{settingsBreadcrumb}</div>
        {#if $generalSettings.showSettingsCloseButton}
          <button
            class="close-button"
            type="button"
            aria-label={_t("actions.close")}
            onclick={onclose}>×</button
          >
        {/if}
      </div>
      {#if activeSection === "general" || activeSection === "compact" || activeSection === "font"}
        <nav class="settings-subnav" aria-label={_t("storage.generalTab")}>
          <button
            type="button"
            class:active={activeSection === "general"}
            aria-current={activeSection === "general" ? "page" : undefined}
            onclick={() => (activeSection = "general")}
          >
            {_t("storage.basicTab")}
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
            {_t("storage.fontTab")}
          </button>
        </nav>
      {:else if activeSection === "statistics"}
        <nav class="settings-subnav" aria-label={_t("statistics.title")}>
          <button
            type="button"
            class:active={activeStatisticsTab === "storage"}
            aria-current={activeStatisticsTab === "storage" ? "page" : undefined}
            onclick={() => (activeStatisticsTab = "storage")}
          >
            {_t("statistics.storageTab")}
          </button>
          <button
            type="button"
            class:active={activeStatisticsTab === "performance"}
            aria-current={activeStatisticsTab === "performance" ? "page" : undefined}
            onclick={() => (activeStatisticsTab = "performance")}
          >
            {_t("statistics.performanceTab")}
          </button>
        </nav>
      {:else}
        <div class="settings-subnav settings-subnav--single" aria-label={settingsSectionTitle}>
          <span class="settings-section-title">{settingsSectionTitle}</span>
        </div>
      {/if}
      <p class="settings-section-description">{settingsSectionDescription}</p>
    </section>

    <div
      class="settings-search-toolbar"
      role="search"
      aria-label={_t("storage.settingsSearchLabel")}
    >
      <div class="settings-search-field">
        <AppIcon name="search" size={15} />
        <label class="visually-hidden" for="settings-search-input"
          >{_t("storage.settingsSearchLabel")}</label
        >
        <input
          id="settings-search-input"
          type="search"
          bind:value={settingsSearch}
          aria-label={_t("storage.settingsSearchLabel")}
          placeholder={_t("storage.settingsSearchPlaceholder")}
          autocomplete="off"
          spellcheck="false"
        />
        {#if settingsSearch}
          <button
            type="button"
            class="settings-search-clear"
            aria-label={_t("storage.clearSettingsSearch")}
            onclick={clearSettingsSearch}>×</button
          >
        {/if}
      </div>
      <span class="settings-count" aria-live="polite">
        {#if settingsSearch.trim() && settingsItemCount !== settingsMatchCount}
          {_t("storage.settingsFilteredCount", {
            matched: settingsMatchCount,
            total: settingsItemCount,
          })}
        {:else}
          {_t("storage.settingsCount", { count: settingsItemCount })}
        {/if}
      </span>
    </div>
    {#if settingsSearchReady && settingsSearch.trim() && settingsItemCount > 0 && settingsMatchCount === 0}
      <div class="settings-search-empty" role="status">
        {_t("storage.settingsSearchNoResults", { query: settingsSearch.trim() })}
      </div>
    {/if}

    {#if activeSection === "general"}
      <GeneralSettingsPanel {onclose} showHeader={false} />
    {:else if activeSection === "compact"}
      <CompactSettingsPanel {onclose} showHeader={false} />
    {:else if activeSection === "font"}
      <FontSizeSettingsPanel {onclose} showHeader={false} />
    {:else if activeSection === "capture"}
      <IgnoredAppsSettingsPanel iconsDir={status?.iconsDir} {onclose} showHeader={false} />
    {:else if activeSection === "keyboard"}
      <KeyboardSettingsPanel configPath={status?.keyboardConfigPath} {onclose} showHeader={false} />
    {:else if activeSection === "ocr"}
      <div class="settings-scroll">
        <section class="setting-card setting-card-row">
          <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
          <span class="setting-label">OCR 引擎</span>
          <select
            class="model-select ocr-engine-select"
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
          <select
            bind:value={modelVariant}
            class="model-select ocr-model-select"
            disabled={ocrInstalling}
          >
            <option value="tiny">tiny (~6MB){installedVariants.includes("tiny") ? " ✓" : ""}</option
            >
            <option value="small"
              >small (~30MB){installedVariants.includes("small") ? " ✓" : ""}</option
            >
            <option value="medium"
              >medium (~135MB){installedVariants.includes("medium") ? " ✓" : ""}</option
            >
          </select>
          {#if installedVariants.includes(modelVariant)}
            <button
              type="button"
              disabled={ocrInstalling || activeVariant === modelVariant}
              onclick={applyModel}
            >
              {activeVariant === modelVariant ? "已应用" : "应用"}
            </button>
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
          <div class="ocr-parameter-grid">
            <div class="ocr-parameter">
              <label class="ocr-parameter-label" for="det-score">
                <span>分数阈值 (score)</span>
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
                <span>低 (更多区域)</span><span>高 (更少区域)</span>
              </div>
            </div>
            <div class="ocr-parameter">
              <label class="ocr-parameter-label" for="det-box">
                <span>框阈值 (box)</span>
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
                <span>低 (更多区域)</span><span>高 (更少区域)</span>
              </div>
            </div>
            <div class="ocr-parameter">
              <label class="ocr-parameter-label" for="det-unclip">
                <span>扩展比例 (unclip)</span>
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

        <p class="auto-save-note">修改即时生效，无需手动保存</p>
      </div>
      {#if feedback}
        <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
      {/if}
    {:else if activeSection === "statistics"}
      <div class="settings-scroll stats-scroll">
        {#if activeStatisticsTab === "storage"}
          {#if status}
            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="clipboard" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>总记录数</strong>
                  <p>数据库中保留的全部记录</p>
                </div>
              </div>
              <span class="stats-metric-value">{status.itemCount}</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="text" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>文本</strong>
                  <p>纯文本记录数量</p>
                </div>
              </div>
              <span class="stats-metric-value">{status.textCount}</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="link" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>链接</strong>
                  <p>链接记录数量</p>
                </div>
              </div>
              <span class="stats-metric-value">{status.linkCount}</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="image" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>图片</strong>
                  <p>托管图片数量与占用空间</p>
                </div>
              </div>
              <span class="stats-metric-value"
                >{status.imageCount} 张 · {formatBytes(status.imageSizeBytes)}</span
              >
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="file" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>文件</strong>
                  <p>托管文件数量与占用空间</p>
                </div>
              </div>
              <span class="stats-metric-value"
                >{status.fileCount} 个 · {formatBytes(status.fileSizeBytes)}</span
              >
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="file" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>数据库</strong>
                  <p>SQLite 数据库文件大小</p>
                </div>
              </div>
              <span class="stats-metric-value">{formatBytes(status.databaseSizeBytes)}</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="search" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>搜索索引</strong>
                  <p>用于全文搜索的索引文件大小</p>
                </div>
              </div>
              <span class="stats-metric-value">{formatBytes(status.searchIndexSizeBytes)}</span>
            </section>
          {:else}
            <div class="settings-state stats-empty-state">
              {loading ? "正在读取存储统计..." : "存储统计暂不可用"}
            </div>
          {/if}
          <p class="auto-save-note">统计数据来自当前项目存储</p>
        {:else}
          {#if perfMetrics}
            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>启动总耗时</strong>
                  <p>应用完成初始化所需时间</p>
                </div>
              </div>
              <span class="stats-metric-value">{perfMetrics.startup.totalStartupMs}ms</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="file" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>数据库打开</strong>
                  <p>打开本地 SQLite 数据库所需时间</p>
                </div>
              </div>
              <span class="stats-metric-value">{perfMetrics.startup.dbOpenMs}ms</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="search" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>搜索初始化</strong>
                  <p>加载搜索索引所需时间</p>
                </div>
              </div>
              <span class="stats-metric-value">{perfMetrics.startup.searchInitMs}ms</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>数据库迁移</strong>
                  <p>启动时执行数据迁移所需时间</p>
                </div>
              </div>
              <span class="stats-metric-value">{perfMetrics.startup.migrationsMs}ms</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>运行时长</strong>
                  <p>本次应用进程已运行时间</p>
                </div>
              </div>
              <span class="stats-metric-value">{perfMetrics.memory.uptimeSeconds}s</span>
            </section>

            <section class="setting-card stats-metric-card">
              <div class="setting-heading stats-metric-heading">
                <span class="setting-icon"><AppIcon name="bar-chart" size={17} /></span>
                <div class="stats-metric-copy">
                  <strong>内存峰值</strong>
                  <p>进程运行期间的最高内存占用</p>
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
                    <strong>搜索次数</strong>
                    <p>已纳入延迟统计的搜索次数</p>
                  </div>
                </div>
                <span class="stats-metric-value">{perfMetrics.searchLatency.searchesRecorded}</span>
              </section>

              <section class="setting-card stats-metric-card">
                <div class="setting-heading stats-metric-heading">
                  <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
                  <div class="stats-metric-copy">
                    <strong>平均搜索耗时</strong>
                    <p>所有已记录搜索的平均耗时</p>
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
                    <strong>P95 搜索耗时</strong>
                    <p>95% 的搜索会在此时间内完成</p>
                  </div>
                </div>
                <span class="stats-metric-value">{perfMetrics.searchLatency.p95Ms ?? "-"}ms</span>
              </section>

              <section class="setting-card stats-metric-card">
                <div class="setting-heading stats-metric-heading">
                  <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
                  <div class="stats-metric-copy">
                    <strong>P99 搜索耗时</strong>
                    <p>99% 的搜索会在此时间内完成</p>
                  </div>
                </div>
                <span class="stats-metric-value">{perfMetrics.searchLatency.p99Ms ?? "-"}ms</span>
              </section>
            {/if}
          {:else}
            <div class="settings-state stats-empty-state">性能统计暂不可用</div>
          {/if}
          <p class="auto-save-note">启动性能为应用初始化耗时，搜索延迟需触发搜索后统计</p>
        {/if}
      </div>
    {:else}
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

          <section class="setting-card">
            <div class="setting-heading">
              <span class="setting-icon"><AppIcon name="file" size={17} /></span>
              <div>
                <strong>{_t("storage.resourcePathsTitle")}</strong>
                <p>{_t("storage.resourcePathsDesc")}</p>
              </div>
            </div>
            <div class="resource-path-grid">
              <label for="image-storage-path">
                <span>{_t("storage.imageStoragePath")}</span>
                <input
                  id="image-storage-path"
                  bind:value={imageStoragePath}
                  autocomplete="off"
                  spellcheck="false"
                  placeholder={status.imagePath}
                />
              </label>
              <label for="file-storage-path">
                <span>{_t("storage.fileStoragePath")}</span>
                <input
                  id="file-storage-path"
                  bind:value={fileStoragePath}
                  autocomplete="off"
                  spellcheck="false"
                  placeholder={status.filesPath}
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
              <button
                type="button"
                disabled={savingResourceStorage}
                onclick={saveResourceStoragePaths}
                >{savingResourceStorage
                  ? _t("storage.saving")
                  : _t("storage.saveDirectory")}</button
              >
            </div>
            {#if status && (!status.imageCleanupEnabled || !status.fileCleanupEnabled)}
              <div class="resource-path-warning">
                <AppIcon name="info" size={14} />
                <span>{_t("storage.resourcePathsCleanupDisabled")}</span>
              </div>
            {/if}
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
    --settings-feedback-size: var(--settings-description-size);
    --settings-feedback-radius: 7px;
    --settings-card-radius: 9px;
    --settings-control-radius: 6px;
    --settings-icon-radius: 7px;
    --settings-close-size: 28px;
    --settings-close-radius: 7px;
    --settings-close-font-size: 19px;
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
    font-size: var(--settings-description-size);
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
    border-radius: var(--settings-control-radius);
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
    font-size: var(--settings-description-size);
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

  .settings-breadcrumb {
    flex: 0 0 auto;
    color: #777;
    font-size: var(--settings-description-size);
    font-weight: 400;
    line-height: 1.5;
  }

  .settings-section-header {
    flex: 0 0 auto;
    padding: 13px 18px 12px;
    border-bottom: 1px solid #292929;
  }

  .settings-section-heading-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }

  .settings-section-description {
    max-width: 430px;
    margin: 7px 0 0;
    color: #777;
    font-size: var(--settings-description-size);
    line-height: 1.5;
  }

  .settings-subnav {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 7px 18px 0;
    background: #191919;
  }

  .settings-section-header .settings-subnav {
    padding: 7px 0 0;
  }

  .settings-subnav button {
    min-height: 28px;
    padding: 5px 12px;
    border: 1px solid transparent;
    border-radius: var(--settings-control-radius);
    color: #888;
    background: transparent;
    font: inherit;
    font-size: var(--settings-heading-size);
    font-weight: 560;
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

  .settings-subnav--single {
    min-height: 28px;
    padding-top: 7px;
  }

  .settings-section-title {
    color: #dedede;
    font-size: var(--settings-heading-size);
    font-weight: 560;
    line-height: 1.35;
  }

  .settings-search-toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 18px 0;
  }

  .settings-search-field {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
    padding: 0 9px;
    border: 1px solid #343434;
    border-radius: var(--settings-control-radius);
    color: #777;
    background: #1a1a1a;
    transition:
      border-color 100ms ease,
      background 100ms ease;
  }

  .settings-search-field:focus-within {
    border-color: #5a5a5a;
    background: #242424;
  }

  .settings-search-field input {
    width: 100%;
    min-width: 0;
    padding: 7px 0;
    border: 0;
    outline: none;
    color: #d8d8d8;
    background: transparent;
    font: inherit;
    font-size: var(--settings-control-size);
  }

  .settings-search-field input::placeholder {
    color: #666;
  }

  .settings-search-field input::-webkit-search-cancel-button {
    appearance: none;
  }

  .settings-search-clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex: 0 0 auto;
    padding: 0;
    border: 0;
    border-radius: 50%;
    color: #999;
    background: transparent;
    font-size: 16px;
    line-height: 1;
  }

  .settings-search-clear:hover {
    color: #e0e0e0;
    background: #2a2a2a;
  }

  .settings-count {
    flex: 0 0 auto;
    color: #888;
    font-size: var(--settings-note-size);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .settings-search-empty {
    margin: 10px 18px 0;
    padding: 9px 10px;
    border: 1px dashed #3a3a3a;
    border-radius: var(--settings-control-radius);
    color: #888;
    background: rgba(30, 30, 30, 0.72);
    font-size: var(--settings-description-size);
    text-align: center;
  }

  .visually-hidden,
  label.visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .setting-heading p {
    margin: 0;
    color: #777;
    line-height: 1.5;
  }


  .close-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--settings-close-size);
    height: var(--settings-close-size);
    flex: 0 0 auto;
    padding: 0;
    border: 1px solid #353535;
    border-radius: var(--settings-close-radius);
    color: #999;
    background: #222;
    font-size: var(--settings-close-font-size);
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
    border-radius: var(--settings-icon-radius);
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

  .ocr-engine-select {
    flex: 1;
    max-width: 180px;
  }

  .ocr-model-select {
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
    color: #8a8a8a;
    font-size: var(--settings-description-size);
  }

  .ocr-parameter-value {
    flex-shrink: 0;
    color: #d7d7d7;
    font-variant-numeric: tabular-nums;
  }

  .ocr-parameter-scale {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-top: 8px;
    color: #555;
    font-size: var(--settings-note-size);
  }

  .transparency-slider {
    width: 100%;
    box-sizing: border-box;
    margin-top: 12px;
    padding: 0;
    border: 0;
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: #2a2a2a;
    outline: none;
    cursor: pointer;
  }

  .transparency-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      #4aa8ff 0%,
      #4aa8ff var(--slider-pct, 50%),
      #2a2a2a var(--slider-pct, 50%),
      #2a2a2a 100%
    );
  }

  .transparency-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border-radius: 50%;
    border: 2px solid #4aa8ff;
    background: #1a1a1a;
    cursor: pointer;
    transition:
      box-shadow 100ms ease,
      transform 100ms ease;
  }

  .transparency-slider::-webkit-slider-thumb:hover {
    box-shadow: 0 0 6px rgba(74, 168, 255, 0.4);
    transform: scale(1.15);
  }

  .transparency-slider::-moz-range-track {
    height: 4px;
    border-radius: 2px;
    background: #2a2a2a;
  }

  .transparency-slider::-moz-range-progress {
    height: 4px;
    border-radius: 2px;
    background: #4aa8ff;
  }

  .transparency-slider::-moz-range-thumb {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid #4aa8ff;
    background: #1a1a1a;
    cursor: pointer;
    transition:
      box-shadow 100ms ease,
      transform 100ms ease;
  }

  .transparency-slider::-moz-range-thumb:hover {
    box-shadow: 0 0 6px rgba(74, 168, 255, 0.4);
    transform: scale(1.15);
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
    border: 1px solid #583b3b;
    border-radius: var(--settings-card-radius);
    color: #c78b8b;
    background: rgba(52, 29, 29, 0.45);
    font-size: var(--settings-description-size);
  }

  .ocr-engine-status.available {
    border-color: #35513f;
    color: #9dc6aa;
    background: rgba(27, 45, 33, 0.45);
  }

  .ocr-engine-status-label {
    color: #888;
  }

  .ocr-engine-status strong {
    color: #dedede;
    font-size: var(--settings-control-size);
    font-weight: 560;
  }

  .ocr-engine-status-state {
    margin-left: auto;
  }

  @media (max-width: 760px) {
    .ocr-stat-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
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
    font-size: var(--settings-note-size);
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
    font-size: var(--settings-description-size);
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
    font-size: var(--settings-control-size);
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
    font-size: var(--settings-description-size);
  }

  .pending-path code {
    font-size: var(--settings-description-size);
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
    color: #d8d8d8;
    font-size: var(--settings-control-size);
    font-variant-numeric: tabular-nums;
    font-weight: 560;
    line-height: 1.45;
    text-align: right;
    overflow-wrap: anywhere;
  }

  .stat-item {
    min-width: 0;
    padding: 10px;
    border: 1px solid #2e2e2e;
    border-radius: var(--settings-card-radius);
    background: #141414;
    text-align: center;
  }

  .stat-value {
    display: block;
    min-width: 0;
    color: #e4e4e4;
    font-size: 17px;
    font-weight: 600;
    margin-bottom: 4px;
    overflow-wrap: anywhere;
  }

  .stat-label {
    color: #777;
    font-size: var(--settings-description-size);
  }

  .number-suffix {
    color: #888;
    font-size: var(--settings-description-size);
    flex-shrink: 0;
  }

  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: #777;
    font-size: var(--settings-description-size);
  }

  .settings-feedback {
    position: absolute;
    right: 18px;
    bottom: 13px;
    left: 18px;
    padding: 8px 10px;
    border: 1px solid #553434;
    border-radius: var(--settings-feedback-radius);
    color: #d59c9c;
    background: rgba(48, 27, 27, 0.96);
    font-size: var(--settings-feedback-size);
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
    .settings-search-toolbar {
      align-items: stretch;
      flex-direction: column;
      gap: 6px;
    }
    .settings-count {
      align-self: flex-end;
    }
  }

  .repair-result {
    margin-top: 10px;
    padding: 8px 9px;
    border: 1px solid #2f2f2f;
    border-radius: var(--settings-control-radius);
    background: #181818;
    font-size: var(--settings-description-size);
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
    font-size: var(--settings-note-size);
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: #666;
    font-size: var(--settings-note-size);
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
    font-size: var(--settings-heading-size);
    font-weight: 560;
  }

  .setting-card-row input {
    width: 100px;
    flex-shrink: 0;
    padding: 7px 10px;
    border: 1px solid #3a3a3a;
    border-radius: var(--settings-control-radius);
    color: #d7d7d7;
    background: #1a1a1a;
    font: inherit;
    font-size: var(--settings-control-size);
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
    font-size: var(--settings-description-size);
    flex-shrink: 0;
  }

  .config-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: #777;
    font-size: var(--settings-note-size);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .open-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: 1px solid #3a3a3a;
    border-radius: var(--settings-control-radius);
    color: #999;
    background: #222;
    font: inherit;
    font-size: var(--settings-control-size);
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
    border-radius: var(--settings-control-radius);
    color: #d7d7d7;
    background: #1a1a1a;
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: var(--settings-control-size);
    outline: none;
    transition: border-color 120ms ease;
  }

  .dir-input-row input:focus {
    border-color: #555;
  }

  .dir-input-row button {
    padding: 7px 12px;
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

  .dir-input-row button:hover {
    color: #ccc;
    background: #2e2e2e;
  }

  .dir-input-row button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .resource-path-grid {
    display: grid;
    gap: 9px;
    margin-top: 10px;
  }

  .resource-path-grid label {
    margin: 0;
  }

  .resource-path-grid label span {
    display: block;
    margin-bottom: 5px;
    color: #888;
    font-size: var(--settings-description-size);
  }

  .resource-path-actions span {
    flex: 1;
    min-width: 0;
    color: #777;
    font-size: var(--settings-description-size);
  }

  .resource-path-warning {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    margin-top: 10px;
    padding: 9px 10px;
    border: 1px solid #4a4a35;
    border-radius: var(--settings-card-radius);
    color: #c6c69d;
    background: rgba(45, 45, 27, 0.45);
    font-size: var(--settings-description-size);
    line-height: 1.45;
  }

  .resource-path-summary {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 5px 10px;
    align-items: center;
    margin-top: 10px;
    padding-top: 9px;
    border-top: 1px solid #2d2d2d;
  }

  .resource-path-summary code {
    min-width: 0;
    overflow: hidden;
    color: #a7a7a7;
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    font-size: var(--settings-description-size);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .resource-path-summary .restart-btn {
    grid-column: 2;
    grid-row: 1 / span 2;
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
    border-radius: var(--settings-control-radius);
    color: #a7a7a7;
    background: #181818;
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: var(--settings-note-size);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .path-button-row button {
    padding: 6px 12px;
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
    border-radius: var(--settings-control-radius);
    color: #d7d7d7;
    background: #1a1a1a;
    font-size: var(--settings-control-size);
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
    border-radius: var(--settings-control-radius);
    color: #d8d8d8;
    background: #2a2a2a;
    font-size: var(--settings-control-size);
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
