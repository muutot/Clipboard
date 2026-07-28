<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";
  import { updateSliderTrack } from "$lib/utils/format";
  import { applyFontSizesToDocument } from "$lib/services/settings-bootstrap";
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

  interface FontSliderDef {
    key: keyof typeof s.fontSizes;
    icon: string;
    label: string;
    desc: string;
    min: number;
    max: number;
  }

  const interfaceSliders: FontSliderDef[] = [
    {
      key: "base",
      icon: "type",
      label: _t("settings.fontSizeBaseLabel"),
      desc: _t("settings.fontSizeBaseDescription"),
      min: 11,
      max: 20,
    },
    {
      key: "secondary",
      icon: "info",
      label: _t("settings.fontSizeSecondaryLabel"),
      desc: _t("settings.fontSizeSecondaryDescription"),
      min: 9,
      max: 16,
    },
    {
      key: "tiny",
      icon: "ruler",
      label: _t("settings.fontSizeTinyLabel"),
      desc: _t("settings.fontSizeTinyDescription"),
      min: 8,
      max: 13,
    },
  ];

  const cardSliders: FontSliderDef[] = [
    {
      key: "cardTitle",
      icon: "text",
      label: _t("settings.fontSizeCardTitleLabel"),
      desc: _t("settings.fontSizeCardTitleDescription"),
      min: 10,
      max: 20,
    },
    {
      key: "cardPreview",
      icon: "info",
      label: _t("settings.fontSizeCardPreviewLabel"),
      desc: _t("settings.fontSizeCardPreviewDescription"),
      min: 8,
      max: 16,
    },
  ];
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  function applyFontSize(category: keyof typeof s.fontSizes, value: number) {
    const updated = { ...s.fontSizes, [category]: value };
    generalSettings.updateSetting("fontSizes", updated);
    emit("settings-font-changed", { fontSizes: updated, display: s.display }).catch((err) => console.warn("settings-font-changed emit failed:", err));
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
    applyFontSizesToDocument(s.fontSizes, s.display);
  });
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("settings.fontSizePanelEyebrow")}</span>
      <h2>{_t("settings.fontSizePanel")}</h2>
      <p>{_t("settings.fontSizePanelDescription")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  <nav class="font-subnav">
    <button class:active={fontSection === "interface"} onclick={() => (fontSection = "interface")}
      >{_t("settings.fontSizeInterfaceTab")}</button
    >
    <button class:active={fontSection === "card"} onclick={() => (fontSection = "card")}
      >{_t("settings.fontSizeCardTab")}</button
    >
  </nav>

  {#if fontSection === "interface"}
    {#each interfaceSliders as slider}
      <section class="setting-card">
        <div class="setting-heading">
          <span class="setting-icon"><AppIcon name={slider.icon as any} size={17} /></span>
          <div class="heading-inline">
            <div>
              <strong>{slider.label}</strong>
              <p>{slider.desc}</p>
            </div>
            <label class="font-size-control">
              <input
                class="font-size-input"
                type="number"
                min={slider.min}
                max={slider.max}
                step="1"
                value={s.fontSizes[slider.key]}
                oninput={numberInputHandler(slider.key, slider.min, slider.max)}
                onblur={numberBlurHandler(slider.key, slider.min, slider.max)}
                aria-label={slider.label}
              />
              <span>px</span>
            </label>
          </div>
        </div>
        <input
          type="range"
          min={slider.min}
          max={slider.max}
          value={s.fontSizes[slider.key]}
          oninput={sliderHandler(slider.key)}
          class="transparency-slider"
        />
      </section>
    {/each}
  {:else}
    {#each cardSliders as slider}
      <section class="setting-card">
        <div class="setting-heading">
          <span class="setting-icon"><AppIcon name={slider.icon as any} size={17} /></span>
          <div class="heading-inline">
            <div>
              <strong>{slider.label}</strong>
              <p>{slider.desc}</p>
            </div>
            <label class="font-size-control">
              <input
                class="font-size-input"
                type="number"
                min={slider.min}
                max={slider.max}
                step="1"
                value={s.fontSizes[slider.key]}
                oninput={numberInputHandler(slider.key, slider.min, slider.max)}
                onblur={numberBlurHandler(slider.key, slider.min, slider.max)}
                aria-label={slider.label}
              />
              <span>px</span>
            </label>
          </div>
        </div>
        <input
          type="range"
          min={slider.min}
          max={slider.max}
          value={s.fontSizes[slider.key]}
          oninput={sliderHandler(slider.key)}
          class="transparency-slider"
        />
      </section>
    {/each}
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
