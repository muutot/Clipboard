<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type {
    ThemeColors,
    ThemeMode,
  } from "$lib/types/clipboard";
  import { DARK_THEME_COLORS, LIGHT_THEME_COLORS } from "$lib/types/clipboard";
  import { generalSettings } from "$lib/services/settings";

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
  let themeColors = $state<ThemeColors>({ ...DARK_THEME_COLORS });

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
      if (v.themeColors) {
        themeColors = { ...v.themeColors };
      }
    });
    return unsub;
  });

  function showFeedback(msg: string, success: boolean) {
    feedback = msg;
    feedbackSuccess = success;
    setTimeout(() => {
      feedback = "";
    }, 2000);
  }

  function changeTheme(value: ThemeMode) {
    generalSettings.updateSetting("theme", value);
    if (value === "dark") {
      generalSettings.updateSetting("themeColors", { ...DARK_THEME_COLORS });
    } else if (value === "light") {
      generalSettings.updateSetting("themeColors", { ...LIGHT_THEME_COLORS });
    }
  }

  function updateColor(key: keyof ThemeColors, value: string) {
    if (s.theme !== "custom") return;
    const cleaned = value.startsWith("#") ? value : "#" + value;
    if (!/^#[0-9a-fA-F]{6}$/.test(cleaned)) return;
    themeColors = { ...themeColors, [key]: cleaned };
    generalSettings.updateSetting("themeColors", { ...themeColors, [key]: cleaned });
  }

  const colorEntries: { key: keyof ThemeColors; label: string; desc: string }[] = [
    { key: "bg", label: _t("theme.bg"), desc: _t("theme.bgDescription") },
    { key: "settingsBg", label: _t("theme.settingsBg"), desc: _t("theme.settingsBgDescription") },
    { key: "accent", label: _t("theme.accent"), desc: _t("theme.accentDescription") },
    { key: "textPrimary", label: _t("theme.textPrimary"), desc: _t("theme.textPrimaryDescription") },
    { key: "textMuted", label: _t("theme.textMuted"), desc: _t("theme.textMutedDescription") },
    { key: "border", label: _t("theme.border"), desc: _t("theme.borderDescription") },
    { key: "cardBg", label: _t("theme.cardBg"), desc: _t("theme.cardBgDescription") },
  ];

  const isReadonly = $derived(s.theme !== "custom");
</script>

{#if showHeader}
  <header>
    <h2>{_t("theme.title")}</h2>
    <button
      class="close-button"
      type="button"
      aria-label={_t("actions.close")}
      onclick={onclose}>×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="palette" size={17} /></span>
      <div>
        <strong>{_t("theme.themeMode")}</strong>
        <p>{_t("theme.themeModeDescription")}</p>
      </div>
    </div>
    <select
      class="theme-select"
      value={s.theme}
      onchange={(e) => changeTheme((e.target as HTMLSelectElement).value as ThemeMode)}
    >
      <option value="dark">{_t("theme.dark")}</option>
      <option value="light">{_t("theme.light")}</option>
      <option value="custom">{_t("theme.custom")}</option>
    </select>
  </section>

  {#if isReadonly}
    <p class="readonly-hint">{_t("theme.readonlyHint")}</p>
  {/if}

  {#each colorEntries as entry}
    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span
          class="color-swatch"
          style="background-color: {themeColors[entry.key]}"
        ></span>
        <div>
          <strong>{entry.label}</strong>
          <p>{entry.desc}</p>
        </div>
      </div>
      <div class="color-input-group">
        <input
          type="color"
          class="color-picker"
          value={themeColors[entry.key]}
          disabled={isReadonly}
          oninput={(e) => updateColor(entry.key, (e.target as HTMLInputElement).value)}
        />
        <input
          type="text"
          class="color-text-input"
          value={themeColors[entry.key]}
          disabled={isReadonly}
          maxlength={7}
          oninput={(e) => {
            const val = (e.target as HTMLInputElement).value;
            if (/^#[0-9a-fA-F]{0,6}$/.test(val)) {
              updateColor(entry.key, val);
            }
          }}
        />
      </div>
    </section>
  {/each}

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>

{#if feedback}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  .settings-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 16px 14px 24px;
  }

  .setting-card {
    background: #1e1e1e;
    border: 1px solid #303030;
    border-radius: 10px;
    padding: 14px 16px;
    margin-bottom: 12px;
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

  .setting-heading strong {
    display: block;
    font-size: 13px;
    color: #d8d8d8;
  }

  .setting-heading p {
    margin: 2px 0 0;
    font-size: 11px;
    color: #777;
  }

  .setting-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .theme-select {
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    background: #1a1a1a;
    color: #d8d8d8;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    padding: 6px 28px 6px 10px;
    font-size: 13px;
    cursor: pointer;
    flex-shrink: 0;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6'%3E%3Cpath d='M0 0l5 6 5-6z' fill='%23777'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 8px center;
  }

  .theme-select:focus {
    border-color: #5a5a5a;
    outline: none;
  }

  .color-swatch {
    width: 20px;
    height: 20px;
    border-radius: 4px;
    border: 1px solid #3a3a3a;
    flex-shrink: 0;
  }

  .color-input-group {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .color-picker {
    width: 32px;
    height: 32px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    cursor: pointer;
    padding: 2px;
    background: #1a1a1a;
  }

  .color-picker:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .color-text-input {
    width: 80px;
    padding: 5px 8px;
    background: #1a1a1a;
    color: #d8d8d8;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    font-size: 12px;
    font-family: monospace;
    text-transform: uppercase;
  }

  .color-text-input:focus {
    border-color: #5a5a5a;
    outline: none;
  }

  .color-text-input:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .readonly-hint {
    font-size: 11px;
    color: #777;
    margin: 0 0 12px 16px;
  }

  .auto-save-note {
    font-size: 11px;
    color: #555;
    margin: 8px 0 0;
  }

  .settings-feedback {
    position: fixed;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    background: #333;
    color: #f5f5f5;
    padding: 8px 16px;
    border-radius: 8px;
    font-size: 12px;
    z-index: 100;
  }

  .settings-feedback.success {
    background: #2d4a2d;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px;
    flex-shrink: 0;
  }

  header h2 {
    font-size: 15px;
    color: #d8d8d8;
    margin: 0;
  }

  .close-button {
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid #3a3a3a;
    border-radius: 7px;
    color: #999;
    font-size: 19px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    line-height: 1;
  }

  .close-button:hover {
    background: #242424;
  }
</style>
