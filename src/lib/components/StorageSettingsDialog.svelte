<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import { generalSettings } from "$lib/services/settings";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import Checkbox from "$lib/components/Checkbox.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import DatePicker from "$lib/components/DatePicker.svelte";
  import KeyboardSettingsPanel from "$lib/components/KeyboardSettingsPanel.svelte";
  import IgnoredAppsSettingsPanel from "$lib/components/IgnoredAppsSettingsPanel.svelte";
  import GeneralSettingsPanel from "$lib/components/GeneralSettingsPanel.svelte";
  import CompactSettingsPanel from "$lib/components/CompactSettingsPanel.svelte";
  import FontSizeSettingsPanel from "$lib/components/FontSizeSettingsPanel.svelte";
  import ThemeSettingsPanel from "$lib/components/ThemeSettingsPanel.svelte";
  import IconColorsSettingsPanel from "$lib/components/IconColorsSettingsPanel.svelte";
  import TagManagementSettingsPanel from "$lib/components/TagManagementSettingsPanel.svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { resetKeyboardConfig } from "$lib/services/keyboard";
  import { listen } from "@tauri-apps/api/event";
  import {
    configureStorageDirectory,
    getStorageKindStats,
    getStorageConfig,
    getStorageStatus,
    permanentlyDeleteStorageKind,
    rebuildSearchIndex,
    getPerformanceMetrics,
    repairDatabase,
    setResourceStoragePaths,
    validateSearchIndex,
    listIconCache,
    deleteIconFiles,
    replaceIconFile,
    type StorageDirectoryUpdate,
    type StorageKind,
    type StorageKindStats,
    type StorageStatus,
    type PerformanceMetrics,
    type RepairResult,
  } from "$lib/services/storage";
  import { getMemoryDiagnostics } from "$lib/services/memory";
  import type { MemoryDiagnostics } from "$lib/types/memory";
  import { isTauriRuntime, getRuntimeInfo } from "$lib/services/runtime";
  import {
    exportToFile,
    getExportFormats,
    getImportFormats,
    importFromFile,
    getSyncConfig,
    setSyncConfig,
    testSyncConnection,
    syncUploadBackup,
    syncListRemoteBackups,
    syncDownloadBackup,
    type ExportFormatInfo,
    type ImportFormatInfo,
    type SyncConfig,
  } from "$lib/services/storage";
  import { checkForUpdate, getRelease, type UpdateInfo } from "$lib/services/update";
  import UpdateDialog from "$lib/components/UpdateDialog.svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { messages, resolvePath } from "$lib/i18n";
  import { formatBytes, updateSliderTrack } from "$lib/utils/format";
  import { endOfDay, startOfDay } from "$lib/utils/date-query";
  import {
    filterSettingsSearchItems,
    normalizeSettingsSearch,
    resolveSettingsNavPath,
    resolveSettingsSearchItems,
    type SettingsSearchItem,
  } from "$lib/settings-search";

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
  let showLimitWarning = $state(false);
  let importTruncationCount = $state(0);
  const storageKinds: readonly {
    kind: StorageKind;
    labelKey: "filter.text" | "filter.link" | "filter.image" | "filter.file";
    icon: "text" | "link" | "image" | "file";
  }[] = [
    { kind: "text", labelKey: "filter.text", icon: "text" },
    { kind: "link", labelKey: "filter.link", icon: "link" },
    { kind: "image", labelKey: "filter.image", icon: "image" },
    { kind: "file", labelKey: "filter.file", icon: "file" },
  ];
  let storageKindStats = $state<Record<StorageKind, StorageKindStats>>({
    text: { itemCount: 0, sizeBytes: 0 },
    link: { itemCount: 0, sizeBytes: 0 },
    image: { itemCount: 0, sizeBytes: 0 },
    file: { itemCount: 0, sizeBytes: 0 },
  });
  let storageKindStatsAvailable = $state(false);
  let deletingStorageKind = $state<StorageKind | null>(null);
  let iconFiles = $state<import("$lib/services/storage").IconCacheEntry[]>([]);
  let loadingIcons = $state(false);
  let selectedIconFiles = $state<Set<string>>(new Set());
  let deletingIcons = $state(false);
  let replacingIcon = $state(false);
  let replaceTarget = $state<import("$lib/services/storage").IconCacheEntry | null>(null);
  let selectedExistingIcon = $state<string | null>(null);
  let iconReplaceOptions = $derived([
    ...new Map(
      iconFiles.filter((f) => f.iconName && f.contentHash).map((e) => [e.contentHash!, e] as const),
    ).values(),
  ]);
  let exportFormats = $state<ExportFormatInfo[]>([]);
  let exportFormat = $state("json");
  let importFormats = $state<ImportFormatInfo[]>([]);
  let importFormat = $state("pastebackup");
  let exportIncludeFavorites = $state(true);
  let exportContentTypes = $state<Set<string>>(new Set(["text", "link", "image", "file"]));
  let exportDateFrom = $state("");
  let exportDateTo = $state("");
  let exporting = $state(false);
  let importing = $state(false);

  function toggleExportContentType(kind: string) {
    const next = new Set(exportContentTypes);
    if (next.has(kind)) {
      next.delete(kind);
    } else {
      next.add(kind);
    }
    exportContentTypes = next;
  }

  function exportDateToMs(value: string, end: boolean): number | null {
    if (!value) return null;
    const timestamp = new Date(`${value}T00:00:00`).getTime();
    if (Number.isNaN(timestamp)) return null;
    return end ? endOfDay(timestamp) : startOfDay(timestamp);
  }

  $effect(() => {
    if (feedback) {
      const t = setTimeout(() => {
        feedback = "";
      }, 2000);
      return () => clearTimeout(t);
    }
  });

  $effect(() => {
    if ((activeSection === "sync_cloud" || activeSection === "sync_advanced") && open) {
      void loadSyncConfig();
    }
  });
  let restartNeeded = $state(false);
  let activeSection = $state<
    | "general_search"
    | "general_items"
    | "general_window"
    | "general_general"
    | "compact"
    | "font"
    | "theme"
    | "icons"
    | "capture"
    | "capture_icons"
    | "storage_paths"
    | "storage_limits"
    | "storage_tools"
    | "sync_cloud"
    | "sync_advanced"
    | "keyboard_item"
    | "keyboard_quick"
    | "keyboard_system"
    | "tags"
    | "ocr"
    | "statistics"
    | "about"
  >("general_general");
  let activeStatisticsTab = $state<"storage" | "performance" | "memory">("storage");
  let keyboardResetToken = $state(0);

  async function handleResetKeyboard() {
    try {
      await resetKeyboardConfig();
      keyboardResetToken++;
    } catch {
      /* ignore */
    }
  }

  const settingsSectionMeta = $derived.by(() => {
    const tab = activeStatisticsTab;
    switch (activeSection) {
      case "general_search":
        return {
          title: _t("storage.generalSearchTab"),
          desc: _t("storage.generalSearchDescription"),
        };
      case "general_items":
        return {
          title: _t("storage.generalItemsTab"),
          desc: _t("storage.generalItemsDescription"),
        };
      case "general_window":
        return {
          title: _t("storage.generalWindowTab"),
          desc: _t("storage.generalWindowDescription"),
        };
      case "general_general":
        return {
          title: _t("storage.generalGeneralTab"),
          desc: _t("storage.generalGeneralDescription"),
        };
      case "compact":
        return {
          title: _t("storage.compactTab"),
          desc: _t("compact.description"),
        };
      case "font":
        return {
          title: _t("storage.fontTab"),
          desc: _t("general.fontSizeDescription"),
        };
      case "theme":
        return {
          title: _t("storage.themeTab"),
          desc: _t("general.fontSizeDescription"),
        };
      case "icons":
        return {
          title: _t("storage.iconsTab"),
          desc: _t("general.iconColorsDescription"),
        };
      case "capture":
        return {
          title: _t("capture.title"),
          desc: _t("capture.description"),
        };
      case "capture_icons":
        return {
          title: _t("storage.iconCacheTitle"),
          desc: _t("storage.iconCacheDesc"),
        };
      case "tags":
        return {
          title: _t("storage.tagsSectionTitle"),
          desc: "",
        };
      case "storage_paths":
        return {
          title: _t("storage.storagePathsTab"),
          desc: _t("storage.storagePathsDescription"),
        };
      case "storage_limits":
        return {
          title: _t("storage.storageLimitsTab"),
          desc: _t("storage.storageLimitsDescription"),
        };
      case "storage_tools":
        return {
          title: _t("storage.storageToolsTab"),
          desc: _t("storage.storageToolsDescription"),
        };
      case "sync_cloud":
      case "sync_advanced":
        return {
          title: _t("storage.syncTitle"),
          desc: _t("storage.syncDescription"),
        };
      case "keyboard_item":
        return {
          title: _t("keyboard.title"),
          desc: _t("storage.keyboardItemDescription"),
        };
      case "keyboard_quick":
        return {
          title: _t("keyboard.title"),
          desc: _t("storage.keyboardQuickDescription"),
        };
      case "keyboard_system":
        return {
          title: _t("keyboard.title"),
          desc: _t("storage.keyboardSystemDescription"),
        };
      case "ocr":
        return {
          title: _t("storage.ocrTitle"),
          desc: _t("storage.ocrDescription"),
        };
      case "statistics":
        return {
          title:
            tab === "storage"
              ? _t("statistics.storageTab")
              : tab === "performance"
                ? _t("statistics.performanceTab")
                : _t("statistics.memoryTab"),
          desc:
            tab === "storage"
              ? _t("statistics.storageDescription")
              : tab === "performance"
                ? _t("statistics.performanceDescription")
                : _t("statistics.memoryDescription"),
        };
      case "about":
        return {
          title: _t("about.sectionTitle"),
          desc: "",
        };
    }
  });

  const settingsBreadcrumb = $derived(
    resolveSettingsNavPath(_t, activeSection, activeStatisticsTab).join(" / "),
  );
  const settingsSectionTitle = $derived(settingsSectionMeta?.title);
  const settingsSectionDescription = $derived(settingsSectionMeta?.desc);

  let tagSearch = $state("");
  let settingsSearch = $state("");
  let settingsContent = $state<HTMLElement | null>(null);
  let settingsItemCount = $state(0);
  let highlightedSettingsItem: HTMLElement | null = null;
  let settingsHighlightTimer: ReturnType<typeof setTimeout> | undefined;

  const resolvedSettingsSearchItems = $derived.by(() =>
    resolveSettingsSearchItems((key) => _t(key)),
  );
  const normalizedSettingsQuery = $derived(normalizeSettingsSearch(settingsSearch));
  const settingsSearchActive = $derived(Boolean(normalizedSettingsQuery));
  const settingsSearchResults = $derived.by(() =>
    normalizedSettingsQuery
      ? filterSettingsSearchItems(resolvedSettingsSearchItems, normalizedSettingsQuery)
      : [],
  );

  function settingsElementText(item: HTMLElement): string {
    const labels = item.querySelectorAll<HTMLElement>(
      "strong, p, label, .setting-label, .config-path, .column-heading, code",
    );
    const text = Array.from(labels)
      .map((element) => element.textContent ?? "")
      .join(" ");
    return normalizeSettingsSearch(text || item.textContent || "");
  }

  function currentSettingsElements(): HTMLElement[] {
    if (!settingsContent) return [];
    return Array.from(
      settingsContent.querySelectorAll<HTMLElement>(
        ".settings-scroll .setting-card, .settings-scroll .filter-board",
      ),
    );
  }

  function updateSettingsItemCount(): void {
    settingsItemCount = currentSettingsElements().length;
  }

  function clearSettingsSearch(): void {
    settingsSearch = "";
  }

  function settingsSearchResultPath(item: SettingsSearchItem): string {
    return resolveSettingsNavPath(_t, item.section, item.statisticsTab).join(" / ");
  }

  function findSettingsElement(item: SettingsSearchItem): HTMLElement | null {
    if (settingsContent) {
      const byId = settingsContent.querySelector<HTMLElement>(
        `[data-settings-search-id="${item.id}"]`,
      );
      if (byId) return byId;
    }
    const title = normalizeSettingsSearch(item.title);
    const elements = currentSettingsElements();
    const match =
      elements.find((element) => {
        const heading = element.querySelector<HTMLElement>(
          "strong, .setting-label, .column-heading",
        );
        return normalizeSettingsSearch(heading?.textContent ?? "") === title;
      }) ??
      elements.find((element) => settingsElementText(element).includes(title)) ??
      null;
    if (match) return match;
    const header = settingsContent?.querySelector<HTMLElement>(".settings-section-header");
    if (header && normalizeSettingsSearch(header.textContent ?? "").includes(title)) return header;
    return null;
  }

  function highlightSettingsElement(element: HTMLElement): void {
    if (settingsHighlightTimer !== undefined) clearTimeout(settingsHighlightTimer);
    highlightedSettingsItem?.classList.remove("settings-search-target-highlight");
    highlightedSettingsItem = element;
    element.classList.add("settings-search-target-highlight");
    element.scrollIntoView({ behavior: "smooth", block: "center" });
    settingsHighlightTimer = setTimeout(() => {
      element.classList.remove("settings-search-target-highlight");
      if (highlightedSettingsItem === element) highlightedSettingsItem = null;
      settingsHighlightTimer = undefined;
    }, 1800);
  }

  function waitForSettingsElement(
    item: SettingsSearchItem,
    timeout = 2000,
  ): Promise<HTMLElement | null> {
    const deadline = Date.now() + timeout;
    return new Promise((resolve) => {
      const poll = () => {
        const element = findSettingsElement(item);
        if (element || Date.now() >= deadline) {
          resolve(element);
          return;
        }
        setTimeout(poll, 60);
      };
      poll();
    });
  }

  async function openSettingsSearchResult(item: SettingsSearchItem): Promise<void> {
    activeSection = item.section;
    if (item.statisticsTab) activeStatisticsTab = item.statisticsTab;
    settingsSearch = "";
    await tick();
    await tick();
    updateSettingsItemCount();
    const element = await waitForSettingsElement(item);
    if (element) highlightSettingsElement(element);
  }

  $effect(() => {
    const root = settingsContent;
    if (!root || typeof MutationObserver === "undefined") return;

    updateSettingsItemCount();
    const observer = new MutationObserver(() => updateSettingsItemCount());
    observer.observe(root, { childList: true, subtree: true });
    return () => observer.disconnect();
  });

  $effect(() => {
    activeSection;
    activeStatisticsTab;
    void tick().then(() => updateSettingsItemCount());
  });

  let retentionPeriodDays = $state(90);
  let maxItemCount = $state(10000);
  let recycleBinDays = $state(30);
  let maxFileCopySize = $state(50 * 1024 * 1024);
  let maxFileCopySizeUnit = $state<"byte" | "KB" | "MB" | "GB">("MB");
  let maxFileCopyDisplay = $state(50);
  let maxTextCaptureSize = $state(500 * 1024);
  let maxTextCaptureSizeUnit = $state<"byte" | "KB" | "MB" | "GB">("KB");
  let maxTextCaptureDisplay = $state(500);
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

  function updateMaxTextCaptureFromDisplay() {
    maxTextCaptureSize = fromDisplaySize(maxTextCaptureDisplay, maxTextCaptureSizeUnit);
  }

  function changeTextCaptureUnit(unit: "byte" | "KB" | "MB" | "GB") {
    maxTextCaptureSizeUnit = unit;
    maxTextCaptureDisplay = toDisplaySize(maxTextCaptureSize, unit);
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
  let memoryDiagnostics = $state<MemoryDiagnostics | null>(null);
  let memoryLoading = $state(false);
  let memoryError = $state("");

  type BrowserMemorySnapshot = {
    usedBytes: number;
    totalBytes: number;
    limitBytes: number;
  };
  let browserMemory = $state<BrowserMemorySnapshot | null>(null);
  let repairResult = $state<RepairResult | null>(null);
  let repairLoading = $state(false);

  function formatMaybeBytes(bytes: number | null | undefined): string {
    return bytes == null ? "��" : formatBytes(bytes);
  }

  function storageKindLabel(kind: StorageKind): string {
    return _t(storageKinds.find((entry) => entry.kind === kind)?.labelKey ?? "filter.text");
  }

  async function loadStorageKindStats(): Promise<boolean> {
    try {
      const entries = await Promise.all(
        storageKinds.map(async ({ kind }) => {
          const stats = await getStorageKindStats(kind);
          if (!stats) throw new Error("Storage kind statistics are unavailable");
          return [kind, stats] as const;
        }),
      );
      storageKindStats = Object.fromEntries(entries) as Record<StorageKind, StorageKindStats>;
      storageKindStatsAvailable = true;
      return true;
    } catch (error) {
      console.error("Unable to load storage kind statistics", error);
      storageKindStats = {
        text: { itemCount: 0, sizeBytes: 0 },
        link: { itemCount: 0, sizeBytes: 0 },
        image: { itemCount: 0, sizeBytes: 0 },
        file: { itemCount: 0, sizeBytes: 0 },
      };
      storageKindStatsAvailable = false;
      return false;
    }
  }

  async function deleteStorageKind(kind: StorageKind) {
    if (deletingStorageKind) return;

    const label = storageKindLabel(kind);
    deletingStorageKind = kind;
    feedback = "";
    feedbackSuccess = false;

    try {
      const freshStats = await getStorageKindStats(kind);
      if (!freshStats) throw new Error(_t("storage.storageUnavailable"));
      storageKindStats = { ...storageKindStats, [kind]: freshStats };
      storageKindStatsAvailable = true;
      if (freshStats.itemCount === 0) {
        feedback = _t("storage.deleteKindNoData", { kind: label });
        feedbackSuccess = true;
        return;
      }

      const confirmed = window.confirm(
        _t("storage.deleteKindConfirm", {
          kind: label,
          count: freshStats.itemCount,
          size: formatBytes(freshStats.sizeBytes),
        }),
      );
      if (!confirmed) return;

      const result = await permanentlyDeleteStorageKind(kind, freshStats);
      const warnings = [...result.warnings];
      let refreshed = true;
      try {
        status = await getStorageStatus();
        refreshed = status !== null && (await loadStorageKindStats());
        void loadPerformanceMetrics();
      } catch (error) {
        console.error("Unable to refresh storage statistics after deletion", error);
        refreshed = false;
      }
      if (!refreshed) warnings.push(_t("storage.deleteKindRefreshFailed"));

      const params = {
        kind: label,
        count: result.deletedCount,
        size: formatBytes(result.deletedSizeBytes),
        files: result.removedFiles,
      };
      if (warnings.length > 0) {
        feedback = _t("storage.deleteKindPartial", {
          ...params,
          warning: warnings.join("; "),
        });
      } else {
        feedback = _t("storage.deleteKindSuccess", params);
        feedbackSuccess = true;
      }
    } catch (error) {
      console.error(`Unable to permanently delete ${kind} storage`, error);
      await loadStorageKindStats();
      feedback = _t("storage.deleteKindFailed", {
        kind: label,
        error: error instanceof Error ? error.message : String(error),
      });
    } finally {
      deletingStorageKind = null;
    }
  }

  async function loadExportFormats(): Promise<void> {
    try {
      const formats = await getExportFormats();
      exportFormats = formats;
      if (formats.length > 0 && !formats.some((format) => format.id === exportFormat)) {
        exportFormat = formats[0].id;
      }
    } catch (error) {
      console.error("Unable to load export formats", error);
      exportFormats = [];
    }
  }

  async function loadImportFormats(): Promise<void> {
    try {
      const formats = await getImportFormats();
      importFormats = formats;
      if (formats.length > 0 && !formats.some((format) => format.id === importFormat)) {
        importFormat = formats[0].id;
      }
    } catch (error) {
      console.error("Unable to load import formats", error);
      importFormats = [];
    }
  }

  async function handleExport() {
    if (!isTauriRuntime() || exporting || importing) return;
    const format = exportFormats.find((entry) => entry.id === exportFormat);
    if (!format) return;
    exporting = true;
    feedback = "";
    feedbackSuccess = false;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const filePath = await save({
        defaultPath: `clipboard-export${format.extension}`,
        filters: [{ name: format.label, extensions: [format.extension.slice(1)] }],
      });
      if (!filePath) return;
      const result = await exportToFile(filePath, format.id, {
        includeFavorites: exportIncludeFavorites,
        dateFromMs: exportDateToMs(exportDateFrom, false),
        dateToMs: exportDateToMs(exportDateTo, true),
        contentTypes: Array.from(exportContentTypes),
      });
      feedback = _t("storage.exportSuccess", {
        path: result.path,
        size: formatBytes(result.byteCount),
      });
      feedbackSuccess = true;
    } catch (error) {
      feedback = _t("storage.exportFailed", {
        error: error instanceof Error ? error.message : String(error),
      });
    } finally {
      exporting = false;
    }
  }

  async function handleImport() {
    if (!isTauriRuntime() || exporting || importing) return;
    const format = importFormats.find((entry) => entry.id === importFormat);
    if (!format) return;
    importing = true;
    feedback = "";
    feedbackSuccess = false;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const filePath = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: format.label,
            extensions: [format.extension.slice(1)],
          },
        ],
      });
      if (!filePath) return;
      const result = await importFromFile(filePath);
      if (result.errors.length > 0) {
        const detail = `${_t("storage.importErrorsN", {
          count: result.errors.length,
        })} ${result.errors[0]}`;
        feedback = _t("storage.importPartial", {
          imported: result.importedCount,
          skipped: result.skippedCount,
          error: detail,
        });
      } else {
        feedback = _t("storage.importSuccess", {
          imported: result.importedCount,
          skipped: result.skippedCount,
        });
        feedbackSuccess = true;
      }
      if (result.pendingTruncation > 0) {
        feedback = _t("storage.importTruncationWarning", {
          max: result.maxItems,
          count: result.pendingTruncation,
        });
        feedbackSuccess = false;
        showLimitWarning = true;
        importTruncationCount = result.pendingTruncation;
      }
    } catch (error) {
      feedback = _t("storage.importFailed", {
        error: error instanceof Error ? error.message : String(error),
      });
    } finally {
      importing = false;
    }
  }

  let appVersion = $state("");
  let appExecutablePath = $state("");
  let checkingUpdate = $state(false);
  let updateResult = $state<UpdateInfo | null>(null);
  let updateError = $state("");
  let showUpdateDialog = $state(false);
  let dialogMode: "current" | "available" = $state("available");
  let loadingRelease = $state(false);

  let syncProvider = $state("off");
  let syncEndpoint = $state("");
  let syncRemotePath = $state("");
  let syncUsername = $state("");
  let syncPassword = $state("");
  let syncTesting = $state(false);
  let syncTestResult = $state<{ success: boolean; message: string } | null>(null);
  let syncing = $state(false);
  let syncLastMs = $state<number | null>(null);
  let syncStatus = $state<string | null>(null);
  let syncListing = $state(false);
  let syncDownloading = $state(false);
  let syncUnsyncedCount = $state(0);
  let syncAutoSync = $state(false);
  let syncAutoInterval = $state(300);
  let syncMaxOplogFiles = $state(10);
  let syncRolloverEntries = $state(100);
  let syncRolloverBytes = $state(51200);
  let syncMaxImageBytes = $state(5242880);
  let syncMaxFileBytes = $state(10485760);
  let syncS3Region = $state("");
  let syncS3Bucket = $state("");
  let syncS3AccessKey = $state("");
  let syncS3SecretKey = $state("");
  let syncEncryptPassword = $state("");
  let syncBackups = $state<Array<{ name: string; sizeBytes: number | null }>>([]);

  async function loadSyncConfig() {
    if (!isTauriRuntime()) return;
    try {
      const cfg: SyncConfig = await getSyncConfig();
      syncProvider = cfg.provider;
      syncEndpoint = cfg.endpoint ?? "";
      syncRemotePath = cfg.remotePath ?? "";
      syncUsername = cfg.username ?? "";
      syncLastMs = cfg.lastSyncMs ?? null;
      syncStatus = cfg.lastSyncStatus ?? null;
      syncUnsyncedCount = cfg.unsyncedCount ?? 0;
      syncAutoSync = cfg.autoSync ?? false;
      syncAutoInterval = cfg.autoSyncIntervalSecs ?? 300;
      syncMaxOplogFiles = cfg.maxRemoteOplogFiles ?? 10;
      syncRolloverEntries = cfg.oplogRolloverEntries ?? 100;
      syncRolloverBytes = cfg.oplogRolloverSizeBytes ?? 51200;
      syncMaxImageBytes = cfg.maxSyncImageBytes ?? 5242880;
      syncMaxFileBytes = cfg.maxSyncFileBytes ?? 10485760;
      syncS3Region = cfg.s3Region ?? "";
      syncS3Bucket = cfg.s3Bucket ?? "";
      syncS3AccessKey = cfg.s3AccessKey ?? "";
      syncEncryptPassword = "";
    } catch (e) {
      console.error("Failed to load sync config", e);
    }
  }

  async function saveSyncSettings() {
    if (!isTauriRuntime()) return;
    try {
      await setSyncConfig(
        syncProvider,
        syncEndpoint || null,
        syncRemotePath || null,
        syncUsername || null,
        syncPassword || null,
        syncAutoSync,
        syncAutoInterval,
        syncMaxOplogFiles,
        syncRolloverEntries,
        syncRolloverBytes,
        syncMaxImageBytes,
        syncMaxFileBytes,
        syncS3Region || null,
        syncS3Bucket || null,
        syncS3AccessKey || null,
        syncS3SecretKey || null,
        syncEncryptPassword || null,
      );
    } catch (e) {
      console.error("Failed to save sync settings", e);
    }
  }

  async function saveSyncConfig() {
    if (!isTauriRuntime()) return;
    try {
      await setSyncConfig(
        syncProvider,
        syncEndpoint || null,
        syncRemotePath || null,
        syncUsername || null,
        syncPassword || null,
        syncAutoSync,
        syncAutoInterval,
        syncMaxOplogFiles,
        syncRolloverEntries,
        syncRolloverBytes,
        syncMaxImageBytes,
        syncMaxFileBytes,
        syncS3Region || null,
        syncS3Bucket || null,
        syncS3AccessKey || null,
        syncS3SecretKey || null,
        syncEncryptPassword || null,
      );
    } catch (e) {
      console.error("Failed to save sync config", e);
    }
  }

  async function handleTestConnection() {
    if (!isTauriRuntime() || syncTesting) return;
    syncTesting = true;
    syncTestResult = null;
    try {
      const resultStr = await testSyncConnection(
        syncProvider,
        syncEndpoint,
        syncRemotePath || null,
        syncUsername || null,
        syncPassword || null,
        syncS3Region || null,
        syncS3Bucket || null,
        syncS3AccessKey || null,
        syncS3SecretKey || null,
      );
      const result = JSON.parse(resultStr);
      syncTestResult = { success: result.success, message: result.message };
    } catch (e) {
      syncTestResult = { success: false, message: String(e) };
    } finally {
      syncTesting = false;
    }
  }

  async function handleSyncUpload() {
    if (!isTauriRuntime() || syncing) return;
    syncing = true;
    feedback = "";
    try {
      const result = await syncUploadBackup();
      if (result.backupType === "noop") {
        feedback = "û����������Ҫͬ��";
        feedbackSuccess = true;
      } else {
        const typeLabel = result.backupType === "oplog" ? "����" : "ȫ��";
        feedback = `${typeLabel}ͬ���ɹ�: ${result.itemsSynced} ����¼, ${result.resourcesSynced} ����Դ, ${(result.bytesUploaded / 1024).toFixed(1)} KB`;
        feedbackSuccess = true;
      }
      syncLastMs = Date.now();
      syncStatus = "success";
    } catch (e) {
      feedback = _t("storage.syncUploadFailed") + `: ${String(e)}`;
      feedbackSuccess = false;
      syncStatus = "failed";
    } finally {
      syncing = false;
    }
  }

  async function handleListBackups() {
    if (!isTauriRuntime() || syncListing) return;
    syncListing = true;
    try {
      const entries = await syncListRemoteBackups();
      syncBackups = entries
        .filter((e) => !e.isDirectory && e.name.endsWith(".zip"))
        .map((e) => ({ name: e.name, sizeBytes: e.sizeBytes }));
    } catch (e) {
      console.error("Failed to list backups", e);
      syncBackups = [];
    } finally {
      syncListing = false;
    }
  }

  async function handleDownloadBackup(filename: string) {
    if (!isTauriRuntime() || syncDownloading) return;
    syncDownloading = true;
    feedback = "";
    try {
      const path = await syncDownloadBackup(filename);
      feedback = _t("storage.syncDownloadSuccess") + `: ${path}`;
      feedbackSuccess = true;
    } catch (e) {
      feedback = _t("storage.syncDownloadFailed") + `: ${String(e)}`;
      feedbackSuccess = false;
    } finally {
      syncDownloading = false;
    }
  }

  async function loadAppVersion(): Promise<void> {
    if (!isTauriRuntime()) return;
    try {
      appVersion = await getVersion();
    } catch (error) {
      console.error("Unable to read app version", error);
    }
    try {
      const info = await getRuntimeInfo();
      appExecutablePath = info?.executablePath ?? "";
    } catch (error) {
      console.error("Unable to read runtime info", error);
    }
  }

  async function handleViewRelease(): Promise<void> {
    if (!isTauriRuntime() || loadingRelease || !appVersion) return;
    loadingRelease = true;
    try {
      updateResult = await getRelease(appVersion);
      dialogMode = "current";
      showUpdateDialog = true;
    } catch (error) {
      updateError = error instanceof Error ? error.message : String(error);
    } finally {
      loadingRelease = false;
    }
  }

  async function handleCheckUpdate(): Promise<void> {
    if (!isTauriRuntime() || checkingUpdate) return;
    checkingUpdate = true;
    updateResult = null;
    updateError = "";
    try {
      updateResult = await checkForUpdate();
      if (updateResult.updateAvailable) {
        dialogMode = "available";
        showUpdateDialog = true;
      }
    } catch (error) {
      updateError = error instanceof Error ? error.message : String(error);
    } finally {
      checkingUpdate = false;
    }
  }

  function formatUpdateDate(value: string | null): string {
    if (!value) return "";
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString();
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
  let ocrDownloadUnlisten: (() => void) | undefined;
  let ocrInstallRequestId = 0;
  let componentDestroyed = false;
  let detScoreThreshold = $state(0.3);
  let detBoxThreshold = $state(0.6);
  let detUnclipRatio = $state(1.5);
  let detScoreSlider = $state<HTMLInputElement | null>(null);
  let detBoxSlider = $state<HTMLInputElement | null>(null);
  let detUnclipSlider = $state<HTMLInputElement | null>(null);

  function releaseOcrDownloadListener(): void {
    if (!ocrDownloadUnlisten) return;
    ocrDownloadUnlisten();
    ocrDownloadUnlisten = undefined;
  }

  onDestroy(() => {
    componentDestroyed = true;
    ocrInstallRequestId += 1;
    releaseOcrDownloadListener();
    if (settingsHighlightTimer !== undefined) {
      clearTimeout(settingsHighlightTimer);
      settingsHighlightTimer = undefined;
    }
    highlightedSettingsItem?.classList.remove("settings-search-target-highlight");
    highlightedSettingsItem = null;
  });

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
    if (activeSection !== "storage_limits") return;
    maxTextCaptureSize = $generalSettings.maxTextCaptureBytes;
    maxTextCaptureDisplay = toDisplaySize(
      $generalSettings.maxTextCaptureBytes,
      maxTextCaptureSizeUnit,
    );
  });

  async function refreshStorageStats() {
    try {
      status = await getStorageStatus();
      await loadStorageKindStats();
    } catch (error) {
      console.error("Unable to refresh storage statistics", error);
    }
    void loadPerformanceMetrics();
  }

  $effect(() => {
    if (open) {
      void loadStatus();
      void loadExportFormats();
      void loadImportFormats();
      void loadAppVersion();
      let unlistenAdd: (() => void) | undefined;
      let unlistenInvalidated: (() => void) | undefined;
      listen("clipboard-item-added", () => void refreshStorageStats()).then((unlisten) => {
        unlistenAdd = unlisten;
      });
      listen("clipboard-history-invalidated", () => void refreshStorageStats()).then((unlisten) => {
        unlistenInvalidated = unlisten;
      });
      return () => {
        unlistenAdd?.();
        unlistenInvalidated?.();
      };
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
      await loadStorageKindStats();
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
          if (!modelVariant) modelVariant = cfg.ppocrModelVariant;
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
    const requestId = ++ocrInstallRequestId;
    releaseOcrDownloadListener();
    ocrInstalling = true;
    feedbackSuccess = false;
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
        if (componentDestroyed || requestId !== ocrInstallRequestId) return;
        ocrProgressLabel = event.payload.label;
        ocrProgressPct = event.payload.percentage;
        ocrProgressCurrent = event.payload.current;
        ocrProgressTotal = event.payload.total;
      });
      if (componentDestroyed || requestId !== ocrInstallRequestId) {
        unlisten();
        return;
      }
      ocrDownloadUnlisten = unlisten;
      await invoke<string>("install_ppocr", { variant: modelVariant });
      if (componentDestroyed || requestId !== ocrInstallRequestId) return;
      feedback = _t("storage.ocrModelInstalled", { variant: modelVariant });
      feedbackSuccess = true;
      await loadOcrStatus();
    } catch (e) {
      if (!componentDestroyed && requestId === ocrInstallRequestId) {
        feedback = _t("storage.ocrModelInstallFailed", { error: String(e) });
      }
    } finally {
      if (requestId === ocrInstallRequestId) {
        releaseOcrDownloadListener();
        if (!componentDestroyed) {
          ocrInstalling = false;
          ocrProgressPct = -1;
        }
      }
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
    if (!open || activeSection !== "statistics" || activeStatisticsTab !== "memory") return;
    void loadMemoryDiagnostics();
    const interval = setInterval(() => void loadMemoryDiagnostics(), 3000);
    return () => clearInterval(interval);
  });

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

  async function loadIconList() {
    loadingIcons = true;
    try {
      iconFiles = await listIconCache();
      selectedIconFiles = new Set();
    } catch (error) {
      console.error("Unable to list icon cache", error);
    } finally {
      loadingIcons = false;
    }
  }

  function toggleIconFile(name: string) {
    const next = new Set(selectedIconFiles);
    if (next.has(name)) {
      next.delete(name);
    } else {
      next.add(name);
    }
    selectedIconFiles = next;
  }

  async function deleteSelectedIcons() {
    if (selectedIconFiles.size === 0) return;
    deletingIcons = true;
    try {
      const deleted = await deleteIconFiles([...selectedIconFiles]);
      feedback = _t("storage.iconsDeleted", { count: deleted });
      feedbackSuccess = true;
      await loadIconList();
    } catch (error) {
      console.error("Unable to delete icon files", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      deletingIcons = false;
    }
  }

  async function applyIconReplacement(name: string, sourcePath: string) {
    if (!isTauriRuntime() || replacingIcon) return;
    replacingIcon = true;
    feedback = "";
    feedbackSuccess = false;
    try {
      await replaceIconFile(name, sourcePath);
      feedback = _t("storage.iconReplaced", { name });
      feedbackSuccess = true;
      await loadIconList();
      closeReplaceDialog();
    } catch (error) {
      console.error("Unable to replace icon", error);
      feedback = _t("storage.iconReplaceFailed", {
        error: error instanceof Error ? error.message : String(error),
      });
    } finally {
      replacingIcon = false;
    }
  }

  async function chooseReplaceFile() {
    const name = replaceTarget?.targetIconName;
    if (!name || replacingIcon) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const filePath = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "ico", "webp", "svg"] }],
    });
    if (!filePath) return;
    await applyIconReplacement(name, filePath);
  }

  async function confirmExistingIcon() {
    const name = replaceTarget?.targetIconName;
    if (!name || !selectedExistingIcon || replacingIcon) return;
    const sourcePath = `${status?.iconsDir ?? ""}/${selectedExistingIcon}`.replace(/\\/g, "/");
    await applyIconReplacement(name, sourcePath);
  }

  function openReplaceDialog(file: import("$lib/services/storage").IconCacheEntry) {
    replaceTarget = file;
    selectedExistingIcon = null;
  }

  function closeReplaceDialog() {
    replaceTarget = null;
    selectedExistingIcon = null;
  }

  $effect(() => {
    if (activeSection === "capture_icons") {
      void loadIconList();
    }
  });

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

  function saveMaxTextCaptureSize() {
    generalSettings.updateSetting("maxTextCaptureBytes", maxTextCaptureSize);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (replaceTarget) {
        closeReplaceDialog();
      } else {
        onclose();
      }
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
        <small>{appVersion}</small>
      </div>
    </div>

    <div
      class="settings-sidebar-search"
      role="search"
      aria-label={_t("storage.settingsSearchLabel")}
    >
      <div class="settings-search-field">
        <AppIcon name="search" size={14} />
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
            onclick={clearSettingsSearch}>��</button
          >
        {/if}
      </div>
    </div>

    <nav class="settings-primary-nav" aria-label={_t("storage.navAriaLabel")}>
      <button
        class:active={activeSection === "general_search" ||
          activeSection === "general_items" ||
          activeSection === "general_window" ||
          activeSection === "general_general"}
        type="button"
        onclick={() => (activeSection = "general_general")}
      >
        <AppIcon name="sliders" size={16} />
        <span>{_t("storage.generalTab")}</span>
      </button>
      <button
        class:active={activeSection === "compact" ||
          activeSection === "font" ||
          activeSection === "theme" ||
          activeSection === "icons"}
        type="button"
        onclick={() => (activeSection = "theme")}
      >
        <AppIcon name="palette" size={16} />
        <span>{_t("storage.appearanceTab")}</span>
      </button>
      <button
        class:active={activeSection === "capture" || activeSection === "capture_icons"}
        type="button"
        onclick={() => (activeSection = "capture")}
      >
        <AppIcon name="filter" size={16} />
        <span>{_t("storage.captureTab")}</span>
      </button>
      <button
        class:active={activeSection === "storage_paths" ||
          activeSection === "storage_limits" ||
          activeSection === "storage_tools"}
        type="button"
        onclick={() => (activeSection = "storage_paths")}
      >
        <AppIcon name="file" size={16} />
        <span>{_t("storage.storageTab")}</span>
      </button>
      <button
        class:active={activeSection === "sync_cloud" || activeSection === "sync_advanced"}
        type="button"
        onclick={() => (activeSection = "sync_cloud")}
      >
        <AppIcon name="cloud" size={16} />
        <span>{_t("storage.syncTab")}</span>
      </button>
      <button
        class:active={activeSection === "keyboard_item" ||
          activeSection === "keyboard_quick" ||
          activeSection === "keyboard_system"}
        type="button"
        onclick={() => (activeSection = "keyboard_item")}
      >
        <AppIcon name="keyboard" size={16} />
        <span>{_t("storage.keyboardTab")}</span>
      </button>
      <button
        class:active={activeSection === "tags"}
        type="button"
        onclick={() => (activeSection = "tags")}
      >
        <AppIcon name="tag" size={16} />
        <span>{_t("storage.tagsTab")}</span>
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
        <span>{_t("storage.statisticsTab")}</span>
      </button>
      <button
        class:active={activeSection === "about"}
        type="button"
        onclick={() => (activeSection = "about")}
      >
        <AppIcon name="info" size={16} />
        <span>{_t("about.tabLabel")}</span>
      </button>
    </nav>

    <div class="sidebar-foot">
      {#if status}
        {@const localUsageBytes =
          status.databaseSizeBytes +
          status.imageSizeBytes +
          status.fileSizeBytes +
          status.searchIndexSizeBytes}
        <div class="sidebar-usage">
          <span>{_t("storage.sidebarUsage")}</span>
          <strong>{formatBytes(localUsageBytes)}</strong>
        </div>
        {#if status.diskTotalBytes != null && status.diskAvailableBytes != null && status.diskTotalBytes > 0}
          {@const diskUsedPercent = Math.min(
            100,
            Math.max(
              0,
              Math.round(
                ((status.diskTotalBytes - status.diskAvailableBytes) / status.diskTotalBytes) * 100,
              ),
            ),
          )}
          <div
            class="sidebar-usage-bar"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={diskUsedPercent}
            aria-label={_t("storage.sidebarUsage")}
          >
            <span style:width={`${diskUsedPercent}%`}></span>
          </div>
          <span class="sidebar-usage-caption">
            {_t("storage.sidebarDiskFree", {
              available: formatBytes(status.diskAvailableBytes),
              total: formatBytes(status.diskTotalBytes),
            })}
          </span>
        {/if}
      {/if}
    </div>
  </aside>

  <div id="settings-content" class="settings-content" bind:this={settingsContent}>
    <section class="settings-section-header" aria-labelledby="settings-title">
      <div class="settings-section-heading-row">
        <div id="settings-title" class="settings-breadcrumb">{settingsBreadcrumb}</div>
        <div class="settings-section-actions">
          <span class="settings-count" aria-live="polite">
            {#if settingsSearchActive}
              {_t("storage.settingsFilteredCount", {
                matched: settingsSearchResults.length,
                total: resolvedSettingsSearchItems.length,
              })}
            {:else}
              {_t("storage.settingsCount", { count: settingsItemCount })}
            {/if}
          </span>
          {#if $generalSettings.showSettingsCloseButton}
            <button
              class="close-button"
              type="button"
              aria-label={_t("actions.close")}
              onclick={onclose}>×</button
            >
          {/if}
        </div>
      </div>
      {#if activeSection === "keyboard_item" || activeSection === "keyboard_quick" || activeSection === "keyboard_system"}
        <section
          class="setting-card toggle-card"
          style="margin-top:3px"
          data-settings-search-id="keyboard.config-file"
        >
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="keyboard" size={17} /></span>
            <div>
              <strong>{_t("keyboard.shortcutConfigTitle")}</strong>
              <p>{_t("storage.keyboardConfigNote")}</p>
            </div>
          </div>
          <div style="display:flex;gap:6px;flex-shrink:0">
            <button
              type="button"
              class="config-bar-btn"
              onclick={() =>
                invoke("open_external_url", {
                  url: status?.keyboardConfigPath ?? "conf/keyboard.json",
                })}
            >
              <AppIcon name="file" size={13} />
              {_t("keyboard.openFile")}
            </button>
            <button type="button" class="config-bar-btn" onclick={() => handleResetKeyboard()}>
              <AppIcon name="restore" size={13} />
              {_t("storage.resetAll")}
            </button>
          </div>
        </section>
      {/if}
      {#if activeSection === "compact" || activeSection === "font" || activeSection === "theme" || activeSection === "icons"}
        <nav class="settings-subnav" aria-label={_t("storage.appearanceTab")}>
          <button
            type="button"
            class:active={activeSection === "theme"}
            aria-current={activeSection === "theme" ? "page" : undefined}
            onclick={() => (activeSection = "theme")}
          >
            {_t("storage.themeTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "font"}
            aria-current={activeSection === "font" ? "page" : undefined}
            onclick={() => (activeSection = "font")}
          >
            {_t("storage.fontTab")}
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
            class:active={activeSection === "icons"}
            aria-current={activeSection === "icons" ? "page" : undefined}
            onclick={() => (activeSection = "icons")}
          >
            {_t("storage.iconsTab")}
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
          <button
            type="button"
            class:active={activeStatisticsTab === "memory"}
            aria-current={activeStatisticsTab === "memory" ? "page" : undefined}
            onclick={() => (activeStatisticsTab = "memory")}
          >
            {_t("statistics.memoryTab")}
          </button>
        </nav>
      {:else if activeSection === "general_search" || activeSection === "general_items" || activeSection === "general_window" || activeSection === "general_general"}
        <nav class="settings-subnav" aria-label={_t("storage.generalTab")}>
          <button
            type="button"
            class:active={activeSection === "general_general"}
            aria-current={activeSection === "general_general" ? "page" : undefined}
            onclick={() => (activeSection = "general_general")}
          >
            {_t("storage.generalGeneralTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "general_window"}
            aria-current={activeSection === "general_window" ? "page" : undefined}
            onclick={() => (activeSection = "general_window")}
          >
            {_t("storage.generalWindowTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "general_search"}
            aria-current={activeSection === "general_search" ? "page" : undefined}
            onclick={() => (activeSection = "general_search")}
          >
            {_t("storage.generalSearchTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "general_items"}
            aria-current={activeSection === "general_items" ? "page" : undefined}
            onclick={() => (activeSection = "general_items")}
          >
            {_t("storage.generalItemsTab")}
          </button>
        </nav>
      {:else if activeSection === "keyboard_item" || activeSection === "keyboard_quick" || activeSection === "keyboard_system"}
        <nav class="settings-subnav" aria-label={_t("storage.keyboardTab")}>
          <button
            type="button"
            class:active={activeSection === "keyboard_item"}
            aria-current={activeSection === "keyboard_item" ? "page" : undefined}
            onclick={() => (activeSection = "keyboard_item")}
          >
            {_t("storage.keyboardItemTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "keyboard_quick"}
            aria-current={activeSection === "keyboard_quick" ? "page" : undefined}
            onclick={() => (activeSection = "keyboard_quick")}
          >
            {_t("storage.keyboardQuickTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "keyboard_system"}
            aria-current={activeSection === "keyboard_system" ? "page" : undefined}
            onclick={() => (activeSection = "keyboard_system")}
          >
            {_t("storage.keyboardSystemTab")}
          </button>
        </nav>
      {:else if activeSection === "capture" || activeSection === "capture_icons"}
        <nav class="settings-subnav" aria-label={_t("storage.captureTab")}>
          <button
            type="button"
            class:active={activeSection === "capture"}
            aria-current={activeSection === "capture" ? "page" : undefined}
            onclick={() => (activeSection = "capture")}
          >
            {_t("capture.title")}
          </button>
          <button
            type="button"
            class:active={activeSection === "capture_icons"}
            aria-current={activeSection === "capture_icons" ? "page" : undefined}
            onclick={() => (activeSection = "capture_icons")}
          >
            {_t("storage.iconCacheTitle")}
          </button>
        </nav>
      {:else if activeSection === "storage_paths" || activeSection === "storage_limits" || activeSection === "storage_tools"}
        <nav class="settings-subnav" aria-label={_t("storage.storageTab")}>
          <button
            type="button"
            class:active={activeSection === "storage_paths"}
            aria-current={activeSection === "storage_paths" ? "page" : undefined}
            onclick={() => (activeSection = "storage_paths")}
          >
            {_t("storage.storagePathsTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "storage_limits"}
            aria-current={activeSection === "storage_limits" ? "page" : undefined}
            onclick={() => (activeSection = "storage_limits")}
          >
            {_t("storage.storageLimitsTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "storage_tools"}
            aria-current={activeSection === "storage_tools" ? "page" : undefined}
            onclick={() => (activeSection = "storage_tools")}
          >
            {_t("storage.storageToolsTab")}
          </button>
        </nav>
      {:else if activeSection === "sync_cloud" || activeSection === "sync_advanced"}
        <nav class="settings-subnav" aria-label={_t("storage.syncTab")}>
          <button
            type="button"
            class:active={activeSection === "sync_cloud"}
            aria-current={activeSection === "sync_cloud" ? "page" : undefined}
            onclick={() => (activeSection = "sync_cloud")}
          >
            {_t("storage.syncCloudTab")}
          </button>
          <button
            type="button"
            class:active={activeSection === "sync_advanced"}
            aria-current={activeSection === "sync_advanced" ? "page" : undefined}
            onclick={() => (activeSection = "sync_advanced")}
          >
            {_t("storage.syncAdvancedTab")}
          </button>
        </nav>
      {:else}
        <div
          class="settings-subnav settings-subnav--single"
          class:settings-subnav--tags={activeSection === "tags"}
          aria-label={settingsSectionTitle}
        >
          <span class="settings-section-title">{settingsSectionTitle}</span>
          {#if activeSection === "tags"}
            <label class="settings-tag-search" aria-label={_t("tags.searchPlaceholder")}>
              <AppIcon name="search" size={15} />
              <input
                type="search"
                value={tagSearch}
                placeholder={_t("tags.searchPlaceholder")}
                aria-label={_t("tags.searchPlaceholder")}
                oninput={(e) => (tagSearch = (e.currentTarget as HTMLInputElement).value)}
              />
            </label>
          {/if}
        </div>
      {/if}
      {#if settingsSectionDescription}
        <p class="settings-section-description">{settingsSectionDescription}</p>
      {/if}
    </section>

    {#if settingsSearchActive}
      <div class="settings-scroll settings-search-results" aria-live="polite">
        {#if settingsSearchResults.length > 0}
          {#each settingsSearchResults as result (result.id)}
            <button
              type="button"
              class="settings-search-result"
              data-settings-search-id={result.id}
              onclick={() => void openSettingsSearchResult(result)}
            >
              <span class="settings-search-result-path">{settingsSearchResultPath(result)}</span>
              <strong>{result.title}</strong>
              {#if result.description}
                <p>{result.description}</p>
              {/if}
            </button>
          {/each}
        {:else}
          <div class="settings-search-empty" role="status">
            {_t("storage.settingsSearchNoResults", { query: settingsSearch.trim() })}
          </div>
        {/if}
      </div>
    {:else if activeSection === "general_search" || activeSection === "general_items" || activeSection === "general_window" || activeSection === "general_general"}
      <GeneralSettingsPanel
        {onclose}
        section={activeSection === "general_search"
          ? "search"
          : activeSection === "general_items"
            ? "items"
            : activeSection === "general_general"
              ? "general"
              : "window"}
        showHeader={false}
      />
    {:else if activeSection === "compact"}
      <CompactSettingsPanel {onclose} showHeader={false} />
    {:else if activeSection === "font"}
      <FontSizeSettingsPanel {onclose} showHeader={false} />
    {:else if activeSection === "theme"}
      <ThemeSettingsPanel {onclose} showHeader={false} />
    {:else if activeSection === "icons"}
      <IconColorsSettingsPanel {onclose} showHeader={false} />
    {:else if activeSection === "capture"}
      <IgnoredAppsSettingsPanel iconsDir={status?.iconsDir} {onclose} showHeader={false} />
    {:else if activeSection === "capture_icons"}
      <div class="settings-scroll">
        <section class="setting-card">
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="image" size={17} /></span>
            <div>
              <strong>{_t("storage.iconCacheTitle")}</strong>
              <p>{_t("storage.iconCacheDesc")}</p>
            </div>
          </div>
          {#if loadingIcons}
            <p class="settings-state">{_t("storage.loadingIcons")}</p>
          {:else if iconFiles.length === 0}
            <p class="settings-state">{_t("storage.noIconFiles")}</p>
          {:else}
            {@const selectableIcons = iconFiles.filter((f) => f.iconName != null)}
            <div class="icon-table-header">
              <span class="icon-col-check"></span>
              <span class="icon-col-app">{_t("storage.iconColApp")}</span>
              <span class="icon-col-icon">{_t("storage.iconColIcon")}</span>
              <span class="icon-col-size">{_t("storage.iconColSize")}</span>
              <span class="icon-col-action">{_t("storage.iconColAction")}</span>
            </div>
            <ul class="icon-file-list">
              {#each iconFiles as file}
                <li class="icon-file-item">
                  <span class="icon-col-check">
                    {#if file.iconName}
                      <label class="row-check">
                        <Checkbox
                          checked={selectedIconFiles.has(file.iconName)}
                          onchange={() => toggleIconFile(file.iconName!)}
                          size={14}
                        />
                      </label>
                    {/if}
                  </span>
                  <span class="icon-col-app">
                    <span class="icon-app-name">{file.displayName}</span>
                    {#if file.appName == null}<span class="icon-orphan-mark">*</span>{/if}
                  </span>
                  <span class="icon-col-icon">
                    {#if file.iconName}
                      <img
                        class="icon-preview"
                        src={convertFileSrc(
                          `${status?.iconsDir ?? ""}/${file.iconName}`.replace(/\\/g, "/"),
                        )}
                        alt=""
                        onerror={(e) => {
                          (e.target as HTMLImageElement).style.display = "none";
                        }}
                      />
                    {:else}
                      <span class="icon-letter">{file.firstChar}</span>
                    {/if}
                  </span>
                  <span class="icon-col-size">
                    {file.sizeBytes === 0 ? "0" : formatBytes(file.sizeBytes)}
                  </span>
                  <span class="icon-col-action">
                    <button
                      type="button"
                      class="icon-replace-btn"
                      disabled={replacingIcon}
                      onclick={() => openReplaceDialog(file)}
                    >
                      {_t("storage.replaceIcon")}
                    </button>
                  </span>
                </li>
              {/each}
            </ul>
            <div class="icon-actions">
              <label class="icon-select-all">
                <Checkbox
                  checked={selectableIcons.length > 0 &&
                    selectedIconFiles.size === selectableIcons.length}
                  disabled={selectableIcons.length === 0}
                  onchange={(checked) => {
                    selectedIconFiles = checked
                      ? new Set(selectableIcons.map((f) => f.iconName as string))
                      : new Set();
                  }}
                  size={14}
                />
                <span>{_t("storage.selectAll")}</span>
              </label>
              <span class="icon-file-count">
                {selectedIconFiles.size} / {iconFiles.length}
                {_t("storage.selected")}
              </span>
              <span class="icon-actions-spacer"></span>
              <button
                type="button"
                disabled={selectedIconFiles.size === 0 || deletingIcons}
                onclick={deleteSelectedIcons}
              >
                {deletingIcons
                  ? _t("storage.deletingIcons")
                  : _t("storage.deleteSelectedIcons", { count: selectedIconFiles.size })}
              </button>
            </div>
          {/if}
        </section>
      </div>
    {:else if activeSection === "tags"}
      <TagManagementSettingsPanel
        {onclose}
        showHeader={false}
        {tagSearch}
        ontagSearchChange={(v) => (tagSearch = v)}
      />
    {:else if activeSection === "keyboard_item"}
      <KeyboardSettingsPanel
        {onclose}
        resetToken={keyboardResetToken}
        category="item"
        showHeader={false}
      />
    {:else if activeSection === "keyboard_quick"}
      <KeyboardSettingsPanel
        {onclose}
        resetToken={keyboardResetToken}
        category="quick"
        showHeader={false}
      />
    {:else if activeSection === "keyboard_system"}
      <KeyboardSettingsPanel
        {onclose}
        resetToken={keyboardResetToken}
        category="system"
        showHeader={false}
      />
    {:else if activeSection === "ocr"}
      <div class="settings-scroll">
        <section class="setting-card setting-card-row">
          <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
          <span class="setting-label">{_t("storage.ocrEngineLabel")}</span>
          <CustomSelect
            className="ocr-engine-select"
            value={ocrEngine}
            options={[
              { value: "ppocr", label: "PP-OCRv6" },
              { value: "tesseract", label: "Tesseract" },
            ]}
            onchange={(v) => saveOcrEngine(v as string)}
          />
        </section>

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
        {:else if activeStatisticsTab === "performance"}
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
              <button
                type="button"
                class="memory-refresh"
                onclick={() => void loadMemoryDiagnostics()}
              >
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
                    <span class="memory-process-size"
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
                <div class="setting-heading stats-metric-heading">
                  <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
                  <div class="stats-metric-copy">
                    <strong>{_t("storage.ocrModelLabel")}</strong>
                    <p>
                      {memoryDiagnostics.ocr.engine} / {memoryDiagnostics.ocr.modelVariant}
                      {memoryDiagnostics.ocr.loaded
                        ? ` �� ${_t("statistics.ocrEngineAvailable")}`
                        : ` �� ${_t("storage.ocrModelNotInstalled")}`}
                    </p>
                  </div>
                </div>
                <span class="stats-metric-value"
                  >{formatBytes(memoryDiagnostics.ocr.modelBytes)} �� {memoryDiagnostics.ocr
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
    {:else if activeSection === "about"}
      <div class="settings-scroll">
        <section class="setting-card toggle-card" data-settings-search-id="about.info">
          <div class="setting-heading">
            <span class="brand-icon"><AppIcon name="clipboard" size={18} /></span>
            <div>
              <strong>{_t("app.name")}</strong>
              <p>{_t("about.versionLabel", { version: appVersion })}</p>
            </div>
          </div>
          <div class="about-update-controls">
            <CustomSelect
              value={$generalSettings.updateSource}
              ariaLabel={_t("about.updateSource")}
              options={[
                { value: "gitcode", label: _t("about.updateSourceGitcode") },
                { value: "github", label: _t("about.updateSourceGithub") },
              ]}
              onchange={(v) =>
                generalSettings.updateSetting("updateSource", v as "gitcode" | "github")}
            />
            <button
              type="button"
              class="settings-action-btn"
              disabled={!appVersion || loadingRelease}
              onclick={handleViewRelease}
            >
              {loadingRelease ? _t("about.loadingReleaseNotes") : _t("about.releaseNotes")}
            </button>
            <button
              type="button"
              class="settings-action-btn"
              disabled={checkingUpdate}
              onclick={handleCheckUpdate}
            >
              {checkingUpdate ? _t("about.checking") : _t("about.checkUpdate")}
            </button>
          </div>
        </section>

        <section class="setting-card">
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="file" size={17} /></span>
            <div>
              <strong>{_t("about.executablePathTitle")}</strong>
              <p class="about-path">{appExecutablePath || _t("about.executablePathEmpty")}</p>
            </div>
          </div>
        </section>

        <section class="setting-card toggle-card">
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="code" size={17} /></span>
            <div>
              <strong>{_t("about.repoTitle")}</strong>
              <p>{_t("about.repoDesc")}</p>
            </div>
          </div>
          <div class="about-update-controls">
            <button
              type="button"
              class="settings-action-btn"
              onclick={() =>
                invoke("open_external_url", { url: "https://github.com/muutot/Clipboard" })}
            >
              GitHub
            </button>
            <button
              type="button"
              class="settings-action-btn"
              onclick={() =>
                invoke("open_external_url", { url: "https://gitcode.com/m2u/Clipboard" })}
            >
              GitCode
            </button>
          </div>
        </section>

        {#if updateResult}
          {#if !updateResult.updateAvailable}
            <div class="about-update-state" role="status">
              <AppIcon name="check" size={14} />
              <span>{_t("about.upToDate")}</span>
            </div>
          {/if}
        {:else if updateError}
          <div class="about-update-state about-update-state--fail" role="alert">
            <AppIcon name="x" size={14} />
            <span>{_t("about.checkFailed", { error: updateError })}</span>
          </div>
        {/if}
        {#if showUpdateDialog && updateResult}
          <UpdateDialog
            result={updateResult}
            mode={dialogMode}
            onclose={() => (showUpdateDialog = false)}
          />
        {/if}
      </div>
    {:else}
      {#if loading}
        <div class="settings-state">{_t("storage.readingConfig")}</div>
      {:else if status}
        <div class="settings-scroll">
          {#if activeSection === "storage_paths"}
            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
              <span class="setting-label">{_t("storage.currentProfile")}</span>
              <span class="config-path">{relativePath(status!.configPath)}</span>
              <button
                type="button"
                class="open-btn"
                onclick={() => invoke("open_external_url", { url: status!.configPath })}
              >
                <AppIcon name="file" size={14} />
                {_t("storage.open")}
              </button>
            </section>

            <section class="setting-card">
              <div class="setting-heading">
                <span class="setting-icon"><AppIcon name="file" size={17} /></span>
                <div>
                  <strong>
                    {_t("storage.dataDirectoryTitle")}
                    <span class:custom={status.usesCustomDataDirectory} class="inline-badge">
                      {status.usesCustomDataDirectory
                        ? _t("storage.custom")
                        : _t("storage.default")}
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
                  <span>{_t("storage.nextLaunch")}</span>
                  <code title={pending.storagePath}>{pending.storagePath}</code>
                  {#if restartNeeded}
                    <button class="restart-btn" type="button" onclick={restartApp}
                      >{_t("storage.restartNow")}</button
                    >
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
                  onclick={restoreDefaultResourceStoragePaths}
                  >{_t("storage.restoreDefault")}</button
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
              <pre>{_t("storage.directoryTree")}</pre>
            </section>
          {/if}
          {#if activeSection === "storage_tools"}
            <section class="setting-card">
              <div class="setting-heading">
                <span class="setting-icon"><AppIcon name="download" size={17} /></span>
                <div>
                  <strong>{_t("storage.transferTitle")}</strong>
                  <p>{_t("storage.transferDesc")}</p>
                </div>
              </div>
              <div class="transfer-actions">
                <div class="transfer-group">
                  <span class="transfer-label">{_t("storage.exportLabel")}</span>
                  <CustomSelect
                    value={exportFormat}
                    disabled={exporting || importing || exportFormats.length === 0}
                    ariaLabel={_t("storage.exportLabel")}
                    options={exportFormats.map((format) => ({
                      value: format.id,
                      label: format.label,
                    }))}
                    onchange={(v) => (exportFormat = v as string)}
                  />
                  <button
                    type="button"
                    class="settings-action-btn"
                    disabled={exporting || importing || exportFormats.length === 0}
                    onclick={handleExport}
                  >
                    {exporting ? _t("storage.exporting") : _t("storage.exportAction")}
                  </button>
                </div>
                <div class="transfer-group">
                  <span class="transfer-label">{_t("storage.importLabel")}</span>
                  <CustomSelect
                    value={importFormat}
                    disabled={exporting || importing || importFormats.length === 0}
                    ariaLabel={_t("storage.importLabel")}
                    options={importFormats.map((format) => ({
                      value: format.id,
                      label: format.label,
                    }))}
                    onchange={(v) => (importFormat = v as string)}
                  />
                  <button
                    type="button"
                    class="settings-action-btn"
                    disabled={exporting || importing || importFormats.length === 0}
                    onclick={handleImport}
                  >
                    {importing ? _t("storage.importing") : _t("storage.importAction")}
                  </button>
                </div>
              </div>
              {#if showLimitWarning}
                <div class="transfer-limit-warning">
                  <span
                    >{_t("storage.importTruncationWarning", {
                      max: maxItemCount,
                      count: importTruncationCount,
                    })}</span
                  >
                  <button
                    type="button"
                    class="settings-action-btn"
                    onclick={() => (activeSection = "storage_limits")}
                  >
                    {_t("storage.importAdjustLimit")}
                  </button>
                </div>
              {/if}
              <div class="export-options">
                <div class="export-option-row">
                  <span class="export-option-label">{_t("storage.exportFavorites")}</span>
                  <label class="export-check">
                    <Checkbox
                      checked={exportIncludeFavorites}
                      onchange={(checked) => (exportIncludeFavorites = checked)}
                      size={15}
                    />
                    <span>{_t("storage.exportIncludeFavorites")}</span>
                  </label>
                </div>
                <div class="export-option-row">
                  <span class="export-option-label">{_t("storage.exportContentTypes")}</span>
                  <div class="export-kind-checks">
                    {#each storageKinds as kindInfo (kindInfo.kind)}
                      <label class="export-check">
                        <Checkbox
                          checked={exportContentTypes.has(kindInfo.kind)}
                          onchange={() => toggleExportContentType(kindInfo.kind)}
                          size={15}
                        />
                        <span>{_t(kindInfo.labelKey)}</span>
                      </label>
                    {/each}
                  </div>
                </div>
                <div class="export-option-row export-date-row">
                  <span class="export-option-label">{_t("storage.exportDateRange")}</span>
                  <DatePicker
                    value={exportDateFrom}
                    onchange={(v) => (exportDateFrom = v)}
                    ariaLabel={_t("storage.exportDateFrom")}
                  />
                  <span class="export-date-separator">�C</span>
                  <DatePicker
                    value={exportDateTo}
                    onchange={(v) => (exportDateTo = v)}
                    ariaLabel={_t("storage.exportDateTo")}
                  />
                </div>
              </div>
            </section>

            <section class="setting-card toggle-card">
              <div class="setting-heading">
                <span class="setting-icon"><AppIcon name="search" size={17} /></span>
                <div>
                  <strong>{_t("storage.searchIndexTitle")}</strong>
                  <p>{_t("storage.searchIndexDesc")}</p>
                </div>
              </div>
              <button
                type="button"
                class="settings-action-btn"
                disabled={rebuilding}
                onclick={rebuildIndex}
              >
                {rebuilding ? _t("storage.rebuilding") : _t("storage.rebuildIndex")}
              </button>
            </section>
          {/if}
          {#if activeSection === "storage_limits"}
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
              <span class="number-suffix">{_t("storage.recordCountUnit")}</span>
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
              <CustomSelect
                className="unit-select"
                value={maxFileCopySizeUnit}
                options={[
                  { value: "byte", label: "B" },
                  { value: "KB", label: "KB" },
                  { value: "MB", label: "MB" },
                  { value: "GB", label: "GB" },
                ]}
                onchange={(v) => changeFileSizeUnit(v as "byte" | "KB" | "MB" | "GB")}
              />
            </section>

            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="text" size={17} /></span>
              <span class="setting-label">{_t("general.maxTextCaptureSize")}</span>
              <input
                type="number"
                bind:value={maxTextCaptureDisplay}
                min="1"
                oninput={updateMaxTextCaptureFromDisplay}
                onchange={saveMaxTextCaptureSize}
              />
              <CustomSelect
                className="unit-select"
                value={maxTextCaptureSizeUnit}
                options={[
                  { value: "byte", label: "B" },
                  { value: "KB", label: "KB" },
                  { value: "MB", label: "MB" },
                  { value: "GB", label: "GB" },
                ]}
                onchange={(v) => changeTextCaptureUnit(v as "byte" | "KB" | "MB" | "GB")}
              />
            </section>

            <section class="setting-card storage-kind-delete-card">
              <div class="setting-heading">
                <span class="setting-icon"><AppIcon name="trash" size={17} /></span>
                <div>
                  <strong>{_t("storage.deleteByKindTitle")}</strong>
                  <p>{_t("storage.deleteByKindDesc")}</p>
                </div>
              </div>
              <div class="storage-kind-delete-list">
                {#each storageKinds as entry (entry.kind)}
                  <div class="storage-kind-delete-row">
                    <span class="storage-kind-icon"><AppIcon name={entry.icon} size={15} /></span>
                    <div class="storage-kind-delete-copy">
                      <strong>{_t(entry.labelKey)}</strong>
                      <span>
                        {storageKindStatsAvailable
                          ? _t("storage.deleteKindCount", {
                              count: storageKindStats[entry.kind].itemCount,
                              size: formatBytes(storageKindStats[entry.kind].sizeBytes),
                            })
                          : "��"}
                      </span>
                    </div>
                    <button
                      type="button"
                      class="danger-action"
                      disabled={!storageKindStatsAvailable ||
                        deletingStorageKind !== null ||
                        storageKindStats[entry.kind].itemCount === 0}
                      onclick={() => deleteStorageKind(entry.kind)}
                    >
                      {deletingStorageKind === entry.kind
                        ? _t("storage.deletingKind")
                        : _t("storage.deleteKindAction")}
                    </button>
                  </div>
                {/each}
              </div>
              <p class="storage-kind-delete-scope">{_t("storage.deleteByKindScope")}</p>
            </section>
          {/if}
          {#if activeSection === "storage_tools"}
            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
              <span class="setting-label">{_t("storage.databaseMaintenance")}</span>
              <button type="button" disabled={repairLoading} onclick={doRepair}>
                {repairLoading ? _t("storage.checkingDatabase") : _t("storage.checkDatabase")}
              </button>
            </section>
            {#if repairResult}
              <div class="repair-result">
                <span class:ok={repairResult.integrityOk} class:fail={!repairResult.integrityOk}>
                  {repairResult.integrityOk
                    ? _t("storage.integrityOk")
                    : _t("storage.integrityProblem")}
                </span>
                <code>{repairResult.integrityMessage}</code>
              </div>
            {/if}
          {/if}
          {#if activeSection === "sync_cloud"}
            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="cloud" size={17} /></span>
              <span class="setting-label">{_t("storage.syncProvider")}</span>
              <CustomSelect
                value={syncProvider}
                ariaLabel={_t("storage.syncProvider")}
                options={[
                  { value: "off", label: _t("storage.syncProviderOff") },
                  { value: "webdav", label: _t("storage.syncProviderWebdav") },
                  { value: "s3", label: _t("storage.syncProviderS3") },
                ]}
                onchange={(v) => (syncProvider = v as string)}
              />
            </section>

            {#if syncProvider !== "off"}
              <section class="setting-card">
                <div class="setting-heading">
                  <span class="setting-icon"><AppIcon name="upload" size={17} /></span>
                  <div style="flex:1">
                    <strong>{_t("storage.syncNow")}</strong>
                    <p
                      style="margin:2px 0 0;font-size:var(--settings-description-size,var(--font-size-secondary,11px));color:var(--text-muted)"
                    >
                      {_t("storage.syncPendingCount", { count: syncUnsyncedCount })}{#if syncLastMs}
                        | {_t("storage.syncLastTime", {
                          time: new Date(syncLastMs).toLocaleString(),
                        })}{/if}
                    </p>
                  </div>
                  <button
                    type="button"
                    class="settings-action-btn"
                    disabled={syncing || syncTesting}
                    onclick={handleSyncUpload}
                  >
                    {syncing ? _t("storage.syncing") : _t("storage.syncNow")}
                  </button>
                </div>
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
                    void saveSyncSettings();
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
                    max="3600"
                    onblur={saveSyncSettings}
                  />
                  <span class="number-suffix">{_t("storage.syncSecondsUnit")}</span>
                </section>
              {/if}

              {#if syncProvider === "webdav"}
                <section class="setting-card">
                  <div class="setting-heading">
                    <span class="setting-icon"><AppIcon name="globe" size={17} /></span>
                    <div>
                      <strong>{_t("storage.syncEndpoint")}</strong>
                    </div>
                  </div>
                  <div class="setting-row">
                    <label for="sync-endpoint">{_t("storage.syncEndpoint")}</label>
                    <input
                      id="sync-endpoint"
                      type="url"
                      bind:value={syncEndpoint}
                      placeholder="https://dav.example.com/remote.php/dav/"
                      onblur={saveSyncConfig}
                    />
                  </div>
                  <div class="setting-row">
                    <label for="sync-remote-path">{_t("storage.syncRemotePath")}</label>
                    <input
                      id="sync-remote-path"
                      type="text"
                      bind:value={syncRemotePath}
                      placeholder="clipboard-backup"
                      onblur={saveSyncConfig}
                    />
                  </div>
                  <div class="setting-row">
                    <label for="sync-username">{_t("storage.syncUsername")}</label>
                    <input
                      id="sync-username"
                      type="text"
                      bind:value={syncUsername}
                      placeholder=""
                      onblur={saveSyncConfig}
                    />
                  </div>
                  <div class="setting-row">
                    <label for="sync-password">{_t("storage.syncPassword")}</label>
                    <input
                      id="sync-password"
                      type="password"
                      bind:value={syncPassword}
                      placeholder=""
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
                      <span class="sync-last-info">
                        {syncTestResult.message}
                      </span>
                    {/if}
                  </div>
                </section>
              {/if}

              {#if syncProvider === "s3"}
                <section class="setting-card">
                  <div class="setting-heading">
                    <span class="setting-icon"><AppIcon name="cloud" size={17} /></span>
                    <div>
                      <strong>{_t("storage.syncS3Title")}</strong>
                    </div>
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
                      placeholder="my-clipboard-backup"
                      onblur={saveSyncConfig}
                    />
                  </div>
                  <div class="setting-row">
                    <label for="sync-s3-access-key">{_t("storage.syncS3AccessKey")}</label>
                    <input
                      id="sync-s3-access-key"
                      type="text"
                      bind:value={syncS3AccessKey}
                      placeholder=""
                      onblur={saveSyncConfig}
                    />
                  </div>
                  <div class="setting-row">
                    <label for="sync-s3-secret-key">{_t("storage.syncS3SecretKey")}</label>
                    <input
                      id="sync-s3-secret-key"
                      type="password"
                      bind:value={syncS3SecretKey}
                      placeholder=""
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
                      <span class="sync-last-info">
                        {syncTestResult.message}
                      </span>
                    {/if}
                  </div>
                </section>
              {/if}

              <section class="setting-card setting-card-row">
                <span class="setting-icon"><AppIcon name="lock" size={17} /></span>
                <span class="setting-label">{_t("storage.syncEncryption")}</span>
                <input
                  type="password"
                  bind:value={syncEncryptPassword}
                  placeholder={_t("storage.syncEncryptionPlaceholder")}
                  onblur={saveSyncSettings}
                  style="flex:1;min-width:0"
                />
              </section>

              <section class="setting-card">
                <div class="setting-heading">
                  <span class="setting-icon"><AppIcon name="download" size={17} /></span>
                  <div style="flex:1">
                    <strong>{_t("storage.syncBackupList")}</strong>
                  </div>
                  <button
                    type="button"
                    class="settings-action-btn"
                    disabled={syncListing || syncDownloading}
                    onclick={handleListBackups}
                  >
                    {_t("storage.syncRefreshList")}
                  </button>
                </div>
                {#if syncBackups.length > 0}
                  <div class="storage-kind-delete-list">
                    {#each syncBackups as backup (backup.name)}
                      <div class="storage-kind-delete-row">
                        <span class="storage-kind-icon"><AppIcon name="file" size={15} /></span>
                        <div class="storage-kind-delete-copy">
                          <strong>{backup.name}</strong>
                          {#if backup.sizeBytes != null}
                            <span>{formatBytes(backup.sizeBytes)}</span>
                          {/if}
                        </div>
                        <button
                          type="button"
                          class="settings-action-btn"
                          disabled={syncDownloading}
                          onclick={() => handleDownloadBackup(backup.name)}
                        >
                          {_t("storage.syncDownload")}
                        </button>
                      </div>
                    {/each}
                  </div>
                {/if}
              </section>
            {/if}
          {/if}

          {#if activeSection === "sync_advanced"}
            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="file" size={17} /></span>
              <span class="setting-label">{_t("storage.syncRolloverEntries")}</span>
              <input
                type="number"
                bind:value={syncRolloverEntries}
                min="10"
                max="10000"
                onblur={saveSyncSettings}
              />
              <span class="number-suffix">{_t("storage.syncEntriesUnit")}</span>
            </section>

            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="file" size={17} /></span>
              <span class="setting-label">{_t("storage.syncRolloverBytes")}</span>
              <input
                type="number"
                bind:value={syncRolloverBytes}
                min="1024"
                max="1048576"
                onblur={saveSyncSettings}
              />
              <span class="number-suffix">{_t("storage.syncBytesUnit")}</span>
            </section>

            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="image" size={17} /></span>
              <span class="setting-label">{_t("storage.syncMaxImageBytes")}</span>
              <input
                type="number"
                bind:value={syncMaxImageBytes}
                min="0"
                max="1073741824"
                onblur={saveSyncSettings}
              />
              <span class="number-suffix">{_t("storage.syncBytesUnit")}</span>
            </section>

            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="file" size={17} /></span>
              <span class="setting-label">{_t("storage.syncMaxFileBytes")}</span>
              <input
                type="number"
                bind:value={syncMaxFileBytes}
                min="0"
                max="1073741824"
                onblur={saveSyncSettings}
              />
              <span class="number-suffix">{_t("storage.syncBytesUnit")}</span>
            </section>

            <section class="setting-card setting-card-row">
              <span class="setting-icon"><AppIcon name="trash" size={17} /></span>
              <span class="setting-label">{_t("storage.syncMaxFiles")}</span>
              <input
                type="number"
                bind:value={syncMaxOplogFiles}
                min="3"
                max="100"
                onblur={saveSyncSettings}
              />
              <span class="number-suffix">{_t("storage.syncFilesUnit")}</span>
            </section>
          {/if}

          <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
        </div>
      {:else}
        <div class="settings-state">{feedback || _t("storage.storageUnavailable")}</div>
      {/if}

      {#if feedback && status}
        <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
      {/if}
    {/if}
  </div>

  {#if replaceTarget}
    <div class="icon-replace-modal" role="presentation" onpointerdown={closeReplaceDialog}>
      <div
        class="icon-replace-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={_t("storage.iconReplaceTitle")}
        tabindex="-1"
        onpointerdown={(e) => e.stopPropagation()}
      >
        <div class="icon-replace-header">
          <strong>{_t("storage.iconReplaceFor", { name: replaceTarget.displayName })}</strong>
          <button
            type="button"
            class="icon-replace-close"
            aria-label={_t("storage.iconReplaceClose")}
            onclick={closeReplaceDialog}
          >
            <AppIcon name="x" size={16} />
          </button>
        </div>
        <p class="icon-replace-hint">{_t("storage.iconReplaceHint")}</p>
        <div class="icon-replace-grid">
          {#each iconReplaceOptions as entry (entry.contentHash)}
            <button
              type="button"
              class="icon-replace-option"
              class:selected={selectedExistingIcon === entry.iconName}
              onclick={() => (selectedExistingIcon = entry.iconName)}
            >
              <span class="icon-replace-thumb">
                {#if entry.iconName}
                  <img
                    src={convertFileSrc(
                      `${status?.iconsDir ?? ""}/${entry.iconName}`.replace(/\\/g, "/"),
                    )}
                    alt=""
                    onerror={(e) => {
                      (e.target as HTMLImageElement).style.display = "none";
                    }}
                  />
                {/if}
              </span>
              <span class="icon-replace-name" title={entry.iconName!}>
                {entry.iconName!.replace(/\.[^.]+$/, "")}
              </span>
            </button>
          {/each}
        </div>
        <div class="icon-replace-footer">
          <button
            type="button"
            class="icon-replace-file-btn"
            disabled={replacingIcon}
            onclick={chooseReplaceFile}
          >
            {_t("storage.chooseFile")}
          </button>
          <button
            type="button"
            class="icon-replace-confirm-btn"
            disabled={!selectedExistingIcon || replacingIcon}
            onclick={confirmExistingIcon}
          >
            {_t("storage.confirmReplace")}
          </button>
        </div>
      </div>
    </div>
  {/if}
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
    border: 1px solid var(--border-color);
    border-radius: 13px;
    background: var(--bg-settings);
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
    border-right: 1px solid var(--border-subtle);
    background: var(--surface-bg);
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
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    background: var(--hover-bg);
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
    color: var(--text-faint);
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
    color: var(--text-muted);
    background: var(--input-bg);
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
    color: var(--text-secondary);
    background: var(--hover-bg);
    border-color: var(--border-color);
  }

  .settings-primary-nav button.active {
    border-color: var(--selection-color);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--selection-color) 15%, var(--hover-bg));
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
    color: var(--text-faint);
    font-size: var(--settings-description-size);
  }

  .sidebar-usage {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }

  .sidebar-usage strong {
    color: var(--text-muted);
    font-weight: 560;
  }

  .sidebar-usage-bar {
    overflow: hidden;
    height: 4px;
    border-radius: 999px;
    background: var(--hover-bg);
  }

  .sidebar-usage-bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width 200ms ease;
  }

  .sidebar-usage-caption {
    color: var(--text-faint);
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
    color: var(--text-muted);
    font-size: var(--settings-description-size);
    font-weight: 400;
    line-height: 1.5;
  }

  .settings-section-header {
    flex: 0 0 auto;
    padding: 13px 18px 12px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .settings-section-heading-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .settings-section-actions {
    display: flex;
    align-items: center;
    flex: 0 0 auto;
    gap: 10px;
  }

  .settings-section-description {
    max-width: 430px;
    margin: 7px 0 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
    line-height: 1.5;
  }

  .settings-subnav {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 7px 18px 0;
    background: var(--bg-settings);
  }

  .settings-section-header .settings-subnav {
    padding: 7px 0 0;
  }

  .settings-subnav button {
    min-height: 28px;
    padding: 5px 12px;
    border: 1px solid transparent;
    border-radius: var(--settings-control-radius);
    color: var(--text-muted);
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
    border-color: var(--border-color);
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .settings-subnav button.active {
    border-color: var(--selection-color);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }

  .settings-subnav--single {
    min-height: 28px;
    padding-top: 7px;
  }

  .settings-subnav--tags {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-top: 0;
    min-height: 0;
  }

  .settings-tag-search {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    width: 260px;
    padding: 0 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-faint);
    background: var(--input-bg);
  }

  .settings-tag-search input {
    min-width: 0;
    flex: 1;
    border: 0;
    outline: 0;
    color: var(--text-primary);
    background: transparent;
    font: inherit;
    font-size: var(--settings-control-size);
  }

  .settings-section-title {
    color: var(--text-primary);
    font-size: var(--settings-heading-size);
    font-weight: 560;
    line-height: 1.35;
  }

  .settings-sidebar-search {
    display: block;
    margin: 0 0 12px;
  }

  .settings-search-field {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
    padding: 0 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-muted);
    background: var(--input-bg);
    transition:
      border-color 100ms ease,
      background 100ms ease;
  }

  .settings-search-field:focus-within {
    border-color: var(--text-faint);
    background: var(--hover-bg);
  }

  .settings-search-field input {
    width: 100%;
    min-width: 0;
    padding: 7px 0;
    border: 0;
    outline: none;
    color: var(--text-primary);
    background: transparent;
    font: inherit;
    font-size: var(--settings-control-size);
  }

  .settings-search-field input::placeholder {
    color: var(--placeholder-color);
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
    color: var(--text-muted);
    background: transparent;
    font-size: 16px;
    line-height: 1;
  }

  .settings-search-clear:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .settings-count {
    min-width: 0;
    color: var(--text-muted);
    font-size: var(--settings-note-size);
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }

  .settings-search-empty {
    margin: 0;
    padding: 9px 10px;
    border: 1px dashed var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-muted);
    background: color-mix(in srgb, var(--card-bg) 72%, transparent);
    font-size: var(--settings-description-size);
    text-align: center;
  }

  .settings-search-results {
    align-content: start;
  }

  .settings-search-result {
    display: grid;
    gap: 3px;
    width: 100%;
    min-width: 0;
    padding: 11px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius);
    color: var(--text-primary);
    background: var(--card-bg);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      border-color 100ms ease,
      background 100ms ease;
  }

  .settings-search-result:hover,
  .settings-search-result:focus-visible {
    border-color: var(--selection-color);
    outline: none;
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }

  .settings-search-result-path {
    overflow: hidden;
    color: var(--text-muted);
    font-size: var(--settings-note-size);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .settings-search-result strong {
    overflow: hidden;
    font-size: var(--settings-heading-size);
    font-weight: 560;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .settings-search-result p {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
    line-height: 1.45;
  }

  :global(.settings-search-target-highlight) {
    border-color: var(--selection-color) !important;
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--selection-color) 55%, transparent),
      0 0 0 4px color-mix(in srgb, var(--selection-color) 12%, transparent) !important;
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
    color: var(--text-muted);
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
    border: 1px solid var(--border-color);
    border-radius: var(--settings-close-radius);
    color: var(--text-muted);
    background: var(--card-bg);
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
  }

  .setting-card {
    padding: 10px 13px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius);
    background: var(--card-bg);
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
    color: var(--text-primary);
    font-size: var(--settings-heading-size);
    font-weight: 560;
  }

  .setting-heading p {
    margin-top: 2px;
    font-size: var(--settings-description-size);
  }

  :global(.ocr-engine-select) {
    flex: 1;
    max-width: 180px;
  }

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
    width: 100%;
    box-sizing: border-box;
    margin-top: 12px;
    padding: 0;
    border: 0;
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: var(--hover-bg);
    outline: none;
    cursor: pointer;
  }

  .transparency-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      var(--selection-color) 0%,
      var(--selection-color) var(--slider-pct, 50%),
      var(--hover-bg) var(--slider-pct, 50%),
      var(--hover-bg) 100%
    );
  }

  .transparency-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border-radius: 50%;
    border: 2px solid var(--selection-color);
    background: var(--input-bg);
    cursor: pointer;
    transition:
      box-shadow 100ms ease,
      transform 100ms ease;
  }

  .transparency-slider::-webkit-slider-thumb:hover {
    box-shadow: 0 0 6px color-mix(in srgb, var(--selection-color) 40%, transparent);
    transform: scale(1.15);
  }

  .transparency-slider::-moz-range-track {
    height: 4px;
    border-radius: 2px;
    background: var(--hover-bg);
  }

  .transparency-slider::-moz-range-progress {
    height: 4px;
    border-radius: 2px;
    background: var(--selection-color);
  }

  .transparency-slider::-moz-range-thumb {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--selection-color);
    background: var(--input-bg);
    cursor: pointer;
    transition:
      box-shadow 100ms ease,
      transform 100ms ease;
  }

  .transparency-slider::-moz-range-thumb:hover {
    box-shadow: 0 0 6px color-mix(in srgb, var(--selection-color) 40%, transparent);
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

  @media (max-width: 760px) {
    .ocr-stat-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  .pending-path code {
    display: block;
    overflow: hidden;
    color: var(--text-secondary);
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .inline-badge {
    display: inline-block;
    margin-left: 8px;
    padding: 2px 7px;
    border: 1px solid var(--border-color);
    border-radius: 999px;
    color: var(--text-muted);
    font-size: var(--settings-note-size);
    font-weight: 500;
    vertical-align: middle;
  }

  .inline-badge.custom {
    border-color: color-mix(in srgb, var(--selection-color) 36%, transparent);
    color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 12%, transparent);
  }

  label {
    display: block;
    margin: 12px 0 6px;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    outline: none;
    color: var(--text-primary);
    background: var(--input-bg);
    font:
      12px "Cascadia Code",
      "SFMono-Regular",
      Consolas,
      monospace;
    transition: border-color 120ms ease;
  }

  input:focus {
    border-color: var(--text-faint);
  }

  .pending-path {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding-top: 9px;
    border-top: 1px solid var(--border-subtle);
    color: var(--text-faint);
    font-size: var(--settings-description-size);
  }

  .pending-path code {
    font-size: var(--settings-description-size);
  }

  .directory-tree-card pre {
    margin: 11px 0 0;
    padding: 10px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    color: var(--text-muted);
    background: var(--input-bg);
    font:
      11px/1.55 "Cascadia Code",
      "SFMono-Regular",
      Consolas,
      monospace;
  }

  .settings-action-btn {
    height: 34px;
    box-sizing: border-box;
    padding: 5px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .about-update-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 0 auto;
  }

  .about-update-controls :global(.settings-select) {
    height: 34px;
  }

  .about-path {
    overflow-wrap: anywhere;
    word-break: break-all;
  }

  .settings-action-btn:hover {
    color: var(--text-primary);
  }

  .settings-action-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .icon-select-all {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    line-height: 32px;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .icon-file-count {
    font-size: 11px;
    line-height: 32px;
    margin-top: 6px;
    color: var(--text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .icon-file-list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 300px;
    overflow-y: auto;
  }

  .icon-table-header,
  .icon-file-item {
    display: grid;
    grid-template-columns: 16px 1fr 40px 70px minmax(90px, auto);
    column-gap: 10px;
    align-items: center;
  }

  .icon-table-header {
    padding: 6px 14px 6px 12px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--input-bg);
    font-size: 11px;
    color: var(--text-muted);
    text-transform: none;
  }

  .icon-table-header .icon-col-app {
    color: var(--text-muted);
  }

  .icon-file-item {
    border-bottom: 1px solid var(--border-subtle);
    padding: 6px 16px 6px 12px;
    font-size: 12px;
  }

  .icon-file-item:last-child {
    border-bottom: none;
  }

  .icon-file-item:hover {
    background: var(--hover-bg);
  }

  .icon-col-check {
    display: flex;
    align-items: center;
  }

  .row-check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    cursor: pointer;
    user-select: none;
  }

  .icon-col-app {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .icon-app-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .icon-orphan-mark {
    flex-shrink: 0;
    color: var(--danger-color, #e5484d);
    font-weight: 600;
  }

  .icon-preview {
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    border-radius: 4px;
    object-fit: contain;
  }

  .icon-letter {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    border-radius: 5px;
    background: color-mix(in srgb, var(--selection-color) 18%, transparent);
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 600;
  }

  .icon-col-icon {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .icon-col-size {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    text-align: right;
  }

  .icon-col-action {
    display: flex;
    justify-content: center;
  }

  .icon-replace-btn {
    height: 26px;
    box-sizing: border-box;
    padding: 0 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .icon-replace-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .icon-replace-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .icon-replace-modal {
    position: fixed;
    z-index: 60;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 16px;
    background: rgba(5, 5, 5, 0.6);
    backdrop-filter: blur(4px);
  }

  .icon-replace-dialog {
    width: min(520px, 100%);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border-color);
    border-radius: 9px;
    background: var(--surface-bg);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }

  .icon-replace-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .icon-replace-header strong {
    font-size: var(--settings-heading-size, 13px);
  }

  .icon-replace-close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
  }

  .icon-replace-close:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .icon-replace-hint {
    margin: 0;
    padding: 10px 16px 0;
    font-size: var(--settings-description-size, 11px);
    color: var(--text-muted);
  }

  .icon-replace-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(84px, 1fr));
    gap: 8px;
    padding: 12px 16px;
    overflow-y: auto;
  }

  .icon-replace-option {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 8px 6px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-secondary);
    background: var(--hover-bg);
    cursor: pointer;
  }

  .icon-replace-option:hover {
    color: var(--text-primary);
  }

  .icon-replace-option.selected {
    border-color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 14%, transparent);
  }

  .icon-replace-thumb {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
  }

  .icon-replace-thumb img {
    width: 28px;
    height: 28px;
    object-fit: contain;
  }

  .icon-replace-name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--settings-description-size, 11px);
  }

  .icon-replace-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 12px 16px;
    border-top: 1px solid var(--border-subtle);
  }

  .icon-replace-footer button {
    height: 32px;
    box-sizing: border-box;
    padding: 0 14px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    font: inherit;
    font-size: var(--settings-control-size, 11px);
    cursor: pointer;
  }

  .icon-replace-file-btn {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .icon-replace-confirm-btn {
    color: #fff;
    background: var(--selection-color);
    border-color: var(--selection-color);
  }

  .icon-replace-footer button:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .icon-replace-footer button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .icon-actions {
    padding: 8px 14px 8px 12px;
    border-top: 1px solid var(--border-subtle);
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .icon-actions-spacer {
    flex: 1;
  }

  .icon-actions button {
    height: 32px;
    box-sizing: border-box;
    padding: 5px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
    white-space: nowrap;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .icon-actions button:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .icon-actions button:disabled {
    opacity: 0.55;
    cursor: default;
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

  .number-suffix {
    color: var(--text-muted);
    font-size: var(--settings-description-size);
    flex-shrink: 0;
  }

  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .settings-feedback {
    position: absolute;
    right: 18px;
    bottom: 13px;
    left: 18px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--danger-color) 35%, transparent);
    border-radius: var(--settings-feedback-radius);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, var(--surface-bg));
    font-size: var(--settings-feedback-size);
  }

  .settings-feedback.success {
    border-color: color-mix(in srgb, var(--success-color) 35%, transparent);
    color: color-mix(in srgb, var(--success-color) 75%, white);
    background: color-mix(in srgb, var(--success-color) 12%, var(--surface-bg));
  }

  .about-update-state {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    padding: 7px 10px;
    border: 1px solid color-mix(in srgb, var(--success-color) 35%, transparent);
    border-radius: var(--settings-control-radius);
    color: color-mix(in srgb, var(--success-color) 75%, white);
    background: color-mix(in srgb, var(--success-color) 12%, var(--surface-bg));
    font-size: var(--settings-description-size);
  }

  .about-update-state--fail {
    border-color: color-mix(in srgb, var(--danger-color) 35%, transparent);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, var(--surface-bg));
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
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-control-radius);
    background: var(--input-bg);
    font-size: var(--settings-description-size);
  }

  .repair-result span.ok {
    color: color-mix(in srgb, var(--success-color) 75%, white);
  }

  .repair-result span.fail {
    color: color-mix(in srgb, var(--danger-color) 75%, white);
  }

  .repair-result code {
    display: block;
    margin-top: 4px;
    color: var(--text-secondary);
    font-size: var(--settings-note-size);
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: var(--text-faint);
    font-size: var(--settings-note-size);
    text-align: center;
  }

  .storage-kind-delete-list {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
    margin-top: 12px;
  }

  .storage-kind-delete-row {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
    padding: 9px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-control-radius);
    background: var(--input-bg);
  }

  .storage-kind-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 27px;
    height: 27px;
    flex-shrink: 0;
    border-radius: var(--settings-icon-radius);
    color: var(--text-muted);
    background: var(--hover-bg);
  }

  .storage-kind-delete-copy {
    display: grid;
    min-width: 0;
    flex: 1;
    gap: 2px;
  }

  .storage-kind-delete-copy strong {
    color: var(--text-primary);
    font-size: var(--settings-heading-size);
    font-weight: 560;
  }

  .storage-kind-delete-copy span,
  .storage-kind-delete-scope {
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .storage-kind-delete-scope {
    margin: 9px 0 0;
    line-height: 1.45;
  }

  .danger-action {
    min-width: 68px;
    padding: 6px 9px;
    border: 1px solid color-mix(in srgb, var(--danger-color) 35%, transparent);
    border-radius: var(--settings-control-radius);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, var(--surface-bg));
    font: inherit;
    font-size: var(--settings-control-size);
    white-space: nowrap;
  }

  .danger-action:hover:not(:disabled) {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--danger-color) 35%, var(--surface-bg));
  }

  .danger-action:disabled {
    opacity: 0.45;
    cursor: default;
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
    color: var(--text-primary);
    font-size: var(--settings-heading-size);
    font-weight: 560;
  }

  .setting-card-row input {
    height: 34px;
    box-sizing: border-box;
    width: 100px;
    flex-shrink: 0;
    padding: 7px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-primary);
    background: var(--input-bg);
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

  .setting-card-row button:not(.toggle-switch) {
    height: 34px;
    box-sizing: border-box;
    padding: 5px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: var(--settings-control-size);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  :global(.setting-card-row .settings-select) {
    height: 34px;
    box-sizing: border-box;
  }

  .setting-card-row button:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .setting-card-row button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .setting-card-row input:focus {
    border-color: var(--text-faint);
  }

  .setting-card-row .number-suffix {
    color: var(--text-muted);
    font-size: var(--settings-description-size);
    flex-shrink: 0;
  }

  .config-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text-muted);
    font-size: var(--settings-note-size);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .open-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-muted);
    background: var(--card-bg);
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
    color: var(--text-primary);
    background: var(--hover-bg);
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
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-primary);
    background: var(--input-bg);
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: var(--settings-control-size);
    outline: none;
    transition: border-color 120ms ease;
  }

  .dir-input-row input:focus {
    border-color: var(--text-faint);
  }

  .dir-input-row button {
    padding: 7px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
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
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .dir-input-row button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .transfer-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 12px;
  }

  .transfer-group {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .transfer-label {
    flex: 0 0 auto;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }

  :global(.transfer-group .custom-select) {
    flex-shrink: 0;
  }

  :global(.transfer-group .settings-select) {
    width: 140px;
    height: 34px;
  }

  .transfer-limit-warning {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 12px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--warning-color) 35%, transparent);
    border-radius: var(--settings-feedback-radius, 7px);
    color: color-mix(in srgb, var(--warning-color) 75%, white);
    background: color-mix(in srgb, var(--warning-color) 12%, var(--surface-bg));
    font-size: var(--settings-feedback-size, var(--font-size-secondary, 11px));
  }

  .transfer-limit-warning button {
    flex-shrink: 0;
  }

  .sync-last-info {
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }

  .setting-actions-row {
    margin-top: 10px;
  }

  .export-options {
    display: flex;
    flex-direction: column;
    gap: 9px;
    margin-top: 12px;
    padding-top: 11px;
    border-top: 1px solid var(--border-subtle);
  }

  .export-option-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .export-option-row.export-date-row {
    flex-wrap: nowrap;
  }

  .export-option-label {
    flex: 0 0 auto;
    min-width: 64px;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }

  .export-kind-checks {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
  }

  .export-check {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    color: var(--text-secondary);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    user-select: none;
  }

  .export-date-separator {
    flex-shrink: 0;
    color: var(--text-faint);
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
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .resource-path-actions span {
    flex: 1;
    min-width: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .resource-path-warning {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    margin-top: 10px;
    padding: 9px 10px;
    border: 1px solid color-mix(in srgb, var(--warning-color) 35%, transparent);
    border-radius: var(--settings-card-radius);
    color: color-mix(in srgb, var(--warning-color) 75%, white);
    background: color-mix(in srgb, var(--warning-color) 12%, var(--surface-bg));
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
    border-top: 1px solid var(--border-subtle);
  }

  .resource-path-summary code {
    min-width: 0;
    overflow: hidden;
    color: var(--text-secondary);
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    font-size: var(--settings-description-size);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .resource-path-summary .restart-btn {
    grid-column: 2;
    grid-row: 1 / span 2;
  }

  :global(.unit-select) {
    width: 64px;
  }

  :global(.unit-select .settings-select) {
    justify-content: center;
    padding-right: 10px;
    text-align: center;
  }

  .restart-btn {
    margin-left: auto;
    padding: 5px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    color: var(--text-secondary);
    background: var(--hover-bg);
    font-size: var(--settings-control-size);
    cursor: pointer;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .restart-btn:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
    border-color: var(--text-faint);
  }

  .config-bar-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-muted);
    background: var(--card-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    white-space: nowrap;
  }

  .config-bar-btn:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }
</style>
