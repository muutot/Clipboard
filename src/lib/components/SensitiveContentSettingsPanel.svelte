<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import {
    getPrivacySettings,
    setPrivacySettings,
    type PrivacySettings,
  } from "$lib/services/capture";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let privacy = $state<PrivacySettings | null>(null);
  let loading = $state(true);
  let localOnlySaving = $state(false);
  let patternsText = $state("");
  let patternsSaving = $state(false);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let feedbackTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    void loadPrivacy();
    return () => {
      if (feedbackTimer !== undefined) clearTimeout(feedbackTimer);
    };
  });

  function showFeedback(message: string, success: boolean) {
    feedback = message;
    feedbackSuccess = success;
    if (feedbackTimer !== undefined) clearTimeout(feedbackTimer);
    feedbackTimer = setTimeout(() => {
      feedbackTimer = undefined;
      feedback = "";
    }, 3000);
  }

  async function loadPrivacy() {
    try {
      const loaded = await getPrivacySettings();
      privacy = loaded;
      patternsText = loaded.sensitivePatterns.join("\n");
    } catch (error) {
      console.error("Unable to load privacy settings", error);
      showFeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      loading = false;
    }
  }

  async function toggleLocalOnly() {
    if (!privacy || localOnlySaving) return;
    localOnlySaving = true;
    const next = !privacy.localOnly;
    try {
      const updated = await setPrivacySettings({ localOnly: next });
      privacy = updated;
      patternsText = updated.sensitivePatterns.join("\n");
    } catch (error) {
      showFeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      localOnlySaving = false;
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
      showFeedback(_t("capture.sensitivePatternsSaved"), true);
    } catch (error) {
      showFeedback(error instanceof Error ? error.message : String(error), false);
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
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card toggle-card" data-settings-search-id="capture.local-only">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="lock" size={17} /></span>
      <div>
        <strong>{_t("capture.localOnly")}</strong>
        <p>{_t("capture.localOnlyDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={privacy?.localOnly ?? false}
      role="switch"
      aria-checked={privacy?.localOnly ?? false}
      aria-label={_t("capture.localOnly")}
      disabled={!privacy || localOnlySaving || loading}
      onclick={toggleLocalOnly}
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  <section class="setting-card" data-settings-search-id="capture.sensitive-patterns">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="filter" size={17} /></span>
      <div>
        <strong>{_t("capture.sensitivePatternsLabel")}</strong>
        <p>{_t("capture.sensitiveContentDescription")}</p>
      </div>
    </div>
    <textarea
      class="patterns-textarea"
      bind:value={patternsText}
      placeholder={_t("capture.sensitivePatternsPlaceholder")}
      spellcheck="false"
      rows="6"></textarea>
    <div class="patterns-actions">
      <button
        type="button"
        class="settings-action-btn"
        disabled={!privacy || patternsSaving || loading}
        onclick={savePatterns}>{_t("actions.save")}</button
      >
    </div>
  </section>
</div>

{#if feedback && !loading}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  header p {
    max-width: 570px;
  }

  .patterns-textarea {
    width: 100%;
    resize: vertical;
    padding: 8px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--input-bg);
    color: var(--text-primary);
    font-family: var(--font-mono, ui-monospace, "SFMono-Regular", "Menlo", monospace);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    line-height: 1.5;
    outline: none;
    transition: border-color 120ms ease;
  }
  .patterns-textarea::placeholder {
    color: var(--placeholder-color);
  }
  .patterns-textarea:focus {
    border-color: var(--text-faint);
  }
  .patterns-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }
</style>
