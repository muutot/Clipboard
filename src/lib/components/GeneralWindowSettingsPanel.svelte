<script lang="ts">
  import SettingEntry from "$lib/components/SettingEntry.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type { FloatPanelPosition, WindowConfig, WindowEffect } from "$lib/types/clipboard";
  import { generalSettings, getWindowConfig, setWindowConfig } from "$lib/services/settings";
  import { createFeedback } from "$lib/utils/feedback.svelte";
  import type { SettingEntryConfig } from "$lib/types/settings-entry";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  let _cachedWindowConfig: WindowConfig | null = null;

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let s = $state($generalSettings);
  let feedback = createFeedback(2000);
  let windowConfig = $state<WindowConfig | null>(
    _cachedWindowConfig ?? { launchAtStartup: false, closeToTray: true, singleInstance: true },
  );
  let windowConfigLoading = $state(!_cachedWindowConfig);
  let windowConfigSaving = $state(false);

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  $effect(() => {
    let cancelled = false;
    void getWindowConfig()
      .then((config) => {
        if (!cancelled) {
          _cachedWindowConfig = config;
          windowConfig = config;
        }
      })
      .catch(() => {
        if (!cancelled) feedback.show(_t("general.windowConfigLoadFailed"), false);
      })
      .finally(() => {
        if (!cancelled) windowConfigLoading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  async function changeWindowSetting(key: "launchAtStartup" | "closeToTray", value: boolean) {
    if (!windowConfig || windowConfigSaving) return;
    const previous = windowConfig;
    windowConfig = { ...previous, [key]: value };
    windowConfigSaving = true;
    try {
      await setWindowConfig({ [key]: value });
    } catch {
      windowConfig = previous;
      feedback.show(_t("general.windowConfigUpdateFailed"), false);
    } finally {
      windowConfigSaving = false;
    }
  }

  const windowEntries: SettingEntryConfig[] = $derived([
    {
      type: "toggle",
      icon: "clock",
      label: _t("general.launchAtStartup"),
      desc: _t("general.launchAtStartupDescription"),
      get: () => windowConfig?.launchAtStartup ?? false,
      set: (v) => void changeWindowSetting("launchAtStartup", v),
      disabled: () => windowConfigLoading || windowConfigSaving || !windowConfig,
    },
    {
      type: "toggle",
      icon: "clipboard",
      label: _t("general.closeToTray"),
      desc: _t("general.closeToTrayDescription"),
      get: () => windowConfig?.closeToTray ?? false,
      set: (v) => void changeWindowSetting("closeToTray", v),
      disabled: () => windowConfigLoading || windowConfigSaving || !windowConfig,
    },
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

{#if feedback.message}
  <div class:success={feedback.success} class="settings-feedback">{feedback.message}</div>
{/if}
