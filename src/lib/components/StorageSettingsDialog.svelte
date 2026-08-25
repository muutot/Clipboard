<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import type { Component } from "svelte";
  import { generalSettings } from "$lib/services/settings";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import SearchField from "$lib/components/SearchField.svelte";
  import GeneralSettingsPanel from "$lib/components/GeneralSettingsPanel.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { resetKeyboardConfig } from "$lib/services/keyboard";
  import { listen } from "@tauri-apps/api/event";
  import {
    getStorageKindStats,
    getStorageConfig,
    getStorageStatus,
    permanentlyDeleteStorageKind,
    rebuildSearchIndex,
    repairDatabase,
    type StorageKind,
    type StorageKindStats,
    type StorageStatus,
    type RepairResult,
  } from "$lib/services/storage";
  import { isTauriRuntime, getRuntimeInfo } from "$lib/services/runtime";
  import {} from "$lib/services/storage";
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
  import { formatBytes } from "$lib/utils/format";
  import { fromDisplaySize, toDisplaySize } from "$lib/utils/unit-convert";
  import { captureFocusRestore, trapTabFocus } from "$lib/utils/focus";
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

  // Lazily loaded panels share one homogenized dispatch: a descriptor maps
  // section ids onto a cached dynamic import plus a render-time props builder,
  // and the template awaits through a single generic block.
  type LazyPanelModule = { default: Component<any> };

  interface LazyPanelDescriptor {
    sections: readonly string[];
    load(): Promise<LazyPanelModule>;
    props(): Record<string, unknown>;
    /** Shared module-cache key when one panel serves several sections. */
    cacheKey?: string;
  }

  const lazyPanelModules = new Map<string, Promise<LazyPanelModule>>();

  const LAZY_PANEL_DESCRIPTORS: LazyPanelDescriptor[] = [
    {
      sections: ["compact"],
      load: () => import("$lib/components/CompactSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["font"],
      load: () => import("$lib/components/FontSizeSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["theme"],
      load: () => import("$lib/components/ThemeSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["icons"],
      load: () => import("$lib/components/IconColorsSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["capture"],
      load: () => import("$lib/components/IgnoredAppsSettingsPanel.svelte"),
      props: () => ({ iconsDir: status?.iconsDir, onclose, showHeader: false }),
    },
    {
      sections: ["capture_privacy"],
      load: () => import("$lib/components/SensitiveContentSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["capture_icons"],
      load: () => import("$lib/components/IconCacheSettingsPanel.svelte"),
      props: () => ({
        iconsDir: status?.iconsDir ?? "",
        onfeedback: (message: string, success: boolean) => {
          feedback = message;
          feedbackSuccess = success;
        },
      }),
    },
    {
      sections: ["tags"],
      load: () => import("$lib/components/TagManagementSettingsPanel.svelte"),
      props: () => ({
        onclose,
        showHeader: false,
        tagSearch,
        ontagSearchChange: (value: string) => (tagSearch = value),
      }),
    },
    {
      sections: ["ocr"],
      load: () => import("$lib/components/OcrSettingsPanel.svelte"),
      props: () => ({
        onfeedback: (message: string, success: boolean) => {
          feedback = message;
          feedbackSuccess = success;
        },
      }),
    },
    {
      sections: ["storage_paths"],
      load: () => import("$lib/components/StoragePathsPanel.svelte"),
      props: () => ({
        status: status as NonNullable<typeof status>,
        onfeedback: (message: string, success: boolean) => {
          feedback = message;
          feedbackSuccess = success;
        },
      }),
    },
    {
      sections: ["storage_tools"],
      load: () => import("$lib/components/TransferPanel.svelte"),
      props: () => ({
        maxItemCount,
        onfeedback: (message: string, success: boolean) => {
          feedback = message;
          feedbackSuccess = success;
        },
        onadjustlimit: () => (activeSection = "storage_limits"),
      }),
    },
    {
      sections: ["sync_cloud"],
      cacheKey: "sync",
      load: () => import("$lib/components/SyncPanel.svelte"),
      props: () => ({
        advanced: false,
        onfeedback: (message: string, success: boolean) => {
          feedback = message;
          feedbackSuccess = success;
        },
      }),
    },
    {
      sections: ["sync_advanced"],
      cacheKey: "sync",
      load: () => import("$lib/components/SyncPanel.svelte"),
      props: () => ({
        advanced: true,
        onfeedback: (message: string, success: boolean) => {
          feedback = message;
          feedbackSuccess = success;
        },
      }),
    },
    {
      sections: ["keyboard_item", "keyboard_quick", "keyboard_system", "keyboard_switch"],
      load: () => import("$lib/components/KeyboardSettingsPanel.svelte"),
      props: () => ({
        onclose,
        resetToken: keyboardResetToken,
        category: activeSection.startsWith("keyboard_")
          ? (activeSection.slice("keyboard_".length) as "item" | "quick" | "system" | "switch")
          : "item",
        showHeader: false,
      }),
    },
    {
      sections: ["statistics"],
      load: () => import("$lib/components/StatisticsSettingsPanel.svelte"),
      props: () => ({
        activeTab: activeStatisticsTab,
        status,
        loading,
        onrefreshStatus: refreshStorageStats,
        onclose,
      }),
    },
    {
      sections: ["about"],
      load: () => import("$lib/components/AboutSettingsPanel.svelte"),
      props: () => ({ appVersion, appExecutablePath, onclose }),
    },
  ];

  function loadLazyPanelModule(section: string): Promise<LazyPanelModule> {
    const descriptor = LAZY_PANEL_DESCRIPTORS.find((entry) => entry.sections.includes(section));
    if (!descriptor) return Promise.reject(new Error(`no panel for ${section}`));
    const cacheKey = descriptor.cacheKey ?? section;
    let promise = lazyPanelModules.get(cacheKey);
    if (!promise) {
      promise = descriptor.load();
      lazyPanelModules.set(cacheKey, promise);
    }
    return promise;
  }

  let { open, onclose, standalone = false }: Props = $props();
  let status = $state<StorageStatus | null>(null);
  let loading = $state(false);
  let saving = $state(false);
  let rebuilding = $state(false);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
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

  $effect(() => {
    if (feedback) {
      const t = setTimeout(() => {
        feedback = "";
      }, 2000);
      return () => clearTimeout(t);
    }
  });

  // Modal focus management: only the overlay variant traps focus; the
  // standalone window owns its whole document.
  let dialogEl = $state<HTMLElement | null>(null);

  $effect(() => {
    if (!open || standalone) return;
    const restoreFocus = captureFocusRestore();
    tick().then(() => {
      dialogEl?.focus();
    });
    return restoreFocus;
  });

  function handleDialogKeydown(event: KeyboardEvent) {
    if (!standalone) trapTabFocus(dialogEl, event);
  }
  let activeSection = $state<SettingsSection>("general_general");
  /** Descriptor for the current section when it is a lazily imported panel.
   *  Sections that additionally require loaded data fall back to the shell
   *  until that data exists (e.g. storage_paths needs `status`). */
  const lazyPanel = $derived(
    LAZY_PANEL_DESCRIPTORS.find(
      (entry) =>
        entry.sections.includes(activeSection) && (activeSection !== "storage_paths" || status),
    ),
  );
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

  function storageKindLabel(kind: StorageKind): string {
    return _t(storageKinds.find((entry) => entry.kind === kind)?.labelKey ?? "filter.text");
  }

  let repairResult = $state<RepairResult | null>(null);
  let repairLoading = $state(false);
  let appVersion = $state("");
  let appExecutablePath = $state("");

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

  async function loadStatus() {
    loading = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      status = await getStorageStatus();
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
        bind:this={dialogEl}
        onkeydowncapture={handleDialogKeydown}
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
      <SearchField
        value={settingsSearch}
        oninput={(v) => (settingsSearch = v)}
        placeholder={_t("storage.settingsSearchPlaceholder")}
        ariaLabel={_t("storage.settingsSearchLabel")}
        id="settings-search-input"
        labelFor="settings-search-input"
        clearLabel={_t("storage.clearSettingsSearch")}
        onclear={clearSettingsSearch}
        autocomplete="off"
        spellcheck={false}
        fill
        sidebar
      />
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
              onclick={onclose}>脳</button
            >
          {/if}
        </div>
      </div>
      {#if activeSettingsNavGroup?.id === "keyboard"}
        <section
          class="setting-card toggle-card keyboard-config-card"
          data-settings-search-id="keyboard.config-file"
        >
          <div class="setting-heading">
            <span class="setting-icon"><AppIcon name="keyboard" size={17} /></span>
            <div>
              <strong>{_t("keyboard.shortcutConfigTitle")}</strong>
              <p>{_t("storage.keyboardConfigNote")}</p>
            </div>
          </div>
          <div class="config-bar-actions">
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
    {:else if lazyPanel}
      {#await loadLazyPanelModule(activeSection)}
        {@render loadingSettingsPanel()}
      {:then module}
        {@const Panel = module.default}
        <Panel {...lazyPanel.props()} />
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else}
      {#if loading}
        <div class="settings-state">{_t("storage.readingConfig")}</div>
      {:else if status}
        <div class="settings-scroll">
          {#if activeSection === "storage_tools"}
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
                          : "—"}
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

  .setting-heading p {
    line-height: 1.5;
  }

  .setting-card {
    padding: 10px 13px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius);
    background: var(--card-bg);
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

  .setting-card-row input {
    width: 100px;
  }

  input:focus {
    border-color: var(--text-faint);
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

  .keyboard-config-card {
    margin-top: 3px;
  }

  .config-bar-actions {
    display: flex;
    flex-shrink: 0;
    gap: 6px;
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
