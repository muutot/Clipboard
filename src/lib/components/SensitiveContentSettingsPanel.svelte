<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomEntry from "$lib/components/settings-entries/CustomEntry.svelte";
  import ToggleEntry from "$lib/components/settings-entries/ToggleEntry.svelte";
  import {
    getPrivacySettings,
    setPrivacySettings,
    type PrivacySettings,
  } from "$lib/services/capture";
  import { messages, resolvePath } from "$lib/i18n";
  import { createFeedback } from "$lib/utils/feedback.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getRuntimeInfo, isTauriRuntime } from "$lib/services/runtime";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let privacy = $state<PrivacySettings | null>(null);
  let loading = $state(true);
  let patternsText = $state("");
  let patternsSaving = $state(false);
  let privacyPaused = $state(false);
  let pauseLoading = $state(true);
  // True on desktop macOS/Linux, where clipboard capture polls instead of
  // using native monitoring and self-trigger marking is unavailable.
  let nonWindowsDesktop = $state(false);
  const feedback = createFeedback(3000);

  onMount(() => {
    void loadPrivacy();
    void loadPrivacyStatus();
    let unlistenPrivacyPause: (() => void) | undefined;
    let disposed = false;
    if (isTauriRuntime()) {
      void getRuntimeInfo().then((runtime) => {
        if (runtime && runtime.operatingSystem !== "windows") {
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
      feedback.dispose();
    };
  });

  async function loadPrivacyStatus() {
    if (!isTauriRuntime()) {
      pauseLoading = false;
      return;
    }

    try {
      const status = await invoke<{ paused: boolean }>("get_privacy_status");
      privacyPaused = status.paused;
    } catch (error) {
      console.error("Unable to load privacy status", error);
    } finally {
      pauseLoading = false;
    }
  }

  async function togglePrivacyPause() {
    if (!isTauriRuntime() || pauseLoading) return;
    pauseLoading = true;

    try {
      privacyPaused = await invoke<boolean>("toggle_privacy_pause");
      feedback.show(_t(privacyPaused ? "capture.paused" : "capture.resumed"), true);
    } catch (error) {
      console.error("Unable to toggle privacy pause", error);
      feedback.show(error instanceof Error ? error.message : String(error), false);
    } finally {
      pauseLoading = false;
    }
  }

  async function loadPrivacy() {
    try {
      const loaded = await getPrivacySettings();
      privacy = loaded;
      patternsText = loaded.sensitivePatterns.join("\n");
    } catch (error) {
      console.error("Unable to load privacy settings", error);
      feedback.show(error instanceof Error ? error.message : String(error), false);
    } finally {
      loading = false;
    }
  }

  async function applyLocalOnly(next: boolean): Promise<boolean> {
    if (!privacy) return false;
    try {
      const updated = await setPrivacySettings({ localOnly: next });
      privacy = updated;
      patternsText = updated.sensitivePatterns.join("\n");
      return true;
    } catch (error) {
      feedback.show(error instanceof Error ? error.message : String(error), false);
      return false;
    }
  }

  async function savePatterns() {
    if (!privacy || patternsSaving) return;
    patternsSaving = true;
    const lines = patternsText
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    try {
      const updated = await setPrivacySettings({ sensitivePatterns: lines });
      privacy = updated;
      patternsText = updated.sensitivePatterns.join("\n");
      feedback.show(_t("capture.sensitivePatternsSaved"), true);
    } catch (error) {
      feedback.show(error instanceof Error ? error.message : String(error), false);
    } finally {
      patternsSaving = false;
    }
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("capture.settings")}</span>
      <h2>{_t("capture.sensitiveContentTitle")}</h2>
      <p>{_t("capture.sensitiveSectionDescription")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}>
      <AppIcon name="x" size={14} strokeWidth={2} />
    </button>
  </header>
{/if}

<div class="settings-scroll">
  <CustomEntry
    searchId="recording.pause"
    config={{
      type: "custom",
      variant: "toggle",
      icon: "pause",
      label: _t("capture.pauseTitle"),
      desc: _t("capture.pauseDescription"),
    }}
  >
    <div class="pause-control">
      <span class="pause-state">{_t(privacyPaused ? "capture.paused" : "capture.active")}</span>
      <button
        type="button"
        class="toggle-switch"
        class:active={!privacyPaused}
        role="switch"
        aria-checked={!privacyPaused}
        aria-label={_t(privacyPaused ? "capture.resumeAction" : "capture.pauseAction")}
        title={_t(privacyPaused ? "capture.resumeAction" : "capture.pauseAction")}
        disabled={pauseLoading || !isTauriRuntime()}
        onclick={togglePrivacyPause}
      >
        <span class="toggle-knob"></span>
      </button>
    </div>
    {#if nonWindowsDesktop}
      <p class="polling-note">{_t("capture.pollingNote")}</p>
    {/if}
  </CustomEntry>

  {#if loading || !privacy}
    <div class="settings-state">{_t("storage.readingConfig")}</div>
  {:else}
    <ToggleEntry
      searchId="capture.local-only"
      config={{
        type: "toggle",
        variant: "card",
        icon: "lock",
        label: _t("capture.localOnly"),
        desc: _t("capture.localOnlyDescription"),
        get: () => privacy!.localOnly,
        set: (v) => void applyLocalOnly(v),
      }}
    />

    <CustomEntry
      searchId="capture.sensitive-patterns"
      config={{
        type: "custom",
        variant: "column",
        icon: "filter",
        label: _t("capture.sensitivePatternsLabel"),
        desc: _t("capture.sensitiveContentDescription"),
      }}
    >
      <textarea
        class="entry-textarea"
        bind:value={patternsText}
        placeholder={_t("capture.sensitivePatternsPlaceholder")}
        spellcheck="false"
        rows="6"></textarea>
      <div class="entry-actions">
        <button
          type="button"
          class="settings-action-btn"
          disabled={patternsSaving}
          onclick={savePatterns}>{_t("actions.save")}</button
        >
      </div>
    </CustomEntry>
  {/if}
</div>

{#if feedback.message}
  <div class:success={feedback.success} class="settings-feedback">{feedback.message}</div>
{/if}

<style>
  header p {
    max-width: 570px;
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
