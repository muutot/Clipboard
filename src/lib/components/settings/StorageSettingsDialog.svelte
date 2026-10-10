<script lang="ts">
  import { tick } from "svelte";
  import { generalSettings } from "$lib/services/settings";
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import SearchField from "$lib/components/common/SearchField.svelte";
  import { getStorageStatus, type StorageStatus } from "$lib/services/storage";
  import { isTauriRuntime, getRuntimeInfo } from "$lib/services/runtime";
  import { getVersion } from "@tauri-apps/api/app";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    SETTINGS_NAV_GROUP_DEFINITIONS,
    type FontSubsection,
    type SettingsNavGroupId,
    type SettingsSection,
    type StatisticsTab,
  } from "$lib/settings-navigation";
  import type { IconName } from "$lib/types/clipboard";
  import { formatBytes } from "$lib/utils/content/format";
  import { captureFocusRestore, trapTabFocus } from "$lib/utils/keyboard/focus";
  import { createLazyPanelRegistry } from "./lazy-panels";
  import { createSettingsSearchController } from "./settings-search-controller.svelte";

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
  let activeStatisticsTab = $state<StatisticsTab>("storage");
  // The font panel splits interface vs. card sliders. The dialog owns the
  // selected sub-tab so a settings-search jump can land on the right one.
  let activeFontSection = $state<FontSubsection>("interface");

  // Panel routing is a registry in `lazy-panels.ts`; hand it the live state the
  // descriptors' render-time props builders read and write.
  const lazyPanels = createLazyPanelRegistry({
    get onclose() {
      return onclose;
    },
    get status() {
      return status;
    },
    get activeSection() {
      return activeSection;
    },
    get activeStatisticsTab() {
      return activeStatisticsTab;
    },
    get activeFontSection() {
      return activeFontSection;
    },
    get tagSearch() {
      return tagSearch;
    },
    get loading() {
      return loading;
    },
    get appVersion() {
      return appVersion;
    },
    get appExecutablePath() {
      return appExecutablePath;
    },
    setActiveSection: (section) => (activeSection = section),
    setFontSection: (section) => (activeFontSection = section),
    setTagSearch: (value) => (tagSearch = value),
    showFeedback: (message, success) => {
      feedback = message;
      feedbackSuccess = success;
    },
    refreshStorageStats,
  });

  /** Descriptor for the current section when it is a lazily imported panel.
   *  Sections that additionally require loaded data fall back to the shell
   *  until that data exists (e.g. storage_paths needs `status`). */
  const lazyPanel = $derived(
    lazyPanels.descriptors.find(
      (entry) =>
        entry.sections.includes(activeSection) && (activeSection !== "storage_paths" || status),
    ),
  );

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
    // Reset the font sub-tab only when entering the section, so re-clicking
    // the active tab keeps the user's current sub-tab.
    if (target.section === "font" && activeSection !== "font") activeFontSection = "interface";
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
  // The dialog binds this scroll container; the search controller reads it.
  let settingsContent = $state<HTMLElement | null>(null);

  const settingsSearch = createSettingsSearchController({
    get contentEl() {
      return settingsContent;
    },
    get activeSection() {
      return activeSection;
    },
    get activeStatisticsTab() {
      return activeStatisticsTab;
    },
    translate: _t,
    selectSection: (section, statisticsTab) => {
      activeSection = section;
      if (statisticsTab !== undefined) activeStatisticsTab = statisticsTab;
    },
    selectFontSection: (section) => (activeFontSection = section),
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
        value={settingsSearch.query}
        oninput={(v) => (settingsSearch.query = v)}
        placeholder={_t("storage.settingsSearchPlaceholder")}
        ariaLabel={_t("storage.settingsSearchLabel")}
        id="settings-search-input"
        labelFor="settings-search-input"
        clearLabel={_t("storage.clearSettingsSearch")}
        onclear={settingsSearch.clear}
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
            {#if settingsSearch.active}
              {_t("storage.settingsFilteredCount", {
                matched: settingsSearch.results.length,
                total: settingsSearch.totalItems,
              })}
            {:else}
              {_t("storage.settingsCount", { count: settingsSearch.itemCount })}
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

    {#if settingsSearch.active}
      <div class="settings-scroll settings-search-results" aria-live="polite">
        {#if settingsSearch.results.length > 0}
          {#each settingsSearch.results as result (result.id)}
            <button
              type="button"
              class="settings-search-result"
              data-settings-search-id={result.id}
              onclick={() => void settingsSearch.openResult(result)}
            >
              <span class="settings-search-result-path">{settingsSearch.resultPath(result)}</span>
              <strong>{result.title}</strong>
              {#if result.description}
                <p>{result.description}</p>
              {/if}
            </button>
          {/each}
        {:else}
          <div class="settings-search-empty" role="status">
            {_t("storage.settingsSearchNoResults", { query: settingsSearch.query.trim() })}
          </div>
        {/if}
      </div>
    {:else if lazyPanel}
      {#await lazyPanels.load(activeSection)}
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
