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
  let pauseReady = $state(false);
  let pauseRevision = 0;
  let disposed = false;
  // True on desktop macOS/Linux, where clipboard capture polls instead of
  // using native monitoring and self-trigger marking is unavailable.
  let nonWindowsDesktop = $state(false);
  const feedback = createFeedback(3000);

  onMount(() => {
    void loadPrivacy();
    let unlistenPrivacyPause: (() => void) | undefined;
    if (isTauriRuntime()) {
      void getRuntimeInfo()
        .then((runtime) => {
          if (!disposed && runtime && runtime.operatingSystem !== "windows") {
            nonWindowsDesktop = true;
          }
        })
        .catch((error) => console.error("Unable to load runtime capabilities", error));
      // Subscribe before reading; an event received during the read is newer
      // than its snapshot and must not be overwritten by that response.
      void listen<boolean>("privacy-pause-changed", (event) => {
        if (disposed) return;
        pauseRevision += 1;
        privacyPaused = event.payload;
        pauseReady = true;
      })
        .then((unlisten) => {
          if (disposed) {
            unlisten();
            return;
          }
          unlistenPrivacyPause = unlisten;
          return loadPrivacyStatus();
        })
        .catch((error) => {
          if (disposed) return;
          feedback.show(error instanceof Error ? error.message : String(error), false);
          void loadPrivacyStatus();
        });
    } else {
      void loadPrivacyStatus();
    }
    return () => {
      disposed = true;
      unlistenPrivacyPause?.();
      feedback.dispose();
    };
  });

  async function loadPrivacyStatus() {
    if (disposed) return;
    if (!isTauriRuntime()) {
      pauseReady = true;
      pauseLoading = false;
      return;
    }
    pauseLoading = true;
    const revision = pauseRevision;
    try {
      const status = await invoke<{ paused: boolean }>("get_privacy_status");
      if (disposed) return;
      if (revision === pauseRevision) privacyPaused = status.paused;
      pauseReady = true;
    } catch (error) {
      if (!disposed) console.error("Unable to load privacy status", error);
    } finally {
      if (!disposed) pauseLoading = false;
    }
  }

  function retryConfiguration() {
    void loadPrivacy();
    void loadPrivacyStatus();
  }

  async function togglePrivacyPause() {
    if (!isTauriRuntime() || pauseLoading) return;
    pauseLoading = true;

    const revision = pauseRevision;
    try {
      const paused = await invoke<boolean>("toggle_privacy_pause");
      if (disposed) return;
      if (revision === pauseRevision) privacyPaused = paused;
      feedback.show(_t(privacyPaused ? "capture.paused" : "capture.resumed"), true);
    } catch (error) {
      console.error("Unable to toggle privacy pause", error);
      feedback.show(error instanceof Error ? error.message : String(error), false);
    } finally {
      pauseLoading = false;
    }
  }

  async function loadPrivacy() {
    if (disposed) return;
    loading = true;
    try {
      const loaded = await getPrivacySettings();
      if (disposed) return;
      privacy = loaded;
      patternsText = loaded.sensitivePatterns.join("\n");
    } catch (error) {
      if (disposed) return;
      console.error("Unable to load privacy settings", error);
      feedback.show(error instanceof Error ? error.message : String(error), false);
    } finally {
      if (!disposed) loading = false;
    }
  }

  async function applyLocalOnly(next: boolean): Promise<boolean> {
    if (!privacy) return false;
    try {
      const updated = await setPrivacySettings({ localOnly: next });
      // The response is a snapshot; another setting may have changed meanwhile.
      privacy = { ...privacy!, localOnly: updated.localOnly };
      return true;
    } catch (error) {
      feedback.show(error instanceof Error ? error.message : String(error), false);
      return false;
    }
  }

  async function savePatterns() {
    if (!privacy || patternsSaving) return;
    patternsSaving = true;
    const submittedText = patternsText;
    const lines = submittedText
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    try {
      const updated = await setPrivacySettings({ sensitivePatterns: lines });
      privacy = { ...privacy!, sensitivePatterns: updated.sensitivePatterns };
      if (patternsText === submittedText) {
        patternsText = updated.sensitivePatterns.join("\n");
      }
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
  {#if loading || pauseLoading}
    <div class="settings-state">{_t("storage.readingConfig")}</div>
  {:else if !privacy || !pauseReady}
    <div class="settings-state" role="status">
      <p>{_t("storage.configLoadFailed")}</p>
      <button type="button" class="settings-action-btn" onclick={retryConfiguration}
        >{_t("storage.retryLoad")}</button
      >
    </div>
  {:else}
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
        <p class="settings-platform-note">{_t("capture.pollingNote")}</p>
      {/if}
    </CustomEntry>

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
</style>
