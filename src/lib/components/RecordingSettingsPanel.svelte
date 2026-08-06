<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { invoke } from "@tauri-apps/api/core";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();
  let privacyPaused = $state(false);
  let privacyLoading = $state(true);
  let feedback = $state("");
  let feedbackSuccess = $state(false);

  onMount(() => {
    void loadPrivacyStatus();
  });

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
    feedback = "";
    feedbackSuccess = false;

    try {
      privacyPaused = await invoke<boolean>("toggle_privacy_pause");
      feedback = _t(privacyPaused ? "capture.paused" : "capture.resumed");
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to toggle privacy pause", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      privacyLoading = false;
    }
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("capture.pauseTitle")}</span>
      <h2>{_t("capture.pauseTitle")}</h2>
      <p>{_t("capture.pauseDescription")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card toggle-card" data-settings-search-id="recording.pause">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="pause" size={17} /></span>
      <div>
        <strong>{_t("capture.pauseTitle")}</strong>
        <p>{_t("capture.pauseDescription")}</p>
      </div>
    </div>
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
        disabled={privacyLoading || !isTauriRuntime()}
        onclick={togglePrivacyPause}
      >
        <span class="toggle-knob"></span>
      </button>
    </div>
  </section>
</div>

{#if feedback}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 20px 22px 15px;
    border-bottom: 1px solid var(--border-subtle);
  }
  .eyebrow {
    color: var(--text-muted);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  h2 {
    margin: 5px 0 4px;
    color: var(--text-primary);
    font-size: var(--settings-page-title-size, 18px);
    font-weight: 590;
  }
  header p {
    max-width: 570px;
    margin: 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
    line-height: 1.5;
  }
  .close-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--settings-close-size, 28px);
    height: var(--settings-close-size, 28px);
    flex: 0 0 auto;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-close-radius, 7px);
    color: var(--text-muted);
    background: var(--card-bg);
    font-size: var(--settings-close-font-size, 19px);
    line-height: 1;
  }
  .settings-scroll {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex: 1;
    min-height: 0;
    padding: 14px 18px 48px;
    overflow: auto;
  }

  .setting-card {
    padding: 10px 13px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius, 9px);
    background: var(--card-bg);
  }

  .toggle-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .setting-heading {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1;
  }

  .setting-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 29px;
    height: 29px;
    flex: 0 0 auto;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-icon-radius, 7px);
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .setting-heading strong {
    display: block;
    color: var(--text-primary);
    font-size: var(--settings-heading-size, 13px);
    font-weight: 560;
  }

  .setting-heading p {
    margin: 2px 0 0;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
    line-height: 1.45;
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

  .toggle-switch {
    position: relative;
    width: 40px;
    height: 22px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 12px;
    background: var(--input-bg);
    transition: background 120ms ease;
  }

  .toggle-switch.active {
    border-color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 18%, transparent);
  }

  .toggle-switch:disabled {
    opacity: 0.5;
  }

  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text-faint);
    transition: transform 120ms ease;
  }

  .toggle-switch.active .toggle-knob {
    transform: translateX(18px);
    background: var(--selection-color);
  }

  .settings-feedback {
    position: absolute;
    right: 18px;
    bottom: 13px;
    left: 18px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--danger-color) 35%, transparent);
    border-radius: var(--settings-feedback-radius, 7px);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, var(--surface-bg));
    font-size: var(--settings-feedback-size, var(--font-size-secondary, 11px));
  }
  .settings-feedback.success {
    border-color: color-mix(in srgb, var(--success-color) 35%, transparent);
    color: color-mix(in srgb, var(--success-color) 75%, white);
    background: color-mix(in srgb, var(--success-color) 12%, var(--surface-bg));
  }
</style>
