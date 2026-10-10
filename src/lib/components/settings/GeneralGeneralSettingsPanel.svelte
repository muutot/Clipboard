<script lang="ts">
  import SettingEntry from "$lib/components/settings/SettingEntry.svelte";
  import { messages, resolvePath, setLocale } from "$lib/i18n";
  import type { GeneralSettings, Language } from "$lib/types/clipboard";
  import { generalSettings, windowConfig } from "$lib/services/settings";
  import { onDestroy } from "svelte";
  import { createFeedback } from "$lib/utils/feedback.svelte";
  import type { SettingEntryConfig } from "$lib/types/settings-entry";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let s = $state($generalSettings);
  let wc = $state($windowConfig);
  let feedback = createFeedback(2000);

  onDestroy(() => feedback.dispose());

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  $effect(() => {
    const unsub = windowConfig.subscribe((v) => {
      wc = v;
    });
    return unsub;
  });

  $effect(() => {
    void windowConfig.ensureLoaded();
  });

  async function changeWindowSetting(key: "launchAtStartup" | "closeToTray", value: boolean) {
    try {
      await windowConfig.update({ [key]: value });
    } catch {
      feedback.show(_t("general.windowConfigUpdateFailed"), false);
    }
  }

  function changeLanguage(lang: Language) {
    generalSettings.updateSetting("language", lang);
    setLocale(lang);
    const key =
      lang === "zh-CN"
        ? "general.languageSwitchedZh"
        : lang === "en"
          ? "general.languageSwitchedEn"
          : "general.languageSwitchedSystem";
    feedback.show(_t(key), true);
  }

  const generalEntries: SettingEntryConfig[] = $derived([
    {
      type: "select",
      id: "general.language",
      icon: "globe",
      label: _t("general.language"),
      desc: _t("general.languageDescription"),
      get: () => s.language,
      set: (v) => changeLanguage(v as Language),
      options: [
        { value: "system", label: _t("general.languageSystem") },
        { value: "zh-CN", label: "中文" },
        { value: "en", label: "English" },
      ],
    },
    {
      type: "toggle",
      id: "general.launch-at-startup",
      icon: "clock",
      label: _t("general.launchAtStartup"),
      desc: _t("general.launchAtStartupDescription"),
      get: () => wc.launchAtStartup,
      set: (v) => void changeWindowSetting("launchAtStartup", v),
    },
    {
      type: "toggle",
      id: "general.close-to-tray",
      icon: "clipboard",
      label: _t("general.closeToTray"),
      desc: _t("general.closeToTrayDescription"),
      get: () => wc.closeToTray,
      set: (v) => void changeWindowSetting("closeToTray", v),
    },
    {
      type: "toggle",
      icon: "trash",
      label: _t("general.useRecycleBin"),
      desc: _t("general.useRecycleBinDescription"),
      get: () => s.useRecycleBin,
      set: (v) => generalSettings.updateSetting("useRecycleBin", v),
    },
    {
      type: "toggle",
      icon: "info",
      label: _t("general.toastNotifications"),
      desc: _t("general.toastNotificationsDescription"),
      get: () => s.showToastNotifications,
      set: (v) => generalSettings.updateSetting("showToastNotifications", v),
    },
    {
      type: "toggle",
      icon: "copy",
      label: _t("general.useSystemTitleBar"),
      desc: _t("general.useSystemTitleBarDescription"),
      get: () => s.useSystemTitleBar,
      set: (v) => generalSettings.updateSetting("useSystemTitleBar", v),
    },
    {
      type: "toggle",
      icon: "x",
      label: _t("general.showSettingsCloseButton"),
      desc: _t("general.showSettingsCloseButtonDescription"),
      get: () => s.showSettingsCloseButton,
      set: (v) => generalSettings.updateSetting("showSettingsCloseButton", v),
    },
    {
      type: "select",
      id: "general.log-level",
      icon: "info",
      label: _t("general.logLevel"),
      desc: _t("general.logLevelDescription"),
      get: () => s.logLevel,
      set: (v) => generalSettings.updateSetting("logLevel", v as GeneralSettings["logLevel"]),
      options: [
        { value: "error", label: _t("general.logLevelError") },
        { value: "warn", label: _t("general.logLevelWarn") },
        { value: "info", label: _t("general.logLevelInfo") },
        { value: "debug", label: _t("general.logLevelDebug") },
      ],
    },
  ]);
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("general.eyebrow")}</span>
      <h2>{_t("storage.generalGeneralTab")}</h2>
      <p>{_t("storage.generalGeneralDescription")}</p>
    </div>
    {#if s.showSettingsCloseButton}
      <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
        >×</button
      >
    {/if}
  </header>
{/if}

<div class="settings-scroll">
  {#each generalEntries as config}
    <SettingEntry {config} />
  {/each}
</div>

{#if feedback.message}
  <div class:success={feedback.success} class="settings-feedback">{feedback.message}</div>
{/if}
