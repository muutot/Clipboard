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
        min="36"
        max="90"
        value={s.compactTextHeight}
        oninput={sliderHandler("compactTextHeight")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactTextHeight, 36, 90)}
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
        min="44"
        max="100"
        value={s.compactTallTextHeight}
        oninput={sliderHandler("compactTallTextHeight")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactTallTextHeight, 44, 100)}
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
        min="64"
        max="200"
        value={s.compactImageHeight}
        oninput={sliderHandler("compactImageHeight")}
        class="transparency-slider"
        style:--slider-pct={sliderPercentage(s.compactImageHeight, 64, 200)}
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


