<script lang="ts">
  import SettingEntry from "$lib/components/SettingEntry.svelte";
  import { messages, resolvePath, locale } from "$lib/i18n";
  import type { Locale } from "$lib/i18n/types";
  import { generalSettings } from "$lib/services/settings";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getRuntimeInfo, isTauriRuntime } from "$lib/services/runtime";
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
  let feedback = createFeedback(2000);
  let privacyPaused = $state(false);
  let privacyLoading = $state(true);
  // True on desktop macOS/Linux, where clipboard capture polls instead of
  // using native monitoring and self-trigger marking is unavailable.
  let nonWindowsDesktop = $state(false);

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  onMount(() => {
    let disposed = false;
    let unlistenPrivacyPause: (() => void) | undefined;
    void loadPrivacyStatus();
    if (isTauriRuntime()) {
      void getRuntimeInfo().then((runtime) => {
        if (!disposed && runtime && runtime.operatingSystem !== "windows") {
          nonWindowsDesktop = true;
        }
      });
      listen<boolean>("privacy-pause-changed", (event) => {
        privacyPaused = event.payload;
      }).then((unlisten) => {
        if (disposed) unlisten();
        else unlistenPrivacyPause = unlisten;
      });
    }
    return () => {
      disposed = true;
      unlistenPrivacyPause?.();
    };
  });

  function changeLanguage(lang: Locale) {
    generalSettings.updateSetting("language", lang);
    locale.set(lang);
    feedback.show(
      _t(lang === "zh-CN" ? "general.languageSwitchedZh" : "general.languageSwitchedEn"),
      true,
    );
  }

  async function loadPrivacyStatus() {
    if (!isTauriRuntime()) {
      privacyLoading = false;
      return;
    }

    try {
      const status = await invoke<{ paused: boolean }>("get_privacy_status");
      privacyPaused = status.paused;
    } catch (error) {
      console.error("Unable to load privacy status", error);
    } finally {
      privacyLoading = false;
    }
  }

  async function togglePrivacyPause() {
    if (!isTauriRuntime() || privacyLoading) return;
    privacyLoading = true;

    try {
      privacyPaused = await invoke<boolean>("toggle_privacy_pause");
      feedback.show(_t(privacyPaused ? "capture.paused" : "capture.resumed"), true);
    } catch (error) {
      console.error("Unable to toggle privacy pause", error);
      feedback.show(error instanceof Error ? error.message : String(error), false);
    } finally {
      privacyLoading = false;
    }
  }

  const generalEntries: SettingEntryConfig[] = $derived([
    {
      type: "custom",
      variant: "toggle",
      id: "general.language",
      icon: "globe",
      label: _t("general.language"),
      desc: _t("general.languageDescription"),
    },
    {
      type: "custom",
      variant: "toggle",
      id: "recording.pause",
      icon: "pause",
      label: _t("capture.pauseTitle"),
      desc: _t("capture.pauseDescription"),
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
    <SettingEntry {config}>
      {#snippet children()}
        {#if config.type === "custom" && config.id === "general.language"}
          <div class="lang-toggle">
            <button
              type="button"
              class:active={s.language === "zh-CN"}
              onclick={() => changeLanguage("zh-CN")}>中文</button
            >
            <button
              type="button"
              class:active={s.language === "en"}
              onclick={() => changeLanguage("en")}>English</button
            >
          </div>
        {:else if config.type === "custom" && config.id === "recording.pause"}
          <div class="pause-control">
            <span class="pause-state"
              >{_t(privacyPaused ? "capture.paused" : "capture.active")}</span
            >
            <button
              type="button"
              class="toggle-switch"
              class:active={!privacyPaused}
              role="switch"
              aria-checked={!privacyPaused}
              aria-label={_t(privacyPaused ? "capture.resumeAction" : "capture.pauseAction")}
              title={_t(privacyPaused ? "capture.resumeAction" : "capture.pauseAction")}
              disabled={privacyLoading || !isTauriRuntime()}
              onclick={togglePrivacyPause}
            >
              <span class="toggle-knob"></span>
            </button>
          </div>
          {#if nonWindowsDesktop}
            <p class="polling-note">{_t("capture.pollingNote")}</p>
          {/if}
        {/if}
      {/snippet}
    </SettingEntry>
  {/each}
</div>

{#if feedback.message}
  <div class:success={feedback.success} class="settings-feedback">{feedback.message}</div>
{/if}

<style>
  .lang-toggle {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .lang-toggle button {
    padding: 7px 16px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-muted);
    background: var(--input-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .lang-toggle button:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .lang-toggle button.active {
    border-color: var(--selection-color);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }

  .pause-control {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 0 0 auto;
  }

  .pause-state {
    color: var(--text-muted);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  .polling-note {
    margin: 6px 0 0;
    color: var(--text-muted);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    line-height: 1.5;
  }
</style>
