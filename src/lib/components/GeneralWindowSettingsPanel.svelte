<script lang="ts">
  import SettingEntry from "$lib/components/SettingEntry.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type { FloatPanelPosition, WindowEffect } from "$lib/types/clipboard";
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

  const windowEntries: SettingEntryConfig[] = $derived([
    {
      type: "slider",
      icon: "sliders",
      label: _t("general.windowTransparency"),
      get: () => s.windowTransparency,
      set: (v) => generalSettings.updateSetting("windowTransparency", v),
      min: 60,
      max: 100,
      suffix: "%",
    },
    {
      type: "select",
      icon: "sliders",
      label: _t("general.windowEffect"),
      desc: _t("general.windowEffectDescription"),
      get: () => s.windowEffect,
      options: [
        { value: "off", label: _t("general.windowEffectOff") },
        { value: "acrylic", label: _t("general.windowEffectAcrylic") },
        { value: "mica", label: _t("general.windowEffectMica") },
      ],
      set: (v) => generalSettings.updateSetting("windowEffect", v as WindowEffect),
    },
    {
      type: "toggle",
      icon: "eye",
      label: _t("general.windowOpacityAffectsText"),
      desc: _t("general.windowOpacityAffectsTextDescription"),
      get: () => s.windowOpacityAffectsText,
      set: (v) => generalSettings.updateSetting("windowOpacityAffectsText", v),
    },
    {
      type: "toggle",
      icon: "pin",
      label: _t("general.rememberWindowPosition"),
      desc: _t("general.rememberWindowPositionDescription"),
      get: () => s.rememberWindowPosition,
      set: (v) => generalSettings.updateSetting("rememberWindowPosition", v),
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
    {
      type: "select",
      icon: "layers",
      label: _t("general.floatPanelPosition"),
      desc: _t("general.floatPanelPositionDescription"),
      get: () => s.floatPanelPosition,
      options: [
        { value: "topLeft", label: _t("general.floatPanelTopLeft") },
        { value: "topRight", label: _t("general.floatPanelTopRight") },
        { value: "bottomLeft", label: _t("general.floatPanelBottomLeft") },
        { value: "bottomRight", label: _t("general.floatPanelBottomRight") },
        { value: "center", label: _t("general.floatPanelCenter") },
      ],
      set: (v) => generalSettings.updateSetting("floatPanelPosition", v as FloatPanelPosition),
    },
    {
      type: "toggle",
      icon: "maximize",
      label: _t("general.desktopFullscreen"),
      desc: _t("general.desktopFullscreenDescription"),
      get: () => s.imageFullscreenMode === "desktop",
      set: (v) => generalSettings.updateSetting("imageFullscreenMode", v ? "desktop" : "overlay"),
    },
    {
      type: "slider",
      icon: "image",
      label: _t("general.viewerBackdropOpacity"),
      get: () => s.viewerBackdropOpacity,
      set: (v) => generalSettings.updateSetting("viewerBackdropOpacity", v),
      min: 0,
      max: 100,
      suffix: "%",
    },
  ]);
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("general.eyebrow")}</span>
      <h2>{_t("storage.generalWindowTab")}</h2>
      <p>{_t("storage.generalWindowDescription")}</p>
    </div>
    {#if s.showSettingsCloseButton}
      <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
        >×</button
      >
    {/if}
  </header>
{/if}

<div class="settings-scroll">
  {#each windowEntries as config}
    <SettingEntry {config} />
  {/each}
</div>
