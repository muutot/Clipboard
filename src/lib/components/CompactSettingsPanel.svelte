<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let s = $state($generalSettings);
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  function sliderPercentage(value: number, min: number, max: number): string {
    const percentage = ((value - min) / (max - min)) * 100;
    return `${Math.min(100, Math.max(0, percentage))}%`;
  }

  function sliderHandler(key: keyof typeof s) {
    return (event: Event) => {
      const val = Number((event.target as HTMLInputElement).value);
      generalSettings.updateSetting(key, val);
    };
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("compact.eyebrow")}</span>
      <h2>{_t("compact.title")}</h2>
      <p>{_t("compact.description")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
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

  {#if s.compactMode}
    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="sliders" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.paddingTop")}</strong>
            <p>{_t("compact.paddingTopDescription")}</p>
          </div>
          <span class="value-label">{s.compactPaddingTop}px</span>
        </div>
      </div>
      <input
        type="range"
        min="0"
        max="20"
        value={s.compactPaddingTop}
        oninput={sliderHandler("compactPaddingTop")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactPaddingTop, 0, 20)}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="sliders" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.paddingBottom")}</strong>
            <p>{_t("compact.paddingBottomDescription")}</p>
          </div>
          <span class="value-label">{s.compactPaddingBottom}px</span>
        </div>
      </div>
      <input
        type="range"
        min="0"
        max="20"
        value={s.compactPaddingBottom}
        oninput={sliderHandler("compactPaddingBottom")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactPaddingBottom, 0, 20)}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="ruler" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.cardGap")}</strong>
            <p>{_t("compact.cardGapDescription")}</p>
          </div>
          <span class="value-label">{s.compactCardGap}px</span>
        </div>
      </div>
      <input
        type="range"
        min="0"
        max="20"
        value={s.compactCardGap}
        oninput={sliderHandler("compactCardGap")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactCardGap, 0, 20)}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="text" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.shortTextHeight")}</strong>
            <p>{_t("compact.shortTextHeightDescription")}</p>
          </div>
          <span class="value-label">{s.compactTextHeight}px</span>
        </div>
      </div>
      <input
        type="range"
        min="40"
        max="90"
        value={s.compactTextHeight}
        oninput={sliderHandler("compactTextHeight")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactTextHeight, 40, 90)}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="text" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.tallTextHeight")}</strong>
            <p>{_t("compact.tallTextHeightDescription")}</p>
          </div>
          <span class="value-label">{s.compactTallTextHeight}px</span>
        </div>
      </div>
      <input
        type="range"
        min="50"
        max="100"
        value={s.compactTallTextHeight}
        oninput={sliderHandler("compactTallTextHeight")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactTallTextHeight, 50, 100)}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="image" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.imageHeight")}</strong>
            <p>{_t("compact.imageHeightDescription")}</p>
          </div>
          <span class="value-label">{s.compactImageHeight}px</span>
        </div>
      </div>
      <input
        type="range"
        min="80"
        max="200"
        value={s.compactImageHeight}
        oninput={sliderHandler("compactImageHeight")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactImageHeight, 80, 200)}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="search" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.searchHeight")}</strong>
            <p>{_t("compact.searchHeightDescription")}</p>
          </div>
          <span class="value-label">{s.compactSearchHeight}px</span>
        </div>
      </div>
      <input
        type="range"
        min="28"
        max="56"
        value={s.compactSearchHeight}
        oninput={sliderHandler("compactSearchHeight")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactSearchHeight, 28, 56)}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="type" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.searchFontSize")}</strong>
            <p>{_t("compact.searchFontSizeDescription")}</p>
          </div>
          <span class="value-label">{s.compactSearchFontSize}px</span>
        </div>
      </div>
      <input
        type="range"
        min="10"
        max="24"
        value={s.compactSearchFontSize}
        oninput={sliderHandler("compactSearchFontSize")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactSearchFontSize, 10, 24)}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("compact.cardBorderRadius")}</strong>
            <p>{_t("compact.cardBorderRadiusDescription")}</p>
          </div>
          <span class="value-label">{s.compactCardBorderRadius}px</span>
        </div>
      </div>
      <input
        type="range"
        min="0"
        max="20"
        value={s.compactCardBorderRadius}
        oninput={sliderHandler("compactCardBorderRadius")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactCardBorderRadius, 0, 20)}
      />
    </section>
  {/if}

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>

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

  button {
    cursor: pointer;
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: var(--text-faint);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    text-align: center;
  }
</style>
