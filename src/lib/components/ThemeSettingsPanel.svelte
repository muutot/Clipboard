<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type {
    ThemeColors,
    ThemeMode,
    ThemePreset,
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
    { key: "textSecondary", label: _t("theme.textSecondary"), desc: _t("theme.textSecondaryDescription") },
    { key: "textMuted", label: _t("theme.textMuted"), desc: _t("theme.textMutedDescription") },
    { key: "textFaint", label: _t("theme.textFaint"), desc: _t("theme.textFaintDescription") },
    { key: "border", label: _t("theme.border"), desc: _t("theme.borderDescription") },
    { key: "borderSubtle", label: _t("theme.borderSubtle"), desc: _t("theme.borderSubtleDescription") },
    { key: "cardBg", label: _t("theme.cardBg"), desc: _t("theme.cardBgDescription") },
    { key: "surfaceBg", label: _t("theme.surfaceBg"), desc: _t("theme.surfaceBgDescription") },
    { key: "statusBarBg", label: _t("theme.statusBarBg"), desc: _t("theme.statusBarBgDescription") },
    { key: "hoverBg", label: _t("theme.hoverBg"), desc: _t("theme.hoverBgDescription") },
    { key: "inputBg", label: _t("theme.inputBg"), desc: _t("theme.inputBgDescription") },
    { key: "selectionColor", label: _t("theme.selectionColor"), desc: _t("theme.selectionColorDescription") },
    { key: "successColor", label: _t("theme.successColor"), desc: _t("theme.successColorDescription") },
    { key: "dangerColor", label: _t("theme.dangerColor"), desc: _t("theme.dangerColorDescription") },
    { key: "warningColor", label: _t("theme.warningColor"), desc: _t("theme.warningColorDescription") },
    { key: "scrollbarColor", label: _t("theme.scrollbarColor"), desc: _t("theme.scrollbarColorDescription") },
  ];

  const isReadonly = $derived(s.theme !== "custom");

  let presetName = $state("");

  function savePreset() {
    const name = presetName.trim();
    if (!name) return;
    const preset: ThemePreset = {
      id: crypto.randomUUID(),
      name,
      colors: { ...themeColors },
    };
    const presets = [...(s.customPresets ?? []), preset];
    generalSettings.updateSetting("customPresets", presets);
    presetName = "";
    showFeedback(_t("theme.presetSaved"), true);
  }

  function applyPreset(preset: ThemePreset) {
    themeColors = { ...preset.colors };
    generalSettings.updateSetting("theme", "custom");
    generalSettings.updateSetting("themeColors", { ...preset.colors });
    showFeedback(_t("theme.presetApplied"), true);
  }

  function deletePreset(id: string) {
    const presets = (s.customPresets ?? []).filter((p) => p.id !== id);
    generalSettings.updateSetting("customPresets", presets);
    showFeedback(_t("theme.presetDeleted"), true);
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("appearanceSettings")}</span>
      <h2>{_t("theme.title")}</h2>
      <p>{_t("theme.themeModeDescription")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
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
  {:else}
  <section class="setting-card preset-section">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="star" size={16} /></span>
      <div>
        <strong>{_t("theme.presets")}</strong>
        <p>{_t("theme.presetsDescription")}</p>
      </div>
    </div>
    <div class="preset-save-row">
      <input
        type="text"
        class="preset-name-input"
        placeholder={_t("theme.presetNamePlaceholder")}
        bind:value={presetName}
        onkeydown={(e) => e.key === "Enter" && savePreset()}
      />
      <button class="preset-save-btn" type="button" disabled={!presetName.trim()} onclick={savePreset}>
        {_t("theme.savePreset")}
      </button>
    </div>
    {#if (s.customPresets ?? []).length > 0}
      <div class="preset-list">
        {#each s.customPresets ?? [] as preset (preset.id)}
          <div class="preset-row">
            <span class="preset-row-name">{preset.name}</span>
            <span class="preset-row-actions">
              <button class="preset-action-btn" type="button" onclick={() => applyPreset(preset)}>
                {_t("theme.applyPreset")}
              </button>
              <button class="preset-action-btn danger" type="button" onclick={() => deletePreset(preset.id)}>
                {_t("theme.deletePreset")}
              </button>
            </span>
          </div>
        {/each}
      </div>
    {:else}
      <p class="preset-empty">{_t("theme.noPresets")}</p>
    {/if}
  </section>

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
  {/if}

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>

{#if feedback}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  .theme-select {
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    background: var(--input-bg);
    color: var(--text-primary);
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    padding: 6px 28px 6px 10px;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    flex-shrink: 0;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6'%3E%3Cpath d='M0 0l5 6 5-6z' fill='%23777'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 8px center;
  }

  .theme-select:focus {
    border-color: var(--text-faint);
    outline: none;
  }

  .color-swatch {
    width: 20px;
    height: 20px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
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
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    cursor: pointer;
    padding: 2px;
    background: var(--input-bg);
  }

  .color-picker:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .color-text-input {
    width: 80px;
    padding: 5px 8px;
    background: var(--input-bg);
    color: var(--text-primary);
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    font-family: monospace;
    text-transform: uppercase;
  }

  .color-text-input:focus {
    border-color: var(--text-faint);
    outline: none;
  }

  .color-text-input:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .readonly-hint {
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
    color: var(--text-muted);
    margin: 0 0 12px 16px;
  }

  .close-button:hover {
    background: var(--hover-bg);
  }

  .preset-section {
    display: block;
  }

  .preset-save-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
  }

  .preset-name-input {
    flex: 1;
    min-width: 0;
    padding: 7px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--input-bg);
    color: var(--text-primary);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    outline: none;
    transition: border-color 120ms ease;
  }

  .preset-name-input:focus {
    border-color: var(--text-faint);
  }

  .preset-save-btn {
    flex-shrink: 0;
    padding: 7px 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--hover-bg);
    color: var(--text-secondary);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    white-space: nowrap;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .preset-save-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
    border-color: var(--text-faint);
  }

  .preset-save-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .preset-list {
    display: grid;
    gap: 6px;
    margin-top: 12px;
  }

  .preset-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    background: var(--surface-bg);
  }

  .preset-row-name {
    min-width: 0;
    overflow: hidden;
    color: var(--text-primary);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    font-weight: 520;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .preset-row-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .preset-action-btn {
    padding: 4px 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--card-bg);
    color: var(--text-muted);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .preset-action-btn:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .preset-action-btn.danger:hover {
    color: color-mix(in srgb, var(--danger-color) 80%, white);
    border-color: color-mix(in srgb, var(--danger-color) 40%, transparent);
    background: color-mix(in srgb, var(--danger-color) 10%, transparent);
  }

  .preset-empty {
    margin: 10px 0 0;
    color: var(--text-faint);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
    text-align: center;
  }
</style>
