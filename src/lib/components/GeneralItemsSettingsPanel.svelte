<script lang="ts">
  import SettingEntry from "$lib/components/SettingEntry.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type { CardActionsDisplay, GroupDisplayMode } from "$lib/types/clipboard";
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

  const itemsEntries: SettingEntryConfig[] = $derived([
    {
      type: "select",
      icon: "grid",
      label: _t("general.cardActionsDisplay"),
      desc: _t("general.cardActionsDisplayDescription"),
      get: () => s.cardActionsDisplay,
      options: [
        { value: "hover", label: _t("general.cardActionsHover") },
        { value: "always", label: _t("general.cardActionsAlways") },
      ],
      set: (v) => generalSettings.updateSetting("cardActionsDisplay", v as CardActionsDisplay),
    },
    {
      type: "select",
      icon: "grid",
      label: _t("general.groupDisplayMode"),
      desc: _t("general.groupDisplayModeDescription"),
      get: () => s.groupDisplayMode,
      options: [
        { value: "iconText", label: _t("general.groupDisplayModeIconText") },
        { value: "iconOnly", label: _t("general.groupDisplayModeIconOnly") },
        { value: "textOnly", label: _t("general.groupDisplayModeTextOnly") },
      ],
      set: (v) => generalSettings.updateSetting("groupDisplayMode", v as GroupDisplayMode),
    },
    {
      type: "toggle",
      icon: "copy",
      label: _t("general.quickCopyBadge"),
      desc: _t("general.quickCopyBadgeDescription"),
      get: () => s.quickCopyBadgeAlwaysVisible,
      set: (v) => generalSettings.updateSetting("quickCopyBadgeAlwaysVisible", v),
    },
    {
      type: "toggle",
      icon: "grid",
      label: _t("general.pinCopiedToTop"),
      desc: _t("general.pinCopiedToTopDescription"),
      get: () => s.pinCopiedToTop,
      set: (v) => generalSettings.updateSetting("pinCopiedToTop", v),
    },
    {
      type: "toggle",
      icon: "scan",
      label: _t("general.pasteCleaning"),
      desc: _t("general.pasteCleaningDescription"),
      get: () => s.pasteCleaningEnabled,
      set: (v) => generalSettings.updateSetting("pasteCleaningEnabled", v),
    },
    {
      type: "toggle",
      icon: "clipboard",
      label: _t("general.doubleClickPaste"),
      desc: _t("general.doubleClickPasteDescription"),
      get: () => s.doubleClickPaste,
      set: (v) => generalSettings.updateSetting("doubleClickPaste", v),
    },
    {
      type: "slider",
      icon: "file",
      label: _t("general.pageSize"),
      desc: _t("general.pageSizeDescription"),
      get: () => Math.min(s.display.pageSize, s.pageSizeLimit),
      set: (v) =>
        generalSettings.updateSetting("display", {
          ...s.display,
          pageSize: Math.min(v, s.pageSizeLimit),
        }),
      min: 50,
      max: () => Math.min(s.pageSizeLimit, 300),
      step: 50,
      suffix: ` ${_t("general.pageSizeUnit")}`,
    },
    {
      type: "slider",
      icon: "file",
      label: _t("general.pageSizeLimit"),
      desc: _t("general.pageSizeLimitDescription"),
      get: () => s.pageSizeLimit,
      set: (v) => {
        generalSettings.updateSetting("pageSizeLimit", v);
        if (s.display.pageSize > v) {
          generalSettings.updateSetting("display", { ...s.display, pageSize: v });
        }
      },
      min: 500,
      max: 6000,
      step: 100,
      suffix: ` ${_t("general.pageSizeLimitUnit")}`,
    },
    {
      type: "slider",
      icon: "file",
      label: _t("general.loadTolerance"),
      desc: _t("general.loadToleranceDescription"),
      get: () => s.loadTolerance,
      set: (v) => generalSettings.updateSetting("loadTolerance", v),
      min: 50,
      max: 500,
      step: 50,
      suffix: ` ${_t("general.loadToleranceUnit")}`,
    },
    {
      type: "toggle",
      icon: "eye",
      label: _t("general.showSecondaryText"),
      desc: _t("general.showSecondaryTextDescription"),
      get: () => s.display.showSecondaryText,
      set: (v) => generalSettings.updateSetting("display", { ...s.display, showSecondaryText: v }),
    },
    {
      type: "slider",
      icon: "text",
      label: _t("general.maxTextLines"),
      desc: _t("general.maxTextLinesDescription"),
      get: () => s.display.maxTextLines,
      set: (v) => generalSettings.updateSetting("display", { ...s.display, maxTextLines: v }),
      min: 1,
      max: 12,
      suffix: ` ${_t("general.maxTextLinesUnit")}`,
    },
  ]);
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("general.eyebrow")}</span>
      <h2>{_t("storage.generalItemsTab")}</h2>
      <p>{_t("storage.generalItemsDescription")}</p>
    </div>
    {#if s.showSettingsCloseButton}
      <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
        >×</button
      >
    {/if}
  </header>
{/if}

<div class="settings-scroll">
  {#each itemsEntries as config}
    <SettingEntry {config} />
  {/each}
</div>
