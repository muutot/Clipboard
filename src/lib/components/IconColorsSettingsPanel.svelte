<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import ColorField from "$lib/components/ColorField.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    ICON_NAMES,
    DEFAULT_ICON_COLORS,
    type IconColors,
    type IconName,
  } from "$lib/types/clipboard";
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

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  const currentColors = $derived<IconColors>(s.iconColors ?? {});
  const isReadonly = $derived(!s.colorIcons);

  function effectiveColor(name: IconName): string {
    return currentColors[name] ?? DEFAULT_ICON_COLORS[name];
  }

  function updateIconColor(name: IconName, value: string) {
    const cleaned = value.startsWith("#") ? value : "#" + value;
    if (!/^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(cleaned)) return;
    generalSettings.updateSetting("iconColors", {
      ...currentColors,
      [name]: cleaned,
    });
  }

  function resetIconColor(name: IconName) {
    const next: IconColors = { ...currentColors };
    delete next[name];
    generalSettings.updateSetting("iconColors", next);
  }

  function resetAllIconColors() {
    generalSettings.updateSetting("iconColors", {});
    showFeedback(_t("general.iconColorsResetAll"), true);
  }

  function showFeedback(msg: string, success: boolean) {
    feedback = msg;
    setTimeout(() => {
      feedback = "";
    }, 2000);
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("appearanceSettings")}</span>
      <h2>{_t("storage.iconsTab")}</h2>
      <p>{_t("general.iconColorsDescription")}</p>
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
        <strong>{_t("general.colorIcons")}</strong>
        <p>{_t("general.colorIconsDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.colorIcons}
      onclick={() => generalSettings.updateSetting("colorIcons", !s.colorIcons)}
      aria-checked={s.colorIcons}
      aria-label={_t("general.colorIcons")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>{_t("general.iconColors")}</strong>
          <p>{_t("general.iconColorsDescription")}</p>
        </div>
        <button
          type="button"
          class="icon-colors-reset-all"
          disabled={isReadonly || Object.keys(currentColors).length === 0}
          onclick={resetAllIconColors}
        >
          {_t("general.iconColorsResetAll")}
        </button>
      </div>
    </div>
    {#if isReadonly}
      <p class="readonly-hint">{_t("general.iconColorsDisabledHint")}</p>
    {/if}
    <div class="icon-color-grid">
      {#each ICON_NAMES as name (name)}
        <div class="icon-color-item">
          <span class="icon-color-preview">
            <AppIcon {name} size={18} />
          </span>
          <span class="icon-color-name">{name}</span>
          <ColorField
            value={effectiveColor(name)}
            disabled={isReadonly}
            compact
            colorAriaLabel={name}
            hexAriaLabel={`${name} hex`}
            onchange={(v) => updateIconColor(name, v)}
          />
          <button
            type="button"
            class="icon-color-reset"
            disabled={isReadonly || currentColors[name] == null}
            title={_t("general.iconColorsReset")}
            aria-label={`${name} ${_t("general.iconColorsReset")}`}
            onclick={() => resetIconColor(name)}
          >
            <AppIcon name="restore" size={12} strokeWidth={2} />
          </button>
        </div>
      {/each}
    </div>
  </section>
</div>

{#if feedback}
  <div class="settings-feedback success">{feedback}</div>
{/if}

<style>
  .icon-colors-reset-all {
    flex-shrink: 0;
    padding: 5px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--hover-bg);
    color: var(--text-secondary);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .icon-colors-reset-all:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--text-faint);
  }

  .icon-colors-reset-all:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .icon-color-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 6px;
    margin-top: 12px;
  }

  .icon-color-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    background: var(--surface-bg);
    min-width: 0;
  }

  .icon-color-preview {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    border-radius: 5px;
    background: var(--input-bg);
  }

  .icon-color-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-secondary);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  .icon-color-reset {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--card-bg);
    color: var(--text-muted);
    cursor: pointer;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .icon-color-reset:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .icon-color-reset:disabled {
    opacity: 0.35;
    cursor: default;
  }
</style>
