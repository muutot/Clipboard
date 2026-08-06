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
      label: _t("general.fontSizeBaseLabel"),
      desc: _t("general.fontSizeBaseDescription"),
      min: 11,
      max: 20,
    },
    {
      key: "secondary",
      icon: "info",
      label: _t("general.fontSizeSecondaryLabel"),
      desc: _t("general.fontSizeSecondaryDescription"),
      min: 9,
      max: 16,
    },
    {
      key: "tiny",
      icon: "ruler",
      label: _t("general.fontSizeTinyLabel"),
      desc: _t("general.fontSizeTinyDescription"),
      min: 8,
      max: 13,
    },
  ];

  const cardSliders: FontSliderDef[] = [
    {
      key: "cardTitle",
      icon: "text",
      label: _t("general.fontSizeCardTitleLabel"),
      desc: _t("general.fontSizeCardTitleDescription"),
      min: 10,
      max: 20,
    },
    {
      key: "cardPreview",
      icon: "info",
      label: _t("general.fontSizeCardPreviewLabel"),
      desc: _t("general.fontSizeCardPreviewDescription"),
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
    emit("settings-font-changed", { fontSizes: updated, display: s.display }).catch((err) =>
      console.warn("settings-font-changed emit failed:", err),
    );
  }

  function sliderHandler(category: keyof typeof s.fontSizes) {
    return (event: Event) => {
      const val = Number((event.target as HTMLInputElement).value);
      applyFontSize(category, val);
      updateSliderTrack(event.target as HTMLInputElement);
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
      <span class="eyebrow">{_t("general.fontSizePanelEyebrow")}</span>
      <h2>{_t("general.fontSizePanel")}</h2>
      <p>{_t("general.fontSizePanelDescription")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  <nav class="font-subnav">
    <button class:active={fontSection === "interface"} onclick={() => (fontSection = "interface")}
      >{_t("general.fontSizeInterfaceTab")}</button
    >
    <button class:active={fontSection === "card"} onclick={() => (fontSection = "card")}
      >{_t("general.fontSizeCardTab")}</button
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
            <span class="value-label">{s.fontSizes[slider.key]}px</span>
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
            <span class="value-label">{s.fontSizes[slider.key]}px</span>
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

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
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
</style>
