<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import { generalSettings } from "$lib/services/settings";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import Checkbox from "$lib/components/Checkbox.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import DatePicker from "$lib/components/DatePicker.svelte";
  import GeneralSettingsPanel from "$lib/components/GeneralSettingsPanel.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { resetKeyboardConfig } from "$lib/services/keyboard";
  import { listen } from "@tauri-apps/api/event";
  import {
    configureStorageDirectory,
    getStorageKindStats,
    getStorageConfig,
    getStorageStatus,
    permanentlyDeleteStorageKind,
    rebuildSearchIndex,
    repairDatabase,
    setResourceStoragePaths,
    validateSearchIndex,
    type StorageDirectoryUpdate,
    type StorageKind,
    type StorageKindStats,
    type StorageStatus,
    type RepairResult,
  } from "$lib/services/storage";
  import { isTauriRuntime, getRuntimeInfo } from "$lib/services/runtime";
  import {
    exportToFile,
    getExportFormats,
    getImportFormats,
    importFromFile,
    getSyncConfig,
    setSyncConfig,
    testSyncConnection,
    runSync,
    type ExportFormatInfo,
    type ImportFormatInfo,
    type SyncConfig,
  } from "$lib/services/storage";
  import { getVersion } from "@tauri-apps/api/app";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    SETTINGS_NAV_GROUP_DEFINITIONS,
    resolveSettingsNavPath,
    type SettingsNavGroupId,
    type SettingsSection,
    type StatisticsTab,
  } from "$lib/settings-navigation";
  import type { IconName } from "$lib/types/clipboard";
  import { formatBytes, updateSliderTrack } from "$lib/utils/format";
  import { endOfDay, startOfDay } from "$lib/utils/date-query";
  import {
    filterSettingsSearchItems,
    normalizeSettingsSearch,
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

  interface SettingsNavTarget {
    section: SettingsSection;
    statisticsTab?: StatisticsTab;
    label: string;
    title: string;
    description: string;
  }

  interface SettingsNavGroup {
    id: SettingsNavGroupId;
    icon: IconName;
    label: string;
    ariaLabel: string;
    preserveTabOnPrimary: boolean;
    tabs: SettingsNavTarget[];
  }

  let compactSettingsPanelModule:
    Promise<typeof import("$lib/components/CompactSettingsPanel.svelte")> | undefined;
  let fontSizeSettingsPanelModule:
    Promise<typeof import("$lib/components/FontSizeSettingsPanel.svelte")> | undefined;
  let themeSettingsPanelModule:
    Promise<typeof import("$lib/components/ThemeSettingsPanel.svelte")> | undefined;
  let iconColorsSettingsPanelModule:
    Promise<typeof import("$lib/components/IconColorsSettingsPanel.svelte")> | undefined;
  let ignoredAppsSettingsPanelModule:
    Promise<typeof import("$lib/components/IgnoredAppsSettingsPanel.svelte")> | undefined;
  let tagManagementSettingsPanelModule:
    Promise<typeof import("$lib/components/TagManagementSettingsPanel.svelte")> | undefined;
  let keyboardSettingsPanelModule:
    Promise<typeof import("$lib/components/KeyboardSettingsPanel.svelte")> | undefined;
  let statisticsSettingsPanelModule:
    Promise<typeof import("$lib/components/StatisticsSettingsPanel.svelte")> | undefined;
  let aboutSettingsPanelModule:
    Promise<typeof import("$lib/components/AboutSettingsPanel.svelte")> | undefined;
  let iconCacheSettingsPanelModule:
    Promise<typeof import("$lib/components/IconCacheSettingsPanel.svelte")> | undefined;

  function loadCompactSettingsPanel() {
    return (compactSettingsPanelModule ??= import("$lib/components/CompactSettingsPanel.svelte"));
  }

  function loadFontSizeSettingsPanel() {
    return (fontSizeSettingsPanelModule ??= import("$lib/components/FontSizeSettingsPanel.svelte"));
  }

  function loadThemeSettingsPanel() {
    return (themeSettingsPanelModule ??= import("$lib/components/ThemeSettingsPanel.svelte"));
  }

  function loadIconColorsSettingsPanel() {
    return (iconColorsSettingsPanelModule ??=
      import("$lib/components/IconColorsSettingsPanel.svelte"));
  }

  function loadIgnoredAppsSettingsPanel() {
    return (ignoredAppsSettingsPanelModule ??=
      import("$lib/components/IgnoredAppsSettingsPanel.svelte"));
  }

  function loadTagManagementSettingsPanel() {
    return (tagManagementSettingsPanelModule ??=
      import("$lib/components/TagManagementSettingsPanel.svelte"));
  }

  function loadKeyboardSettingsPanel() {
    return (keyboardSettingsPanelModule ??= import("$lib/components/KeyboardSettingsPanel.svelte"));
  }

  function loadStatisticsSettingsPanel() {
    return (statisticsSettingsPanelModule ??=
      import("$lib/components/StatisticsSettingsPanel.svelte"));
  }

  function loadAboutSettingsPanel() {
    return (aboutSettingsPanelModule ??= import("$lib/components/AboutSettingsPanel.svelte"));
  }

  function loadIconCacheSettingsPanel() {
    return (iconCacheSettingsPanelModule ??=
      import("$lib/components/IconCacheSettingsPanel.svelte"));
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
  let activeSection = $state<SettingsSection>("general_general");
  let activeStatisticsTab = $state<StatisticsTab>("storage");
  let keyboardResetToken = $state(0);

  async function handleResetKeyboard() {
    try {
      await resetKeyboardConfig();
      keyboardResetToken++;
    } catch {
      /* ignore */
    }
  }

  const settingsNavGroups = $derived.by((): SettingsNavGroup[] =>
    SETTINGS_NAV_GROUP_DEFINITIONS.map((group) => ({
      id: group.id,
      icon: group.icon,
      label: group.displayLabel ?? _t(group.labelKey),
      ariaLabel: _t(group.ariaLabelKey ?? group.labelKey),
      preserveTabOnPrimary: group.preserveTabOnPrimary ?? false,
      tabs: group.tabs.map((tab) => ({
        section: tab.section,
        statisticsTab: tab.statisticsTab,
        label: _t(tab.labelKey),
        title: _t(tab.titleKey ?? tab.labelKey),
        description: tab.descriptionKey ? _t(tab.descriptionKey) : "",
      })),
    })),
  );

  function isSettingsNavTargetActive(target: SettingsNavTarget): boolean {
    return (
      activeSection === target.section &&
      (target.statisticsTab === undefined || activeStatisticsTab === target.statisticsTab)
    );
  }

  function activateSettingsNavTarget(target: SettingsNavTarget): void {
    activeSection = target.section;
    if (target.statisticsTab !== undefined) activeStatisticsTab = target.statisticsTab;
  }

  function activateSettingsNavGroup(group: SettingsNavGroup): void {
    const target = group.tabs[0];
    if (group.preserveTabOnPrimary) {
      activeSection = target.section;
      return;
    }
    activateSettingsNavTarget(target);
  }

  const activeSettingsNavGroup = $derived.by(() =>
    settingsNavGroups.find((group) => group.tabs.some(isSettingsNavTargetActive)),
  );
  const activeSettingsNavTarget = $derived.by(() =>
    activeSettingsNavGroup?.tabs.find(isSettingsNavTargetActive),
  );
  const settingsSectionMeta = $derived(
    activeSettingsNavTarget
      ? {
          title: activeSettingsNavTarget.title,
          desc: activeSettingsNavTarget.description,
        }
      : undefined,
  );

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

  function storageKindLabel(kind: StorageKind): string {
    return _t(storageKinds.find((entry) => entry.kind === kind)?.labelKey ?? "filter.text");
  }

  let repairResult = $state<RepairResult | null>(null);
  let repairLoading = $state(false);

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
    feedback = "";
    try {
      await persistSyncConfig();
      const result = await runSync();
      if (
        result.uploadedEntries === 0 &&
        result.downloadedEntries === 0 &&
        result.deletedRemoteObjects === 0
      ) {
        feedback = _t("storage.syncNoChanges");
        feedbackSuccess = true;
      } else {
        feedback = _t("storage.syncRunSummary", {
          uploaded: String(result.uploadedEntries),
          downloaded: String(result.downloadedEntries),
          applied: String(result.appliedEntries),
          bytesUp: (result.bytesUploaded / 1024).toFixed(1),
          bytesDown: (result.bytesDownloaded / 1024).toFixed(1),
        });
        feedbackSuccess = true;
      }
      syncLastMs = Date.now();
      syncStatus = "success";
      syncPendingEntries = 0;
    } catch (e) {
      feedback = _t("storage.syncRunFailed") + `: ${String(e)}`;
      feedbackSuccess = false;
      syncStatus = "failed";
    } finally {
      syncing = false;
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
  }

  $effect(() => {
    if (open) {
      void loadStatus();
      void loadExportFormats();
      void loadImportFormats();
      void loadAppVersion();
    }
  });

  $effect(() => {
    if (!open) return;

    if (activeSection !== "storage_limits") return;
    const refreshVisibleStatistics = refreshStorageStats;

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
    if (!open || activeSection !== "ocr") return;

    void loadOcrStatus();
    const interval = setInterval(() => void loadOcrTaskStatus(), 2000);
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

    void loadHistoryConfig();
  }

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

  function saveMaxTextCaptureSize() {
    generalSettings.updateSetting("maxTextCaptureBytes", maxTextCaptureSize);
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

{#snippet loadingSettingsPanel()}
  <div class="settings-state" role="status">{_t("storage.loadingSettingsPanel")}</div>
{/snippet}

{#snippet settingsPanelLoadFailed()}
  <div class="settings-state" role="alert">{_t("storage.settingsPanelLoadFailed")}</div>
{/snippet}

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
      {#each settingsNavGroups as group (group.id)}
        <button
          class:active={group.tabs.some(isSettingsNavTargetActive)}
          type="button"
          onclick={() => activateSettingsNavGroup(group)}
        >
          <AppIcon name={group.icon} size={16} />
          <span>{group.label}</span>
        </button>
      {/each}
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
      {#if activeSettingsNavGroup?.id === "keyboard"}
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
      {#if activeSettingsNavGroup && activeSettingsNavGroup.tabs.length > 1}
        <nav class="settings-subnav" aria-label={activeSettingsNavGroup.ariaLabel}>
          {#each activeSettingsNavGroup.tabs as tab (`${tab.section}:${tab.statisticsTab ?? ""}`)}
            <button
              type="button"
              class:active={isSettingsNavTargetActive(tab)}
              aria-current={isSettingsNavTargetActive(tab) ? "page" : undefined}
              onclick={() => activateSettingsNavTarget(tab)}
            >
              {tab.label}
            </button>
          {/each}
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
      {#await loadCompactSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel {onclose} showHeader={false} />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else if activeSection === "font"}
      {#await loadFontSizeSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel {onclose} showHeader={false} />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else if activeSection === "theme"}
      {#await loadThemeSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel {onclose} showHeader={false} />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else if activeSection === "icons"}
      {#await loadIconColorsSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel {onclose} showHeader={false} />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else if activeSection === "capture"}
      {#await loadIgnoredAppsSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel iconsDir={status?.iconsDir} {onclose} showHeader={false} />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else if activeSection === "capture_icons"}
      {#await loadIconCacheSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel
          iconsDir={status?.iconsDir ?? ""}
          onfeedback={(message, success) => {
            feedback = message;
            feedbackSuccess = success;
          }}
          {onclose}
        />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else if activeSection === "tags"}
      {#await loadTagManagementSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel
          {onclose}
          showHeader={false}
          {tagSearch}
          ontagSearchChange={(v) => (tagSearch = v)}
        />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else if activeSection === "keyboard_item" || activeSection === "keyboard_quick" || activeSection === "keyboard_system"}
      {#await loadKeyboardSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel
          {onclose}
          resetToken={keyboardResetToken}
          category={activeSection === "keyboard_item"
            ? "item"
            : activeSection === "keyboard_quick"
              ? "quick"
              : "system"}
          showHeader={false}
        />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
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
      {#await loadStatisticsSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel
          activeTab={activeStatisticsTab}
          {status}
          {loading}
          onrefreshStatus={refreshStorageStats}
          {onclose}
        />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else if activeSection === "about"}
      {#await loadAboutSettingsPanel()}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel {appVersion} {appExecutablePath} {onclose} />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
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
                  style="flex:1;min-width:0"
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
                  <div style="flex:1">
                    <strong>{_t("storage.syncNow")}</strong>
                    <p
                      style="margin:2px 0 0;font-size:var(--settings-description-size,var(--font-size-secondary,11px));color:var(--text-muted)"
                    >
                      {_t("storage.syncPendingCount", {
                        count: syncPendingEntries,
                      })}{#if syncLastMs}
                        | {_t("storage.syncLastTime", {
                          time: new Date(syncLastMs).toLocaleString(),
                        })}{/if}{#if syncStatus}
                        | {syncStatus === "success"
                          ? _t("storage.syncStatusSuccess")
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
          {/if}

          {#if activeSection === "sync_advanced"}
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

  .settings-brand {
    display: flex;
    align-items: center;
  }

  .settings-brand {
    gap: 10px;
    padding: 2px 5px 18px;
  }

  .brand-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    background: var(--hover-bg);
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
    line-height: 1.5;
  }

  .setting-card {
    padding: 10px 13px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius);
    background: var(--card-bg);
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
