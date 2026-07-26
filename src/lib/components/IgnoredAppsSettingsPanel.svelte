<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import {
    configureIgnoredApplications,
    getApplicationFilterSettings,
    type ApplicationFilterSettings,
    type DiscoveredApplication,
  } from "$lib/services/capture";
  import { messages, resolvePath } from "$lib/i18n";
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { isTauriRuntime } from "$lib/services/runtime";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  function appIconUrl(iconFileName: string | null | undefined): string | undefined {
    if (!iconFileName || !isTauriRuntime()) return undefined;
    if (!iconsDir) return undefined;
    const fullPath = `${iconsDir}/${iconFileName}`.replace(/\\/g, "/");
    return convertFileSrc(fullPath);
  }

  interface Props {
    iconsDir?: string;
    onclose: () => void;
    showHeader?: boolean;
  }

  let { iconsDir = "", onclose, showHeader = true }: Props = $props();
  let settings = $state<ApplicationFilterSettings | null>(null);
  let availableSearch = $state("");
  let ignoredSearch = $state("");
  let selectedAvailable = $state<string[]>([]);
  let manualApplication = $state("");
  let loading = $state(true);
  let saving = $state(false);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let privacyPaused = $state(false);
  let privacyLoading = $state(true);

  const ignoredKeys = $derived(
    new Set((settings?.ignoredApplications ?? []).map(normalizeApplication)),
  );
  const appIconMap = $derived(
    new Map(
      (settings?.discoveredApplicationsWithIcons ?? []).map((app) => [app.name, app.iconPath]),
    ),
  );
  const availableApplications = $derived(
    (settings?.discoveredApplications ?? []).filter(
      (application) => !ignoredKeys.has(normalizeApplication(application)),
    ),
  );
  const visibleAvailable = $derived(filterApplications(availableApplications, availableSearch));
  const visibleIgnored = $derived(
    filterApplications(settings?.ignoredApplications ?? [], ignoredSearch),
  );

  onMount(() => {
    void loadSettings();
    void loadPrivacyStatus();
  });

  async function loadPrivacyStatus() {
    if (!isTauriRuntime()) {
      privacyLoading = false;
      return;
    }

    try {
      const status = await invoke<{ paused: boolean }>("get_privacy_status");
      privacyPaused = status.paused;
    } catch (error) {
      console.error("Unable to load privacy status", error);
    } finally {
      privacyLoading = false;
    }
  }

  async function togglePrivacyPause() {
    if (!isTauriRuntime() || privacyLoading) return;
    privacyLoading = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      privacyPaused = await invoke<boolean>("toggle_privacy_pause");
      feedback = _t(privacyPaused ? "capture.paused" : "capture.resumed");
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to toggle privacy pause", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      privacyLoading = false;
    }
  }

  async function loadSettings() {
    loading = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      settings = await getApplicationFilterSettings();
      if (!settings) feedback = _t("capture.browserUnavailable");
    } catch (error) {
      console.error("Unable to load application filters", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
    }
  }

  function toggleAvailable(application: string) {
    selectedAvailable = selectedAvailable.includes(application)
      ? selectedAvailable.filter((candidate) => candidate !== application)
      : [...selectedAvailable, application];
  }

  async function ignoreSelected() {
    if (!settings || selectedAvailable.length === 0) return;
    await saveIgnored([...settings.ignoredApplications, ...selectedAvailable]);
    selectedAvailable = [];
  }

  async function removeIgnored(application: string) {
    if (!settings) return;
    await saveIgnored(
      settings.ignoredApplications.filter((candidate) => candidate !== application),
    );
  }

  async function addManualApplication() {
    const application = manualApplication.trim();
    if (!settings || !application) return;
    await saveIgnored([...settings.ignoredApplications, application]);
    manualApplication = "";
  }

  async function saveIgnored(applications: string[]) {
    if (!settings) return;
    saving = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      const ignoredApplications = await configureIgnoredApplications(applications);
      settings = { ...settings, ignoredApplications };
      feedback = _t("capture.saved", { count: ignoredApplications.length });
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to save ignored applications", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }

  function filterApplications(applications: string[], query: string): string[] {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (!normalizedQuery) return applications;
    return applications.filter((application) =>
      application.toLocaleLowerCase().includes(normalizedQuery),
    );
  }

  function normalizeApplication(application: string): string {
    return application.trim().toLocaleLowerCase();
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("capture.settings")}</span>
      <h2>{_t("capture.title")}</h2>
      <p>{_t("capture.description")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

{#if loading}
  <div class="settings-state">{_t("capture.readingApps")}</div>
{:else if settings}
  <div class="settings-scroll">
    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="pause" size={17} /></span>
        <div>
          <strong>{_t("capture.pauseTitle")}</strong>
          <p>{_t("capture.pauseDescription")}</p>
        </div>
      </div>
      <div class="pause-control">
        <span class="pause-state">{_t(privacyPaused ? "capture.paused" : "capture.active")}</span>
        <button
          type="button"
          class="toggle-switch"
          class:active={!privacyPaused}
          role="switch"
          aria-checked={!privacyPaused}
          aria-label={_t(privacyPaused ? "capture.resumeAction" : "capture.pauseAction")}
          title={_t(privacyPaused ? "capture.resumeAction" : "capture.pauseAction")}
          disabled={privacyLoading || !isTauriRuntime()}
          onclick={togglePrivacyPause}
        >
          <span class="toggle-knob"></span>
        </button>
      </div>
    </section>

    <section class="filter-board">
      <div class="application-column">
        <div class="column-heading">
          <strong>{_t("capture.availableApps")} <span>{availableApplications.length}</span></strong>
          <button
            type="button"
            title={_t("capture.refreshApps")}
            aria-label={_t("capture.refreshApps")}
            onclick={loadSettings}>&#x21bb;</button
          >
        </div>
        <label class="search-field">
          <AppIcon name="search" size={15} />
          <input bind:value={availableSearch} placeholder={_t("capture.searchApps")} />
        </label>
        <div class="application-list">
          {#each visibleAvailable as application}
            <button
              type="button"
              class="application-row"
              class:selected={selectedAvailable.includes(application)}
              onclick={() => toggleAvailable(application)}
            >
              {#if appIconMap.get(application)}
                <img
                  class="app-icon"
                  src={appIconUrl(appIconMap.get(application))}
                  alt={application}
                />
              {:else}
                <span class="app-avatar">{application.slice(0, 1).toLocaleUpperCase()}</span>
              {/if}
              <strong>{application}</strong>
            </button>
          {:else}
            <p class="empty-list">{_t("capture.noAppsFound")}</p>
          {/each}
        </div>
      </div>

      <div class="transfer-column">
        <button
          type="button"
          aria-label={_t("capture.ignoreSelected")}
          title={_t("capture.ignoreSelected")}
          disabled={saving || selectedAvailable.length === 0}
          onclick={ignoreSelected}>&#x2192;</button
        >
      </div>

      <div class="application-column">
        <div class="column-heading">
          <strong
            >{_t("capture.ignoredApps")} <span>{settings.ignoredApplications.length}</span></strong
          >
          <span class="plus-mark">+</span>
        </div>
        <label class="search-field">
          <AppIcon name="search" size={15} />
          <input bind:value={ignoredSearch} placeholder={_t("capture.searchIgnored")} />
        </label>
        <div class="manual-add">
          <input
            bind:value={manualApplication}
            placeholder={_t("capture.addManual")}
            onkeydown={(event) => event.key === "Enter" && addManualApplication()}
          />
          <button
            type="button"
            disabled={saving || !manualApplication.trim()}
            onclick={addManualApplication}>{_t("capture.add")}</button
          >
        </div>
        <div class="application-list">
          {#each visibleIgnored as application}
            <div class="application-row ignored-row">
              {#if appIconMap.get(application)}
                <img
                  class="app-icon"
                  src={appIconUrl(appIconMap.get(application))}
                  alt={application}
                />
              {:else}
                <span class="app-avatar locked">{application.slice(0, 1).toLocaleUpperCase()}</span>
              {/if}
              <strong>{application}</strong>
              <button
                type="button"
                aria-label={_t("capture.removeIgnored", { app: application })}
                title={_t("capture.moveOut")}
                disabled={saving}
                onclick={() => removeIgnored(application)}>×</button
              >
            </div>
          {:else}
            <p class="empty-list">{_t("capture.noIgnoredMatch")}</p>
          {/each}
        </div>
      </div>
    </section>

    <p class="auto-save-note">{_t("capture.configNote")}</p>
  </div>
{:else}
  <div class="settings-state">{feedback || _t("capture.captureUnavailable")}</div>
{/if}

{#if feedback && settings}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 20px 22px 15px;
    border-bottom: 1px solid var(--border-subtle);
  }
  .eyebrow {
    color: var(--text-muted);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  h2 {
    margin: 5px 0 4px;
    color: var(--text-primary);
    font-size: var(--settings-page-title-size, 18px);
    font-weight: 590;
  }
  header p {
    max-width: 570px;
    margin: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
    line-height: 1.5;
  }
  .close-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--settings-close-size, 28px);
    height: var(--settings-close-size, 28px);
    flex: 0 0 auto;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-close-radius, 7px);
    color: var(--text-muted);
    background: var(--card-bg);
    font-size: var(--settings-close-font-size, 19px);
    line-height: 1;
  }
  .settings-scroll {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex: 1;
    min-height: 0;
    padding: 14px 18px 48px;
    overflow: auto;
    scrollbar-color: var(--scrollbar-color) transparent;
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
    background: var(--scrollbar-color);
  }
  .setting-card {
    padding: 10px 13px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius, 9px);
    background: var(--card-bg);
  }

  .toggle-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .setting-heading {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1;
  }

  .setting-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 29px;
    height: 29px;
    flex: 0 0 auto;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-icon-radius, 7px);
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .setting-heading strong {
    display: block;
    color: var(--text-primary);
    font-size: var(--settings-heading-size, 13px);
    font-weight: 560;
  }

  .setting-heading p {
    margin: 2px 0 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
    line-height: 1.45;
  }

  .pause-control {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 0 0 auto;
  }

  .pause-state {
    color: var(--text-muted);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  .toggle-switch {
    position: relative;
    width: 40px;
    height: 22px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 12px;
    background: var(--input-bg);
    transition: background 120ms ease;
  }

  .toggle-switch.active {
    border-color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 18%, transparent);
  }

  .toggle-switch:disabled {
    opacity: 0.5;
  }

  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text-faint);
    transition: transform 120ms ease;
  }

  .toggle-switch.active .toggle-knob {
    transform: translateX(18px);
    background: var(--selection-color);
  }

  .filter-board {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 30px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    min-height: 365px;
    flex: 1;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius, 9px);
    background: var(--surface-bg);
    overflow: hidden;
  }
  .application-column {
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 13px;
  }

  .application-column + .transfer-column,
  .transfer-column + .application-column {
    border-left: 1px solid var(--border-subtle);
  }
  .column-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 25px;
    color: var(--text-primary);
    font-size: var(--settings-heading-size, 13px);
  }
  .column-heading strong span {
    margin-left: 4px;
    padding: 2px 6px;
    border: 1px solid var(--border-color);
    border-radius: 999px;
    color: var(--text-muted);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    font-weight: 500;
  }
  .column-heading button,
  .plus-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    color: var(--text-muted);
    background: transparent;
    font-size: var(--font-size-base, 14px);
  }
  .search-field {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 9px 0;
    padding: 7px 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-faint);
    background: var(--input-bg);
  }
  .search-field input,
  .manual-add input {
    min-width: 0;
    flex: 1;
    border: 0;
    outline: 0;
    color: var(--text-primary);
    background: transparent;
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }
  .manual-add {
    display: flex;
    gap: 6px;
    margin-bottom: 8px;
    padding: 6px 7px 6px 9px;
    border: 1px dashed var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--input-bg);
  }
  .manual-add button {
    border: 0;
    border-radius: var(--settings-control-radius, 6px);
    padding: 4px 7px;
    color: var(--text-secondary);
    background: var(--hover-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }
  .application-list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--scrollbar-color) transparent;
  }
  .application-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 36px;
    padding: 3px 5px;
    border: 0;
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-secondary);
    background: transparent;
    font-size: var(--settings-control-size, 11px);
    cursor: pointer;
  }
  .application-row + .application-row {
    margin-top: 2px;
  }
  .application-row:hover {
    background: var(--hover-bg);
  }
  .application-row.selected {
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--selection-color) 50%, transparent);
  }
  .application-row.ignored-row {
    cursor: default;
  }
  .application-row.ignored-row:hover {
    background: var(--hover-bg);
  }
  .application-row strong {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .application-row > button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    margin-left: auto;
    border: 0;
    color: var(--text-faint);
    background: transparent;
    font-size: var(--font-size-base, 14px);
  }
  .app-avatar {
    display: inline-grid;
    width: 25px;
    height: 25px;
    flex: 0 0 auto;
    place-items: center;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-icon-radius, 7px);
    color: var(--text-primary);
    background: var(--hover-bg);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    font-weight: 700;
  }
  .app-avatar.locked {
    color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 15%, var(--hover-bg));
  }
  .app-icon {
    width: 25px;
    height: 25px;
    flex: 0 0 auto;
    border-radius: var(--settings-icon-radius, 7px);
    object-fit: contain;
  }
  .transfer-column {
    display: grid;
    place-items: center;
  }
  .transfer-column button {
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: 50%;
    color: var(--text-muted);
    background: transparent;
    font-size: var(--font-size-base, 14px);
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .transfer-column button:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.35;
  }
  .empty-list {
    margin: 16px 6px;
    color: var(--text-faint);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    text-align: center;
  }
  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: var(--text-faint);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    text-align: right;
  }
  .settings-feedback {
    position: absolute;
    right: 18px;
    bottom: 13px;
    left: 18px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--danger-color) 35%, transparent);
    border-radius: var(--settings-feedback-radius, 7px);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, var(--surface-bg));
    font-size: var(--settings-feedback-size, var(--font-size-secondary, 11px));
  }
  .settings-feedback.success {
    border-color: color-mix(in srgb, var(--success-color) 35%, transparent);
    color: color-mix(in srgb, var(--success-color) 75%, white);
    background: color-mix(in srgb, var(--success-color) 12%, var(--surface-bg));
  }
  @media (max-width: 700px) {
    .filter-board {
      grid-template-columns: 1fr;
    }
    .transfer-column {
      min-height: 38px;
      border-top: 1px solid var(--border-subtle);
      border-left: 0 !important;
    }
    .transfer-column button {
      transform: rotate(90deg);
    }
    .transfer-column + .application-column {
      border-top: 1px solid var(--border-subtle);
      border-left: 0;
    }
  }
</style>
