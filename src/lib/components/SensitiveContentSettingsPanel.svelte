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

  async function applyLocalOnly(next: boolean): Promise<boolean> {
    if (!privacy) return false;
    try {
      const updated = await setPrivacySettings({ localOnly: next });
      privacy = updated;
      patternsText = updated.sensitivePatterns.join("\n");
      return true;
    } catch (error) {
      showFeedback(error instanceof Error ? error.message : String(error), false);
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
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}>
      <AppIcon name="x" size={14} strokeWidth={2} />
    </button>
  </header>
{/if}

<div class="settings-scroll">
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

{#if feedback && !loading}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  header p {
    max-width: 570px;
  }
</style>
