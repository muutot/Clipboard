<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath, locale } from "$lib/i18n";
  import type { Locale } from "$lib/i18n/types";
  import type {
    CardActionsDisplay,
    SearchSuggestionMode,
    WindowConfig,
  } from "$lib/types/clipboard";
  import { generalSettings, getWindowConfig, setWindowConfig } from "$lib/services/settings";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let s = $state($generalSettings);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let windowConfig = $state<WindowConfig | null>(null);
  let windowConfigLoading = $state(true);
  let windowConfigSaving = $state(false);

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  $effect(() => {
    let cancelled = false;
    void getWindowConfig()
      .then((config) => {
        if (!cancelled) windowConfig = config;
      })
      .catch(() => {
        if (!cancelled) showFeedback(_t("general.windowConfigLoadFailed"), false);
      })
      .finally(() => {
        if (!cancelled) windowConfigLoading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  function showFeedback(message: string, success: boolean) {
    feedback = message;
    feedbackSuccess = success;
    setTimeout(() => (feedback = ""), 2000);
  }

  function changeLanguage(lang: Locale) {
    generalSettings.updateSetting("language", lang);
    locale.set(lang);
    showFeedback(_t(lang === "zh-CN" ? "已切换至中文" : "Switched to English"), true);
  }

  function handleTransparency(event: Event) {
    const val = Number((event.target as HTMLInputElement).value);
    generalSettings.updateSetting("windowTransparency", val);
    updateSliderTrack(transparencyEl);
  }

  async function changeWindowSetting(key: "launchAtStartup" | "closeToTray", value: boolean) {
    if (!windowConfig || windowConfigSaving) return;
    const previous = windowConfig;
    windowConfig = { ...previous, [key]: value };
    windowConfigSaving = true;
    try {
      await setWindowConfig({ [key]: value });
    } catch {
      windowConfig = previous;
      showFeedback(_t("general.windowConfigUpdateFailed"), false);
    } finally {
      windowConfigSaving = false;
    }
  }

  function updateSliderTrack(el: HTMLInputElement | null) {
    if (!el) return;
    const pct = ((Number(el.value) - Number(el.min)) / (Number(el.max) - Number(el.min))) * 100;
    el.style.setProperty("--slider-pct", pct + "%");
  }

  let transparencyEl = $state<HTMLInputElement | null>(null);
  let viewerOpacityEl = $state<HTMLInputElement | null>(null);

  $effect(() => {
    updateSliderTrack(transparencyEl);
  });

  $effect(() => {
    updateSliderTrack(viewerOpacityEl);
  });
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("general.eyebrow")}</span>
      <h2>{_t("general.title")}</h2>
      <p>{_t("general.description")}</p>
    </div>
    {#if s.showSettingsCloseButton}
      <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
        >×</button
      >
    {/if}
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="globe" size={17} /></span>
      <div>
        <strong>{_t("general.language")}</strong>
        <p>{_t("general.languageDescription")}</p>
      </div>
    </div>
    <div class="lang-toggle">
      <button
        type="button"
        class:active={s.language === "zh-CN"}
        onclick={() => changeLanguage("zh-CN")}>中文</button
      >
      <button type="button" class:active={s.language === "en"} onclick={() => changeLanguage("en")}
        >English</button
      >
    </div>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="search" size={17} /></span>
      <div>
        <strong>{_t("general.searchSuggestionMode")}</strong>
        <p>{_t("general.searchSuggestionModeDescription")}</p>
      </div>
    </div>
    <select
      class="theme-select"
      value={s.searchSuggestionMode}
      aria-label={_t("general.searchSuggestionMode")}
      onchange={(e) =>
        generalSettings.updateSetting(
          "searchSuggestionMode",
          (e.target as HTMLSelectElement).value as SearchSuggestionMode,
        )}
    >
      <option value="off">{_t("general.searchSuggestionOff")}</option>
      <option value="panel">{_t("general.searchSuggestionPanel")}</option>
      <option value="inline">{_t("general.searchSuggestionInline")}</option>
    </select>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
      <div>
        <strong>{_t("general.cardActionsDisplay")}</strong>
        <p>{_t("general.cardActionsDisplayDescription")}</p>
      </div>
    </div>
    <select
      class="theme-select"
      value={s.cardActionsDisplay}
      aria-label={_t("general.cardActionsDisplay")}
      onchange={(e) =>
        generalSettings.updateSetting(
          "cardActionsDisplay",
          (e.target as HTMLSelectElement).value as CardActionsDisplay,
        )}
    >
      <option value="hover">{_t("general.cardActionsHover")}</option>
      <option value="always">{_t("general.cardActionsAlways")}</option>
    </select>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
      <div>
        <strong>{_t("general.searchHistory")}</strong>
        <p>{_t("general.searchHistoryDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.searchHistoryEnabled}
      onclick={() => generalSettings.updateSetting("searchHistoryEnabled", !s.searchHistoryEnabled)}
      aria-checked={s.searchHistoryEnabled}
      aria-label={_t("general.searchHistory")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
      <div>
        <strong>{_t("general.launchAtStartup")}</strong>
        <p>{_t("general.launchAtStartupDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={windowConfig?.launchAtStartup ?? false}
      onclick={() =>
        void changeWindowSetting("launchAtStartup", !(windowConfig?.launchAtStartup ?? false))}
      disabled={windowConfigLoading || windowConfigSaving || !windowConfig}
      aria-checked={windowConfig?.launchAtStartup ?? false}
      aria-label={_t("general.launchAtStartup")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="clipboard" size={17} /></span>
      <div>
        <strong>{_t("general.closeToTray")}</strong>
        <p>{_t("general.closeToTrayDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={windowConfig?.closeToTray ?? false}
      onclick={() => void changeWindowSetting("closeToTray", !(windowConfig?.closeToTray ?? false))}
      disabled={windowConfigLoading || windowConfigSaving || !windowConfig}
      aria-checked={windowConfig?.closeToTray ?? false}
      aria-label={_t("general.closeToTray")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="sliders" size={17} /></span>
      <div class="heading-inline">
        <strong>{_t("general.windowTransparency")}</strong>
        <span class="value-label">{s.windowTransparency}%</span>
      </div>
    </div>
    <input
      type="range"
      min="60"
      max="100"
      value={s.windowTransparency}
      oninput={handleTransparency}
      class="transparency-slider"
      bind:this={transparencyEl}
    />
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
      <div>
        <strong>{_t("general.compactMode")}</strong>
        <p>{_t("general.compactModeDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.compactMode}
      onclick={() => generalSettings.updateSetting("compactMode", !s.compactMode)}
      aria-checked={s.compactMode}
      aria-label={_t("general.compactMode")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="pin" size={17} /></span>
      <div>
        <strong>{_t("general.alwaysOnTop")}</strong>
        <p>{_t("general.alwaysOnTopDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.alwaysOnTop}
      onclick={() => generalSettings.updateSetting("alwaysOnTop", !s.alwaysOnTop)}
      aria-checked={s.alwaysOnTop}
      aria-label={_t("general.alwaysOnTop")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
      <div>
        <strong>{_t("general.pinCopiedToTop")}</strong>
        <p>{_t("general.pinCopiedToTopDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.pinCopiedToTop}
      onclick={() => generalSettings.updateSetting("pinCopiedToTop", !s.pinCopiedToTop)}
      aria-checked={s.pinCopiedToTop}
      aria-label={_t("general.pinCopiedToTop")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="trash" size={17} /></span>
      <div>
        <strong>{_t("general.useRecycleBin")}</strong>
        <p>{_t("general.useRecycleBinDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.useRecycleBin}
      onclick={() => generalSettings.updateSetting("useRecycleBin", !s.useRecycleBin)}
      aria-checked={s.useRecycleBin}
      aria-label={_t("general.useRecycleBin")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="info" size={17} /></span>
      <div>
        <strong>{_t("general.toastNotifications")}</strong>
        <p>{_t("general.toastNotificationsDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.showToastNotifications}
      onclick={() =>
        generalSettings.updateSetting("showToastNotifications", !s.showToastNotifications)}
      aria-checked={s.showToastNotifications}
      aria-label={_t("general.toastNotifications")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="copy" size={17} /></span>
      <div>
        <strong>{_t("general.useSystemTitleBar")}</strong>
        <p>{_t("general.useSystemTitleBarDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.useSystemTitleBar}
      onclick={() => generalSettings.updateSetting("useSystemTitleBar", !s.useSystemTitleBar)}
      aria-checked={s.useSystemTitleBar}
      aria-label={_t("general.useSystemTitleBar")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="x" size={17} /></span>
      <div>
        <strong>{_t("general.showSettingsCloseButton")}</strong>
        <p>{_t("general.showSettingsCloseButtonDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.showSettingsCloseButton}
      onclick={() =>
        generalSettings.updateSetting("showSettingsCloseButton", !s.showSettingsCloseButton)}
      aria-checked={s.showSettingsCloseButton}
      aria-label={_t("general.showSettingsCloseButton")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="maximize" size={17} /></span>
      <div>
        <strong>{_t("general.desktopFullscreen")}</strong>
        <p>{_t("general.desktopFullscreenDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.imageFullscreenMode === "desktop"}
      onclick={() =>
        generalSettings.updateSetting(
          "imageFullscreenMode",
          s.imageFullscreenMode === "desktop" ? "overlay" : "desktop",
        )}
      aria-checked={s.imageFullscreenMode === "desktop"}
      aria-label={_t("general.desktopFullscreen")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="image" size={17} /></span>
      <div class="heading-inline">
        <strong>{_t("general.viewerBackdropOpacity")}</strong>
        <span class="value-label">{s.viewerBackdropOpacity}%</span>
      </div>
    </div>
    <input
      type="range"
      min="0"
      max="100"
      value={s.viewerBackdropOpacity}
      oninput={(e) => {
        generalSettings.updateSetting(
          "viewerBackdropOpacity",
          Number((e.target as HTMLInputElement).value),
        );
        updateSliderTrack(viewerOpacityEl);
      }}
      class="transparency-slider"
      bind:this={viewerOpacityEl}
    />
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="pin" size={17} /></span>
      <div>
        <strong>{_t("general.rememberWindowPosition")}</strong>
        <p>{_t("general.rememberWindowPositionDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.rememberWindowPosition}
      onclick={() =>
        generalSettings.updateSetting("rememberWindowPosition", !s.rememberWindowPosition)}
      aria-checked={s.rememberWindowPosition}
      aria-label={_t("general.rememberWindowPosition")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>

{#if feedback}
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
    max-width: 430px;
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
    cursor: pointer;
  }

  .settings-scroll {
    display: grid;
    gap: 8px;
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
  }

  .heading-inline {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex: 1;
    min-width: 0;
    gap: 8px;
  }

  .value-label {
    color: var(--text-muted);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .lang-toggle {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .lang-toggle button {
    padding: 7px 16px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-muted);
    background: var(--input-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .lang-toggle button:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .lang-toggle button.active {
    border-color: var(--text-faint);
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .transparency-slider {
    width: 100%;
    margin-top: 12px;
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

  .toggle-switch {
    width: 40px;
    height: 22px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 12px;
    background: var(--input-bg);
    cursor: pointer;
    position: relative;
    flex-shrink: 0;
    transition:
      border-color 100ms ease,
      background 100ms ease;
  }

  .toggle-switch.active {
    border-color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 18%, transparent);
  }

  .toggle-switch:disabled {
    cursor: wait;
    opacity: 0.6;
  }

  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text-faint);
    transition:
      transform 120ms ease,
      background 100ms ease;
  }

  .toggle-switch.active .toggle-knob {
    transform: translateX(18px);
    background: var(--selection-color);
  }

  .theme-select {
    padding: 5px 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-primary);
    background: var(--input-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    flex-shrink: 0;
    outline: none;
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
  }

  .theme-select:focus {
    border-color: var(--text-faint);
  }

  button {
    cursor: pointer;
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

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: var(--text-faint);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    text-align: center;
  }
</style>
