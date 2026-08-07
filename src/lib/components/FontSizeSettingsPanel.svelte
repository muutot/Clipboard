<script lang="ts">
  import SettingEntry from "$lib/components/SettingEntry.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";
  import { applyFontSizesToDocument } from "$lib/services/settings-bootstrap";
  import type { SettingEntryConfig } from "$lib/types/settings-entry";
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

  function sliderEntry(
    key: keyof typeof s.fontSizes,
    icon: string,
    label: string,
    desc: string,
    min: number,
    max: number,
  ): SettingEntryConfig {
    return {
      type: "slider",
      icon: icon as any,
      label,
      desc,
      get: () => s.fontSizes[key],
      set: (v) => {
        const updated = { ...s.fontSizes, [key]: v };
        generalSettings.updateSetting("fontSizes", updated);
        emit("settings-font-changed", { fontSizes: updated, display: s.display }).catch((err) =>
          console.warn("settings-font-changed emit failed:", err),
        );
      },
      min,
      max,
      suffix: "px",
    };
  }

  const interfaceSliders: SettingEntryConfig[] = $derived([
    sliderEntry(
      "base",
      "type",
      _t("general.fontSizeBaseLabel"),
      _t("general.fontSizeBaseDescription"),
      11,
      20,
    ),
    sliderEntry(
      "secondary",
      "info",
      _t("general.fontSizeSecondaryLabel"),
      _t("general.fontSizeSecondaryDescription"),
      9,
      16,
    ),
    sliderEntry(
      "tiny",
      "ruler",
      _t("general.fontSizeTinyLabel"),
      _t("general.fontSizeTinyDescription"),
      8,
      13,
    ),
  ]);

  const cardSliders: SettingEntryConfig[] = $derived([
    sliderEntry(
      "cardTitle",
      "text",
      _t("general.fontSizeCardTitleLabel"),
      _t("general.fontSizeCardTitleDescription"),
      10,
      20,
    ),
    sliderEntry(
      "cardPreview",
      "info",
      _t("general.fontSizeCardPreviewLabel"),
      _t("general.fontSizeCardPreviewDescription"),
      8,
      16,
    ),
  ]);
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
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
      <SettingEntry config={slider} />
    {/each}
  {:else}
    {#each cardSliders as slider}
      <SettingEntry config={slider} />
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
</style>
