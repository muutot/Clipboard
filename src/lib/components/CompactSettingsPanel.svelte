<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let s = $state($generalSettings);
  generalSettings.subscribe((v) => { s = v; });

  function updateSliderTrack(el: HTMLInputElement) {
    const pct = ((Number(el.value) - Number(el.min)) / (Number(el.max) - Number(el.min))) * 100;
    el.style.setProperty("--slider-pct", pct + "%");
  }

  function sliderHandler(key: keyof typeof s) {
    return (event: Event) => {
      const val = Number((event.target as HTMLInputElement).value);
      generalSettings.updateSetting(key, val);
      updateSliderTrack(event.target as HTMLInputElement);
    };
  }

  $effect(() => {
    document.querySelectorAll<HTMLInputElement>(".compact-slider").forEach((el) => {
      updateSliderTrack(el);
    });
  });
</script>

<header>
  <div>
    <span class="eyebrow">{_t("compact.eyebrow")}</span>
    <h2>{_t("compact.title")}</h2>
    <p>{_t("compact.description")}</p>
  </div>
  <button
    class="close-button"
    type="button"
    aria-label={_t("actions.close")}
    onclick={onclose}>×</button
  >
</header>

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
        <div>
          <strong>{_t("compact.paddingTop")}</strong>
          <p>{s.compactPaddingTop}px · {_t("compact.paddingTopDescription")}</p>
        </div>
      </div>
      <input
        type="range"
        min="0"
        max="20"
        value={s.compactPaddingTop}
        oninput={sliderHandler("compactPaddingTop")}
        class="compact-slider"
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="sliders" size={17} /></span>
        <div>
          <strong>{_t("compact.paddingBottom")}</strong>
          <p>{s.compactPaddingBottom}px · {_t("compact.paddingBottomDescription")}</p>
        </div>
      </div>
      <input
        type="range"
        min="0"
        max="20"
        value={s.compactPaddingBottom}
        oninput={sliderHandler("compactPaddingBottom")}
        class="compact-slider"
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="ruler" size={17} /></span>
        <div>
          <strong>{_t("compact.cardGap")}</strong>
          <p>{s.compactCardGap}px · {_t("compact.cardGapDescription")}</p>
        </div>
      </div>
      <input
        type="range"
        min="0"
        max="20"
        value={s.compactCardGap}
        oninput={sliderHandler("compactCardGap")}
        class="compact-slider"
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="text" size={17} /></span>
        <div>
          <strong>{_t("compact.shortTextHeight")}</strong>
          <p>{s.compactTextHeight}px · {_t("compact.shortTextHeightDescription")}</p>
        </div>
      </div>
      <input
        type="range"
        min="40"
        max="90"
        value={s.compactTextHeight}
        oninput={sliderHandler("compactTextHeight")}
        class="compact-slider"
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="text" size={17} /></span>
        <div>
          <strong>{_t("compact.tallTextHeight")}</strong>
          <p>{s.compactTallTextHeight}px · {_t("compact.tallTextHeightDescription")}</p>
        </div>
      </div>
      <input
        type="range"
        min="50"
        max="100"
        value={s.compactTallTextHeight}
        oninput={sliderHandler("compactTallTextHeight")}
        class="compact-slider"
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="image" size={17} /></span>
        <div>
          <strong>{_t("compact.imageHeight")}</strong>
          <p>{s.compactImageHeight}px · {_t("compact.imageHeightDescription")}</p>
        </div>
      </div>
      <input
        type="range"
        min="80"
        max="200"
        value={s.compactImageHeight}
        oninput={sliderHandler("compactImageHeight")}
        class="compact-slider"
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
    border-bottom: 1px solid #292929;
  }

  .eyebrow {
    color: #777;
    font-size: 9.5px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 {
    margin: 5px 0 4px;
    color: #efefef;
    font-size: 18px;
    font-weight: 590;
  }

  header p {
    max-width: 430px;
    margin: 0;
    color: #777;
    font-size: 10.5px;
    line-height: 1.5;
  }

  .close-button {
    width: 28px;
    height: 28px;
    border: 1px solid #353535;
    border-radius: 7px;
    color: #999;
    background: #222;
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
  }

  .settings-scroll {
    display: grid;
    gap: 10px;
    min-height: 0;
    padding: 14px 18px 48px;
    overflow: auto;
    scrollbar-color: #9a9a9a transparent;
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
    background: #858585;
  }

  .setting-card {
    padding: 13px;
    border: 1px solid #303030;
    border-radius: 9px;
    background: #1e1e1e;
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
    border: 1px solid #363636;
    border-radius: 7px;
    color: #d2d2d2;
    background: #242424;
  }

  .setting-heading strong {
    display: block;
    color: #dedede;
    font-size: 11.5px;
    font-weight: 560;
  }

  .setting-heading p {
    margin: 2px 0 0;
    color: #777;
    font-size: 9.8px;
  }

  .compact-slider {
    width: 100%;
    margin-top: 12px;
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: #2a2a2a;
    outline: none;
    cursor: pointer;
  }

  .compact-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(to right, #4aa8ff 0%, #4aa8ff var(--slider-pct, 50%), #2a2a2a var(--slider-pct, 50%), #2a2a2a 100%);
  }

  .compact-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border-radius: 50%;
    border: 2px solid #4aa8ff;
    background: #1a1a1a;
    cursor: pointer;
    transition: box-shadow 100ms ease, transform 100ms ease;
  }

  .compact-slider::-webkit-slider-thumb:hover {
    box-shadow: 0 0 6px rgba(74, 168, 255, 0.4);
    transform: scale(1.15);
  }

  .compact-slider::-moz-range-track {
    height: 4px;
    border-radius: 2px;
    background: #2a2a2a;
  }

  .compact-slider::-moz-range-progress {
    height: 4px;
    border-radius: 2px;
    background: #4aa8ff;
  }

  .compact-slider::-moz-range-thumb {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid #4aa8ff;
    background: #1a1a1a;
    cursor: pointer;
    transition: box-shadow 100ms ease, transform 100ms ease;
  }

  .compact-slider::-moz-range-thumb:hover {
    box-shadow: 0 0 6px rgba(74, 168, 255, 0.4);
    transform: scale(1.15);
  }

  .toggle-switch {
    width: 40px;
    height: 22px;
    padding: 0;
    border: 1px solid #3a3a3a;
    border-radius: 12px;
    background: #1a1a1a;
    cursor: pointer;
    position: relative;
    flex-shrink: 0;
    transition: border-color 100ms ease, background 100ms ease;
  }

  .toggle-switch.active {
    border-color: #4aa8ff;
    background: rgba(74, 168, 255, 0.18);
  }

  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #666;
    transition: transform 120ms ease, background 100ms ease;
  }

  .toggle-switch.active .toggle-knob {
    transform: translateX(18px);
    background: #4aa8ff;
  }

  button {
    cursor: pointer;
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: #666;
    font-size: 10px;
    text-align: center;
  }
</style>
