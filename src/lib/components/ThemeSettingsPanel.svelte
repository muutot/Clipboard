<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import ColorField from "$lib/components/ColorField.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type { ThemeColors, ThemeMode, ThemePreset } from "$lib/types/clipboard";
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
    generalSettings.updateSetting("activePresetId", undefined);
    if (value === "dark") {
      generalSettings.updateSetting("themeColors", { ...DARK_THEME_COLORS });
    } else if (value === "light") {
      generalSettings.updateSetting("themeColors", { ...LIGHT_THEME_COLORS });
    }
  }

  function updateColor(key: keyof ThemeColors, value: string) {
    if (s.theme !== "custom") return;
    const cleaned = value.startsWith("#") ? value : "#" + value;
    if (!/^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(cleaned)) return;
    themeColors = { ...themeColors, [key]: cleaned };
    generalSettings.updateSetting("themeColors", { ...themeColors, [key]: cleaned });
  }

  function resetToDark() {
    themeColors = { ...DARK_THEME_COLORS };
    generalSettings.updateSetting("themeColors", { ...DARK_THEME_COLORS });
  }

  function resetToLight() {
    themeColors = { ...LIGHT_THEME_COLORS };
    generalSettings.updateSetting("themeColors", { ...LIGHT_THEME_COLORS });
  }

  const colorEntries: { key: keyof ThemeColors; label: string; desc: string }[] = [
    { key: "bg", label: _t("theme.bg"), desc: _t("theme.bgDescription") },
    { key: "settingsBg", label: _t("theme.settingsBg"), desc: _t("theme.settingsBgDescription") },
    { key: "accent", label: _t("theme.accent"), desc: _t("theme.accentDescription") },
    {
      key: "textPrimary",
      label: _t("theme.textPrimary"),
      desc: _t("theme.textPrimaryDescription"),
    },
    {
      key: "textSecondary",
      label: _t("theme.textSecondary"),
      desc: _t("theme.textSecondaryDescription"),
    },
    { key: "textMuted", label: _t("theme.textMuted"), desc: _t("theme.textMutedDescription") },
    { key: "textFaint", label: _t("theme.textFaint"), desc: _t("theme.textFaintDescription") },
    {
      key: "placeholderColor",
      label: _t("theme.placeholderColor"),
      desc: _t("theme.placeholderColorDescription"),
    },
    { key: "border", label: _t("theme.border"), desc: _t("theme.borderDescription") },
    {
      key: "borderSubtle",
      label: _t("theme.borderSubtle"),
      desc: _t("theme.borderSubtleDescription"),
    },
    { key: "cardBg", label: _t("theme.cardBg"), desc: _t("theme.cardBgDescription") },
    { key: "surfaceBg", label: _t("theme.surfaceBg"), desc: _t("theme.surfaceBgDescription") },
    {
      key: "statusBarBg",
      label: _t("theme.statusBarBg"),
      desc: _t("theme.statusBarBgDescription"),
    },
    { key: "hoverBg", label: _t("theme.hoverBg"), desc: _t("theme.hoverBgDescription") },
    { key: "inputBg", label: _t("theme.inputBg"), desc: _t("theme.inputBgDescription") },
    {
      key: "selectionColor",
      label: _t("theme.selectionColor"),
      desc: _t("theme.selectionColorDescription"),
    },
    {
      key: "successColor",
      label: _t("theme.successColor"),
      desc: _t("theme.successColorDescription"),
    },
    {
      key: "dangerColor",
      label: _t("theme.dangerColor"),
      desc: _t("theme.dangerColorDescription"),
    },
    {
      key: "warningColor",
      label: _t("theme.warningColor"),
      desc: _t("theme.warningColorDescription"),
    },
    {
      key: "scrollbarColor",
      label: _t("theme.scrollbarColor"),
      desc: _t("theme.scrollbarColorDescription"),
    },
  ];

  const isReadonly = $derived(s.theme !== "custom");
  const displayColors = $derived(
    s.theme === "dark"
      ? DARK_THEME_COLORS
      : s.theme === "light"
        ? LIGHT_THEME_COLORS
        : themeColors,
  );

  let editingId = $state<string | undefined>(undefined);
  let editName = $state("");
  let editInput = $state<HTMLInputElement | undefined>(undefined);
  let adding = $state(false);
  let newPresetName = $state("");
  let addInput = $state<HTMLInputElement | undefined>(undefined);

  $effect(() => {
    if (editingId !== undefined) {
      editInput?.focus();
      editInput?.select();
    }
  });

  $effect(() => {
    if (adding) {
      addInput?.focus();
    }
  });

  function startRename(preset: ThemePreset) {
    editingId = preset.id;
    editName = preset.name;
  }

  function commitRename() {
    if (editingId) {
      const name = editName.trim();
      if (name) {
        const presets = (s.customPresets ?? []).map((p) =>
          p.id === editingId ? { ...p, name } : p,
        );
        generalSettings.updateSetting("customPresets", presets);
        showFeedback(_t("theme.presetRenamed"), true);
      }
    }
    editingId = undefined;
    editName = "";
  }

  function cancelRename() {
    editingId = undefined;
    editName = "";
  }

  function saveNewPreset() {
    const name = newPresetName.trim();
    adding = false;
    newPresetName = "";
    if (!name) return;
    const preset: ThemePreset = {
      id: crypto.randomUUID(),
      name,
      colors: { ...themeColors },
    };
    const presets = [...(s.customPresets ?? []), preset];
    generalSettings.updateSetting("customPresets", presets);
    generalSettings.updateSetting("activePresetId", preset.id);
    showFeedback(_t("theme.presetSaved"), true);
  }

  function cancelAdd() {
    adding = false;
    newPresetName = "";
  }

  function overwritePreset(preset: ThemePreset) {
    const presets = (s.customPresets ?? []).map((p) =>
      p.id === preset.id ? { ...p, colors: { ...themeColors } } : p,
    );
    generalSettings.updateSetting("customPresets", presets);
    showFeedback(_t("theme.presetUpdated"), true);
  }

  function applyPreset(preset: ThemePreset) {
    themeColors = { ...preset.colors };
    generalSettings.updateSetting("theme", "custom");
    generalSettings.updateSetting("themeColors", { ...preset.colors });
    generalSettings.updateSetting("activePresetId", preset.id);
    showFeedback(_t("theme.presetApplied"), true);
  }

  function deletePreset(id: string) {
    if (s.activePresetId === id) {
      generalSettings.updateSetting("activePresetId", undefined);
    }
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
    <CustomSelect
      value={s.theme}
      ariaLabel={_t("theme.themeMode")}
      options={[
        { value: "dark", label: _t("theme.dark") },
        { value: "light", label: _t("theme.light") },
        { value: "custom", label: _t("theme.custom") },
      ]}
      onchange={(val) => {
        if (val === "dark" || val === "light" || val === "custom") {
          changeTheme(val);
        }
      }}
    />
  </section>

  {#if isReadonly}
    <p class="readonly-hint">{_t("theme.readonlyHint")}</p>
  {/if}

  {#if !isReadonly}
    <section class="setting-card preset-section">
      <div class="preset-heading-row">
        <div class="setting-heading">
          <span class="setting-icon"><AppIcon name="star" size={16} /></span>
          <div>
            <strong>{_t("theme.presets")}</strong>
            <p>{_t("theme.presetsDescription")}</p>
          </div>
        </div>
        {#if (s.customPresets ?? []).length > 0}
          <div class="preset-heading-actions">
            <button class="preset-action-btn" type="button" onclick={resetToDark}>
              {_t("theme.darkPreset")}
            </button>
            <button class="preset-action-btn" type="button" onclick={resetToLight}>
              {_t("theme.lightPreset")}
            </button>
            <button class="preset-action-btn" type="button" onclick={() => (adding = true)}>
              + {_t("theme.addPreset")}
            </button>
          </div>
        {/if}
      </div>
      <div class="preset-list">
        {#each s.customPresets ?? [] as preset (preset.id)}
          <div class="preset-row" class:active={s.activePresetId === preset.id}>
            {#if editingId === preset.id}
              <input
                type="text"
                class="preset-name-input"
                value={editName}
                bind:this={editInput}
                oninput={(e) => (editName = (e.target as HTMLInputElement).value)}
                onkeydown={(e) => {
                  if (e.key === "Enter") commitRename();
                  else if (e.key === "Escape") cancelRename();
                }}
                onblur={commitRename}
              />
            {:else}
              <span class="preset-row-name">
                {#if s.activePresetId === preset.id}
                  <span class="preset-check">&#10003;</span>
                {/if}
                {preset.name}
              </span>
            {/if}
            <span class="preset-row-actions">
              <button class="preset-action-btn" type="button" onclick={() => startRename(preset)}>
                {_t("theme.renamePreset")}
              </button>
              <button
                class="preset-action-btn"
                type="button"
                onclick={() => overwritePreset(preset)}
              >
                {_t("theme.overwritePreset")}
              </button>
              <button class="preset-action-btn" type="button" onclick={() => applyPreset(preset)}>
                {_t("theme.applyPreset")}
              </button>
              <button
                class="preset-action-btn danger"
                type="button"
                onclick={() => deletePreset(preset.id)}
              >
                {_t("theme.deletePreset")}
              </button>
            </span>
          </div>
        {/each}
        {#if adding}
          <div class="preset-row">
            <input
              type="text"
              class="preset-name-input"
              value={newPresetName}
              bind:this={addInput}
              placeholder={_t("theme.presetNamePlaceholder")}
              oninput={(e) => (newPresetName = (e.target as HTMLInputElement).value)}
              onkeydown={(e) => {
                if (e.key === "Enter") saveNewPreset();
                else if (e.key === "Escape") cancelAdd();
              }}
              onblur={cancelAdd}
            />
          </div>
        {:else if (s.customPresets ?? []).length === 0}
          <button class="preset-add-row" type="button" onclick={() => (adding = true)}>
            + {_t("theme.addPreset")}
          </button>
        {/if}
      </div>
    </section>
  {/if}

  {#each colorEntries as entry}
      <section class="setting-card toggle-card">
        <div class="setting-heading">
          <span class="color-swatch" style="background-color: {displayColors[entry.key]}"></span>
          <div>
            <strong>{entry.label}</strong>
            <p>{entry.desc}</p>
          </div>
        </div>
        <div class="color-input-group">
          <ColorField
            value={displayColors[entry.key]}
            disabled={isReadonly}
            onchange={(v) => updateColor(entry.key, v)}
          />
        </div>
      </section>
    {/each}
</div>

{#if feedback}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
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

  .close-button:hover {
    background: var(--hover-bg);
  }

  .preset-section {
    display: block;
  }

  .preset-heading-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .preset-heading-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .preset-name-input {
    flex: 1;
    min-width: 0;
    padding: 5px 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--input-bg);
    color: var(--text-primary);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    outline: none;
  }

  .preset-name-input:focus {
    border-color: var(--text-faint);
  }

  .preset-name-input::placeholder {
    color: var(--placeholder-color);
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

  .preset-add-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    width: 100%;
    padding: 8px 10px;
    border: 1px dashed var(--border-color);
    border-radius: 7px;
    background: transparent;
    color: var(--text-muted);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition:
      color 100ms ease,
      background 100ms ease;
  }

  .preset-add-row:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
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

  .preset-row.active {
    border-color: color-mix(in srgb, var(--selection-color) 36%, transparent);
    background: color-mix(in srgb, var(--selection-color) 8%, var(--surface-bg));
  }

  .preset-check {
    color: var(--selection-color);
    font-weight: 700;
    margin-right: 4px;
  }
</style>
