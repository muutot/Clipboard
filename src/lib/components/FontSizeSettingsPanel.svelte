<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";
  import { emit } from "@tauri-apps/api/event";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let s = $state($generalSettings);
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => { s = v; });
    return unsub;
  });

  function updateSliderTrack(el: HTMLInputElement) {
    const pct = ((Number(el.value) - Number(el.min)) / (Number(el.max) - Number(el.min))) * 100;
    el.style.setProperty("--slider-pct", pct + "%");
  }

  function applyFontSize(category: keyof typeof s.fontSizes, value: number) {
    generalSettings.updateSetting("fontSizes", { ...s.fontSizes, [category]: value });
    document.documentElement.style.fontSize = `${s.fontSizes.base}px`;
    document.documentElement.style.setProperty(`--font-size-${category}`, `${value}px`);
    emit("settings-font-changed", { fontSizes: { ...s.fontSizes, [category]: value }, display: s.display }).catch(() => {});
  }

  function updateDisplay(partial: Partial<typeof s.display>) {
    const d = { ...s.display, ...partial };
    generalSettings.updateSetting("display", d);
    emit("settings-font-changed", { fontSizes: s.fontSizes, display: d }).catch(() => {});
  }

  function sliderHandler(category: keyof typeof s.fontSizes) {
    return (event: Event) => {
      const val = Number((event.target as HTMLInputElement).value);
      applyFontSize(category, val);
      updateSliderTrack(event.target as HTMLInputElement);
    };
  }

  $effect(() => {
    const el = document.querySelectorAll<HTMLInputElement>(".transparency-slider");
    el.forEach(updateSliderTrack);
  });

  $effect(() => {
    document.documentElement.style.fontSize = `${s.fontSizes.base}px`;
    document.documentElement.style.setProperty("--font-size-base", `${s.fontSizes.base}px`);
    document.documentElement.style.setProperty("--font-size-secondary", `${s.fontSizes.secondary}px`);
    document.documentElement.style.setProperty("--font-size-tiny", `${s.fontSizes.tiny}px`);
    document.documentElement.style.setProperty("--font-size-cardTitle", `${s.fontSizes.cardTitle}px`);
    document.documentElement.style.setProperty("--font-size-cardPreview", `${s.fontSizes.cardPreview}px`);
    document.documentElement.style.setProperty("--show-secondary", s.display.showSecondaryText ? "block" : "none");
  });
</script>

<header>
  <div>
    <span class="eyebrow">设置 / 显示</span>
    <h2>字体大小</h2>
    <p>为不同 UI 区域单独调整字体大小</p>
  </div>
  <button class="close-button" type="button" aria-label="关闭" onclick={onclose}>×</button>
</header>

