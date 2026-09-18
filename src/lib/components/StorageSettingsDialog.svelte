<script lang="ts">
  import { tick } from "svelte";
  import type { Component } from "svelte";
  import { generalSettings } from "$lib/services/settings";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import SearchField from "$lib/components/SearchField.svelte";
  import { getStorageStatus, type StorageStatus } from "$lib/services/storage";
  import { isTauriRuntime, getRuntimeInfo } from "$lib/services/runtime";
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
  import { captureFocusRestore, trapTabFocus } from "$lib/utils/focus";
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
      sections: ["general_general"],
      load: () => import("$lib/components/GeneralGeneralSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["general_window"],
      load: () => import("$lib/components/GeneralWindowSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["general_search"],
      load: () => import("$lib/components/GeneralSearchSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["general_items"],
      load: () => import("$lib/components/GeneralItemsSettingsPanel.svelte"),
      props: () => ({ onclose, showHeader: false }),
    },
    {
      sections: ["layout"],
      load: () => import("$lib/components/LayoutSettingsPanel.svelte"),
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
      sections: ["tags_rules"],
      load: () => import("$lib/components/TagRulesSettingsPanel.svelte"),
      props: () => ({
        onclose,
        showHeader: false,
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
      sections: ["storage_limits"],
      load: () => import("$lib/components/StorageLimitsPanel.svelte"),
      props: () => ({
        onfeedback: (message: string, success: boolean) => {
          feedback = message;
          feedbackSuccess = success;
        },
      }),
    },
    {
      sections: ["storage_tools"],
      load: () => import("$lib/components/StorageToolsPanel.svelte"),
      props: () => ({
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
        tab: "cloud" as const,
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
        tab: "advanced" as const,
        onfeedback: (message: string, success: boolean) => {
          feedback = message;
          feedbackSuccess = success;
        },
      }),
    },
    {
      sections: ["sync_s3"],
      cacheKey: "sync",
      load: () => import("$lib/components/SyncPanel.svelte"),
      props: () => ({
        tab: "s3" as const,
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
        category: activeSection.startsWith("keyboard_")
          ? (activeSection.slice("keyboard_".length) as "item" | "quick" | "system" | "switch")
          : "item",
        showHeader: false,
        configPath: status?.keyboardConfigPath ?? null,
      }),
    },
    {
      sections: ["keyboard_float"],
      load: () => import("$lib/components/FloatPanelClickSettingsPanel.svelte"),
      props: () => ({
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
      // Evict on rejection: a cached rejected promise would make every later
      // navigation re-await the same failure, permanently breaking the panel
      // after one transient chunk-load error until the window restarts.
      promise.catch(() => lazyPanelModules.delete(cacheKey));
      lazyPanelModules.set(cacheKey, promise);
    }
    return promise;
  }

  let { open, onclose, standalone = false }: Props = $props();
  let status = $state<StorageStatus | null>(null);
  let loading = $state(false);
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

  const settingsBreadcrumb = $derived.by(() => {
    const group = activeSettingsNavGroup;
    if (!group) return activeSection;
    if (group.tabs.length === 1) return group.label;
    const target = activeSettingsNavTarget;
    return target ? `${group.label} / ${target.label}` : group.label;
  });
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

  let appVersion = $state("");
  let appExecutablePath = $state("");

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

  async function refreshStorageStats() {
    try {
      status = await getStorageStatus();
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
              onclick={onclose}
            >
              <AppIcon name="x" size={14} strokeWidth={2} />
            </button>
          {/if}
        </div>
      </div>
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
    {:else if lazyPanel}
      {#await loadLazyPanelModule(activeSection)}
        {@render loadingSettingsPanel()}
      {:then module}
        {#if lazyPanel}
          {@const Panel = module.default}
          <Panel {...lazyPanel.props()} />
        {:else}
          {@render loadingSettingsPanel()}
        {/if}
      {:catch}
        {@render settingsPanelLoadFailed()}
      {/await}
    {:else}
      <div class="settings-state">
        {loading ? _t("storage.readingConfig") : feedback || _t("storage.storageUnavailable")}
      </div>
    {/if}

    {#if feedback}
      <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
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

  @media (max-width: 560px) {
    .settings-dialog {
      grid-template-columns: 1fr;
    }
    .settings-sidebar {
      display: none;
    }
  }
</style>
