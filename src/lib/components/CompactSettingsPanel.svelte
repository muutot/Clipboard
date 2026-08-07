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

  const compactEntries: SettingEntryConfig[] = [
    {
      type: "toggle",
      icon: "grid",
      label: _t("general.compactMode"),
      desc: _t("general.compactModeDescription"),
      get: () => s.compactMode,
      set: (v) => generalSettings.updateSetting("compactMode", v),
    },
    {
      type: "slider",
      icon: "sliders",
      label: _t("compact.paddingTop"),
      desc: _t("compact.paddingTopDescription"),
      get: () => s.compactPaddingTop,
      set: (v) => generalSettings.updateSetting("compactPaddingTop", v),
      min: 0,
      max: 20,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "sliders",
      label: _t("compact.paddingBottom"),
      desc: _t("compact.paddingBottomDescription"),
      get: () => s.compactPaddingBottom,
      set: (v) => generalSettings.updateSetting("compactPaddingBottom", v),
      min: 0,
      max: 20,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "ruler",
      label: _t("compact.cardGap"),
      desc: _t("compact.cardGapDescription"),
      get: () => s.compactCardGap,
      set: (v) => generalSettings.updateSetting("compactCardGap", v),
      min: 0,
      max: 20,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "text",
      label: _t("compact.shortTextHeight"),
      desc: _t("compact.shortTextHeightDescription"),
      get: () => s.compactTextHeight,
      set: (v) => generalSettings.updateSetting("compactTextHeight", v),
      min: 36,
      max: 90,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "text",
      label: _t("compact.tallTextHeight"),
      desc: _t("compact.tallTextHeightDescription"),
      get: () => s.compactTallTextHeight,
      set: (v) => generalSettings.updateSetting("compactTallTextHeight", v),
      min: 44,
      max: 100,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "image",
      label: _t("compact.imageHeight"),
      desc: _t("compact.imageHeightDescription"),
      get: () => s.compactImageHeight,
      set: (v) => generalSettings.updateSetting("compactImageHeight", v),
      min: 64,
      max: 200,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "search",
      label: _t("compact.searchHeight"),
      desc: _t("compact.searchHeightDescription"),
      get: () => s.compactSearchHeight,
      set: (v) => generalSettings.updateSetting("compactSearchHeight", v),
      min: 28,
      max: 56,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "type",
      label: _t("compact.searchFontSize"),
      desc: _t("compact.searchFontSizeDescription"),
      get: () => s.compactSearchFontSize,
      set: (v) => generalSettings.updateSetting("compactSearchFontSize", v),
      min: 10,
      max: 24,
      suffix: "px",
    },
    {
      type: "slider",
      icon: "grid",
      label: _t("compact.cardBorderRadius"),
      desc: _t("compact.cardBorderRadiusDescription"),
      get: () => s.compactCardBorderRadius,
      set: (v) => generalSettings.updateSetting("compactCardBorderRadius", v),
      min: 0,
      max: 20,
      suffix: "px",
    },
  ];
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
  <SettingEntry config={compactEntries[0]} />

  {#if s.compactMode}
    {#each compactEntries.slice(1) as config}
      <SettingEntry {config} />
    {/each}
  {/if}

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>
