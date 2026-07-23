<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath, locale } from "$lib/i18n";
  import type { Locale } from "$lib/i18n/types";
  import type { GeneralSettings, FontSize, ThemeMode } from "$lib/types/clipboard";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let language = $state<Locale>($locale);
  let fontSize = $state<FontSize>("normal");
  let windowTransparency = $state(95);
  let compactMode = $state(false);
  let alwaysOnTop = $state(false);
  let useSystemTitleBar = $state(false);
  let theme = $state<ThemeMode>("dark");
  let feedback = $state("");
  let feedbackSuccess = $state(false);

  function changeLanguage(lang: Locale) {
    language = lang;
    locale.set(lang);
    feedback = _t(lang === "zh-CN" ? "已切换至中文" : "Switched to English");
    feedbackSuccess = true;
    setTimeout(() => (feedback = ""), 2000);
  }

  function changeFontSize(size: FontSize) {
    fontSize = size;
    const root = document.documentElement;
    const sizes: Record<FontSize, string> = { small: "13px", normal: "14px", large: "16px" };
    root.style.fontSize = sizes[size];
  }

  function handleTransparency(event: Event) {
    windowTransparency = Number((event.target as HTMLInputElement).value);
  }

  const fontSizeOptions = $derived([
    { value: "small" as const, label: _t("general.fontSizeSmall") },
    { value: "normal" as const, label: _t("general.fontSizeNormal") },
    { value: "large" as const, label: _t("general.fontSizeLarge") },
  ]);
</script>

<header>
  <div>
    <span class="eyebrow">设置 / 常规</span>
    <h2>{_t("general.title")}</h2>
    <p>界面外观和语言偏好设置。</p>
  </div>
  <button
    class="close-button"
    type="button"
    aria-label={_t("actions.close")}
    onclick={onclose}>×</button
  >
</header>

<div class="settings-scroll">
  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="globe" size={17} /></span>
      <div>
        <strong>{_t("general.language")}</strong>
        <p>界面显示语言</p>
      </div>
    </div>
    <div class="lang-toggle">
      <button
        type="button"
        class:active={language === "zh-CN"}
        onclick={() => changeLanguage("zh-CN")}
      >中文</button>
      <button
        type="button"
        class:active={language === "en"}
        onclick={() => changeLanguage("en")}
      >English</button>
    </div>
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="type" size={17} /></span>
      <div>
        <strong>{_t("general.fontSize")}</strong>
        <p>调整界面元素的字体大小</p>
      </div>
    </div>
    <div class="font-size-controls">
      {#each fontSizeOptions as option}
        <button
          type="button"
          class:active={fontSize === option.value}
          onclick={() => changeFontSize(option.value)}
        >{option.label}</button>
      {/each}
    </div>
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="sliders" size={17} /></span>
      <div>
        <strong>{_t("general.windowTransparency")}</strong>
        <p>{windowTransparency}%</p>
      </div>
    </div>
    <input
      type="range"
      min="60"
      max="100"
      value={windowTransparency}
      oninput={handleTransparency}
      class="transparency-slider"
    />
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
      <div>
        <strong>{_t("general.compactMode")}</strong>
        <p>减少卡片间距，显示更多记录</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={compactMode}
      onclick={() => (compactMode = !compactMode)}
      aria-checked={compactMode}
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
        <p>窗口始终悬浮在其他应用上方</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={alwaysOnTop}
      onclick={() => (alwaysOnTop = !alwaysOnTop)}
      aria-checked={alwaysOnTop}
      aria-label={_t("general.alwaysOnTop")}
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
        <p>使用操作系统原生标题栏</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={useSystemTitleBar}
      onclick={() => (useSystemTitleBar = !useSystemTitleBar)}
      aria-checked={useSystemTitleBar}
      aria-label={_t("general.useSystemTitleBar")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="palette" size={17} /></span>
      <div>
        <strong>{_t("general.theme")}</strong>
        <p>仅支持暗黑主题</p>
      </div>
    </div>
    <div class="theme-option">
      <span class="theme-dot-dark"></span>
      <span>{_t("general.themeDark")}</span>
    </div>
  </section>
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

  .lang-toggle,
  .font-size-controls {
    display: flex;
    gap: 6px;
    margin-top: 12px;
  }

  .lang-toggle button,
  .font-size-controls button {
    padding: 7px 16px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #999;
    background: #1a1a1a;
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    transition: background 100ms ease, border-color 100ms ease, color 100ms ease;
  }

  .lang-toggle button:hover,
  .font-size-controls button:hover {
    color: #ccc;
    background: #292929;
  }

  .lang-toggle button.active,
  .font-size-controls button.active {
    border-color: #5a5a5a;
    color: #f0f0f0;
    background: #333;
  }

  .transparency-slider {
    width: 100%;
    margin-top: 12px;
    -webkit-appearance: none;
    appearance: none;
    height: 6px;
    border-radius: 3px;
    background: #2a2a2a;
    outline: none;
  }

  .transparency-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid #555;
    background: #ccc;
    cursor: pointer;
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

  .theme-option {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
    color: #bbb;
    font-size: 11px;
  }

  .theme-dot-dark {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 1px solid #444;
    background: #1a1a1a;
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
    border: 1px solid #553434;
    border-radius: 7px;
    color: #d59c9c;
    background: rgba(48, 27, 27, 0.96);
    font-size: 10px;
  }

  .settings-feedback.success {
    border-color: #35513f;
    color: #9dc6aa;
    background: rgba(27, 45, 33, 0.96);
  }
</style>
