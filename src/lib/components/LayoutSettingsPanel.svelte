<script lang="ts">
  import SettingEntry from "$lib/components/SettingEntry.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";
  import type { SettingEntryConfig } from "$lib/types/settings-entry";

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

  const layoutEntries: SettingEntryConfig[] = $derived([
    {
      type: "slider",
      icon: "sliders",
      label: _t("layout.paddingTop"),
      desc: _t("layout.paddingTopDescription"),
      get: () => s.cardPaddingTop,
      set: (v) => generalSettings.updateSetting("cardPaddingTop", v),
      min: 0,
      max: 20,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "sliders",
      label: _t("layout.paddingBottom"),
      desc: _t("layout.paddingBottomDescription"),
      get: () => s.cardPaddingBottom,
      set: (v) => generalSettings.updateSetting("cardPaddingBottom", v),
      min: 0,
      max: 20,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "ruler",
      label: _t("layout.cardGap"),
      desc: _t("layout.cardGapDescription"),
      get: () => s.cardGap,
      set: (v) => generalSettings.updateSetting("cardGap", v),
      min: 0,
      max: 20,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "text",
      label: _t("layout.shortTextHeight"),
      desc: _t("layout.shortTextHeightDescription"),
      get: () => s.cardTextHeight,
      set: (v) => generalSettings.updateSetting("cardTextHeight", v),
      min: 36,
      max: 90,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "text",
      label: _t("layout.tallTextHeight"),
      desc: _t("layout.tallTextHeightDescription"),
      get: () => s.cardTallTextHeight,
      set: (v) => generalSettings.updateSetting("cardTallTextHeight", v),
      min: 42,
      max: 100,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "type",
      label: _t("layout.customTitleHeight"),
      desc: _t("layout.customTitleHeightDescription"),
      get: () => s.cardCustomTitleHeight,
      set: (v) => generalSettings.updateSetting("cardCustomTitleHeight", v),
      min: 40,
      max: 120,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "image",
      label: _t("layout.imageHeight"),
      desc: _t("layout.imageHeightDescription"),
      get: () => s.cardImageHeight,
      set: (v) => generalSettings.updateSetting("cardImageHeight", v),
      min: 64,
      max: 200,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "search",
      label: _t("layout.searchHeight"),
      desc: _t("layout.searchHeightDescription"),
      get: () => s.searchHeight,
      set: (v) => generalSettings.updateSetting("searchHeight", v),
      min: 28,
      max: 56,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "type",
      label: _t("layout.searchFontSize"),
      desc: _t("layout.searchFontSizeDescription"),
      get: () => s.searchFontSize,
      set: (v) => generalSettings.updateSetting("searchFontSize", v),
      min: 10,
      max: 24,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "grid",
      label: _t("layout.cardBorderRadius"),
      desc: _t("layout.cardBorderRadiusDescription"),
      get: () => s.cardBorderRadius,
      set: (v) => generalSettings.updateSetting("cardBorderRadius", v),
      min: 0,
      max: 20,
      suffix: "px",
    },
    {
      type: "select",
      icon: "grid",
      label: _t("general.detailDisplayMode"),
      desc: _t("general.detailDisplayModeDescription"),
      get: () => s.detailDisplayMode,
      options: [
        { value: "overlay", label: _t("general.detailDisplayModeOverlay") },
        { value: "split", label: _t("general.detailDisplayModeSplit") },
      ],
      set: (v) => generalSettings.updateSetting("detailDisplayMode", v as "overlay" | "split"),
    },
  ]);
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("layout.eyebrow")}</span>
      <h2>{_t("layout.title")}</h2>
      <p>{_t("layout.description")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  {#each layoutEntries as config}
    <SettingEntry {config} />
  {/each}
</div>
