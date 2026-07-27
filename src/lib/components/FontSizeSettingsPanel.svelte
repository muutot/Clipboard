<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";
  import { emit } from "@tauri-apps/api/event";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let s = $state($generalSettings);
  let fontSection = $state<"interface" | "card">("interface");
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
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
    emit("settings-font-changed", {
      fontSizes: { ...s.fontSizes, [category]: value },
      display: s.display,
    }).catch(() => {});
  }

  function sliderHandler(category: keyof typeof s.fontSizes) {
    return (event: Event) => {
      const val = Number((event.target as HTMLInputElement).value);
      applyFontSize(category, val);
      updateSliderTrack(event.target as HTMLInputElement);
    };
  }

  function commitNumberInput(
    input: HTMLInputElement,
    category: keyof typeof s.fontSizes,
    min: number,
    max: number,
  ) {
    const parsed = Number(input.value);
    const fallback = s.fontSizes[category];
    const value = Number.isFinite(parsed)
      ? Math.round(Math.min(max, Math.max(min, parsed)))
      : fallback;
    input.value = String(value);
    applyFontSize(category, value);
  }

  function numberInputHandler(category: keyof typeof s.fontSizes, min: number, max: number) {
    return (event: Event) => {
      const input = event.target as HTMLInputElement;
      if (input.value.trim() !== "") commitNumberInput(input, category, min, max);
    };
  }

  function numberBlurHandler(category: keyof typeof s.fontSizes, min: number, max: number) {
    return (event: Event) => {
      commitNumberInput(event.target as HTMLInputElement, category, min, max);
    };
  }

  $effect(() => {
    fontSection;
    const el = document.querySelectorAll<HTMLInputElement>(".transparency-slider");
    el.forEach(updateSliderTrack);
  });

  $effect(() => {
    document.documentElement.style.fontSize = `${s.fontSizes.base}px`;
    document.documentElement.style.setProperty("--font-size-base", `${s.fontSizes.base}px`);
    document.documentElement.style.setProperty(
      "--font-size-secondary",
      `${s.fontSizes.secondary}px`,
    );
    document.documentElement.style.setProperty("--font-size-tiny", `${s.fontSizes.tiny}px`);
    document.documentElement.style.setProperty(
      "--font-size-cardTitle",
      `${s.fontSizes.cardTitle}px`,
    );
    document.documentElement.style.setProperty(
      "--font-size-cardPreview",
      `${s.fontSizes.cardPreview}px`,
    );
  });
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">设置 / 显示</span>
      <h2>字体大小</h2>
      <p>为不同界面区域单独调整字体大小</p>
    </div>
    <button class="close-button" type="button" aria-label="关闭" onclick={onclose}>×</button>
  </header>
{/if}

