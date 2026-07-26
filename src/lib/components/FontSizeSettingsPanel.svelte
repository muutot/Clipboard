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
    document.documentElement.style.setProperty(
      "--show-secondary",
      s.display.showSecondaryText ? "block" : "none",
    );
  });
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">设置 / 显示</span>
      <h2>字体大小</h2>
      <p>为不同 UI 区域单独调整字体大小</p>
    </div>
    <button class="close-button" type="button" aria-label="关闭" onclick={onclose}>×</button>
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="type" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>界面基础</strong>
          <p>列表标题、设置文字等主体内容的字体大小</p>
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
            aria-label="界面基础字号"
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
          <strong>描述文字</strong>
          <p>时间戳、来源名称、文件大小等描述性信息</p>
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
            aria-label="描述文字字号"
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
          <strong>备注文字</strong>
          <p>标签、标记、角标等最小号文字的字体大小</p>
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
            aria-label="备注文字字号"
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

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="text" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>条目标题</strong>
          <p>列表卡片上的标题文字大小</p>
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
          <strong>条目辅助文字</strong>
          <p>列表卡片上的辅助预览/自定义标题首行文字</p>
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
            aria-label="条目辅助文字字号"
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

  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="text" size={17} /></span>
      <div class="heading-inline">
        <div>
          <strong>{_t("font.secondaryTextLines")}</strong>
          <p>{_t("font.secondaryTextLinesDescription")}</p>
        </div>
        <span class="value-label">{s.display.maxTextLines} {_t("font.unitLines")}</span>
      </div>
    </div>
    <input
      type="range"
      min="1"
      max="12"
      value={s.display.maxTextLines}
      oninput={(event) => {
        const input = event.target as HTMLInputElement;
        updateDisplay({ maxTextLines: Number(input.value) });
        updateSliderTrack(input);
      }}
      class="transparency-slider"
    />
  </section>

  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
      <div>
        <strong>显示辅助文字</strong>
        <p>列表条目下方的小字预览文本</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.display.showSecondaryText}
      onclick={() => updateDisplay({ showSecondaryText: !s.display.showSecondaryText })}
      role="switch"
      aria-checked={s.display.showSecondaryText}
      aria-label="显示辅助文字"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <p class="auto-save-note">修改即时生效，无需手动保存</p>
</div>

<style>
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