<div class="settings-scroll">
  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="type" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>界面基础</strong>
          <p>列表标题、设置文字等主体内容的字体大小</p>
        </div>
        <span class="value-label">{s.fontSizes.base}px</span>
      </div>
    </div>
    <input type="range" min="11" max="20" value={s.fontSizes.base} oninput={sliderHandler("base")} class="transparency-slider" />
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="info" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>描述文字</strong>
          <p>时间戳、来源名称、文件大小等描述性信息</p>
        </div>
        <span class="value-label">{s.fontSizes.secondary}px</span>
      </div>
    </div>
    <input type="range" min="9" max="16" value={s.fontSizes.secondary} oninput={sliderHandler("secondary")} class="transparency-slider" />
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="ruler" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>备注文字</strong>
          <p>标签、标记、角标等最小号文字的字体大小</p>
        </div>
        <span class="value-label">{s.fontSizes.tiny}px</span>
      </div>
    </div>
    <input type="range" min="8" max="13" value={s.fontSizes.tiny} oninput={sliderHandler("tiny")} class="transparency-slider" />
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="text" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>条目标题</strong>
          <p>列表卡片上的标题文字大小</p>
        </div>
        <span class="value-label">{s.fontSizes.cardTitle}px</span>
      </div>
    </div>
    <input type="range" min="10" max="20" value={s.fontSizes.cardTitle} oninput={sliderHandler("cardTitle")} class="transparency-slider" />
  </section>

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="info" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>条目辅助文字</strong>
          <p>列表卡片上的辅助预览/自定义标题首行文字</p>
        </div>
        <span class="value-label">{s.fontSizes.cardPreview}px</span>
      </div>
    </div>
    <input type="range" min="8" max="16" value={s.fontSizes.cardPreview} oninput={sliderHandler("cardPreview")} class="transparency-slider" />
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
      <div>
        <strong>显示辅助文字</strong>
        <p>列表条目下方的小字预览文本</p>
      </div>
    </div>
    <button type="button" class="toggle-switch" class:active={s.display.showSecondaryText}
      onclick={() => updateDisplay({ showSecondaryText: !s.display.showSecondaryText })}
      role="switch" aria-checked={s.display.showSecondaryText} aria-label="显示辅助文字">
      <span class="toggle-knob"></span>
    </button>
  </section>

  <p class="auto-save-note">修改即时生效，无需手动保存</p>
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
    font-size: var(--font-size-tiny, 9.5px);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 {
    margin: 5px 0 4px;
    color: #efefef;
    font-size: var(--font-size-base, 18px);
    font-weight: 590;
  }

  header p {
    max-width: 430px;
    margin: 0;
    color: #777;
    font-size: var(--font-size-secondary, 10.5px);
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
    gap: 8px;
    min-height: 0;
    padding: 14px 18px 48px;
    overflow: auto;
    scrollbar-color: #9a9a9a transparent;
    scrollbar-width: thin;
  }

  .settings-scroll::-webkit-scrollbar { width: 7px; }
  .settings-scroll::-webkit-scrollbar-track { background: transparent; }
  .settings-scroll::-webkit-scrollbar-thumb { border-radius: 10px; background: #858585; }

  .setting-card {
    padding: 10px 13px;
    border: 1px solid #303030;
    border-radius: 9px;
    background: #1e1e1e;
  }

  .setting-heading {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
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

  .heading-inline {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex: 1;
    min-width: 0;
    gap: 8px;
  }

  .setting-heading strong {
    color: #dedede;
    font-size: var(--font-size-base, 11.5px);
    font-weight: 560;
  }

  .setting-heading p {
    margin: 2px 0 0;
    color: #777;
    font-size: var(--font-size-secondary, 9.8px);
  }

  .heading-inline strong {
    display: block;
  }

  .value-label {
    color: #aaa;
    font-size: var(--font-size-secondary, 12px);
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
    background: #2a2a2a;
    outline: none;
    cursor: pointer;
  }

  .transparency-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(to right, #4aa8ff 0%, #4aa8ff var(--slider-pct, 50%), #2a2a2a var(--slider-pct, 50%), #2a2a2a 100%);
  }

  .transparency-slider::-webkit-slider-thumb {
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

  .transparency-slider::-webkit-slider-thumb:hover {
    box-shadow: 0 0 6px rgba(74, 168, 255, 0.4);
    transform: scale(1.15);
  }

  .transparency-slider::-moz-range-track { height: 4px; border-radius: 2px; background: #2a2a2a; }
  .transparency-slider::-moz-range-progress { height: 4px; border-radius: 2px; background: #4aa8ff; }
  .transparency-slider::-moz-range-thumb {
    width: 16px; height: 16px; border-radius: 50%;
    border: 2px solid #4aa8ff; background: #1a1a1a;
    cursor: pointer; transition: box-shadow 100ms ease, transform 100ms ease;
  }
  .transparency-slider::-moz-range-thumb:hover {
    box-shadow: 0 0 6px rgba(74, 168, 255, 0.4);
    transform: scale(1.15);
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: #666;
    font-size: var(--font-size-tiny, 10px);
    text-align: center;
  }

  .toggle-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
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

  button { cursor: pointer; }
</style>