<div class="settings-scroll">
  <nav class="font-subnav">
    <button class:active={fontSection === "interface"} onclick={() => (fontSection = "interface")}
      >界面文字</button
    >
    <button class:active={fontSection === "card"} onclick={() => (fontSection = "card")}
      >卡片文字</button
    >
  </nav>

  {#if fontSection === "interface"}
    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="type" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>主文字</strong>
            <p>设置界面标题、详情面板等正文的字体大小</p>
          </div>
          <label class="font-size-control">
            <input
              class="font-size-input"
              type="number"
              min="11"
              max="20"
              step="1"
              value={s.fontSizes.base}
              oninput={numberInputHandler("base", 11, 20)}
              onblur={numberBlurHandler("base", 11, 20)}
              aria-label="主文字字号"
            />
            <span>px</span>
          </label>
        </div>
      </div>
      <input
        type="range"
        min="11"
        max="20"
        value={s.fontSizes.base}
        oninput={sliderHandler("base")}
        class="transparency-slider"
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="info" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>副文字</strong>
            <p>时间戳、来源名称等辅助信息的字体大小</p>
          </div>
          <label class="font-size-control">
            <input
              class="font-size-input"
              type="number"
              min="9"
              max="16"
              step="1"
              value={s.fontSizes.secondary}
              oninput={numberInputHandler("secondary", 9, 16)}
              onblur={numberBlurHandler("secondary", 9, 16)}
              aria-label="副文字字号"
            />
            <span>px</span>
          </label>
        </div>
      </div>
      <input
        type="range"
        min="9"
        max="16"
        value={s.fontSizes.secondary}
        oninput={sliderHandler("secondary")}
        class="transparency-slider"
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="ruler" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>小文字</strong>
            <p>面包屑、保存提示等最小号文字的字体大小</p>
          </div>
          <label class="font-size-control">
            <input
              class="font-size-input"
              type="number"
              min="8"
              max="13"
              step="1"
              value={s.fontSizes.tiny}
              oninput={numberInputHandler("tiny", 8, 13)}
              onblur={numberBlurHandler("tiny", 8, 13)}
              aria-label="小文字字号"
            />
            <span>px</span>
          </label>
        </div>
      </div>
      <input
        type="range"
        min="8"
        max="13"
        value={s.fontSizes.tiny}
        oninput={sliderHandler("tiny")}
        class="transparency-slider"
      />
    </section>
  {:else}
    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="text" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>标题</strong>
            <p>列表卡片上条目标题的字体大小</p>
          </div>
          <label class="font-size-control">
            <input
              class="font-size-input"
              type="number"
              min="10"
              max="20"
              step="1"
              value={s.fontSizes.cardTitle}
              oninput={numberInputHandler("cardTitle", 10, 20)}
              onblur={numberBlurHandler("cardTitle", 10, 20)}
              aria-label="条目标题字号"
            />
            <span>px</span>
          </label>
        </div>
      </div>
      <input
        type="range"
        min="10"
        max="20"
        value={s.fontSizes.cardTitle}
        oninput={sliderHandler("cardTitle")}
        class="transparency-slider"
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="info" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>预览</strong>
            <p>列表卡片上条目预览正文的字体大小</p>
          </div>
          <label class="font-size-control">
            <input
              class="font-size-input"
              type="number"
              min="8"
              max="16"
              step="1"
              value={s.fontSizes.cardPreview}
              oninput={numberInputHandler("cardPreview", 8, 16)}
              onblur={numberBlurHandler("cardPreview", 8, 16)}
              aria-label="预览文字字号"
            />
            <span>px</span>
          </label>
        </div>
      </div>
      <input
        type="range"
        min="8"
        max="16"
        value={s.fontSizes.cardPreview}
        oninput={sliderHandler("cardPreview")}
        class="transparency-slider"
      />
    </section>
  {/if}

  <p class="auto-save-note">修改即时生效，无需手动保存</p>
</div>

<style>
  .font-subnav {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
  }

  .font-subnav button {
    min-height: 28px;
    padding: 5px 12px;
    border: 1px solid transparent;
    border-radius: var(--settings-control-radius);
    color: var(--text-muted);
    background: transparent;
    font: inherit;
    font-size: var(--settings-heading-size);
    font-weight: 560;
    cursor: pointer;
    transition:
      color 100ms ease,
      background 100ms ease,
      border-color 100ms ease;
  }

  .font-subnav button:hover {
    border-color: var(--border-color);
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .font-subnav button.active {
    border-color: var(--selection-color);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }

  .heading-inline strong {
    display: block;
  }

  .font-size-control {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: 0 0 auto;
    margin: 0;
    color: var(--text-muted);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  .font-size-input {
    width: 46px;
    box-sizing: border-box;
    padding: 5px 6px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    outline: none;
    color: var(--text-primary);
    background: var(--input-bg);
    font: inherit;
    font-variant-numeric: tabular-nums;
    text-align: right;
    transition: border-color 120ms ease;
  }

  .font-size-input:focus {
    border-color: var(--text-faint);
  }

  .font-size-input[type="number"] {
    appearance: textfield;
    -moz-appearance: textfield;
  }

  .font-size-input[type="number"]::-webkit-inner-spin-button,
  .font-size-input[type="number"]::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
</style>
