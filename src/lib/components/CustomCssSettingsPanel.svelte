<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { get } from "svelte/store";
  import { generalSettings } from "$lib/services/settings";
  import { messages, resolvePath } from "$lib/i18n";
  import { compileCustomCss, MAX_CUSTOM_CSS_LENGTH } from "$lib/utils/custom-css";
  import { createFeedback } from "$lib/utils/feedback.svelte";

  let { onclose, showHeader = true }: { onclose: () => void; showHeader?: boolean } = $props();
  const t = (key: string) => resolvePath($messages, "customCss." + key);
  let settings = $state($generalSettings);
  let draft = $state(get(generalSettings).customCss);
  let loading = $state(true);
  let busy = $state(false);
  const dirty = $derived(draft !== settings.customCss);
  const feedback = createFeedback(4000);
  const example =
    ":root {\n  --accent: #7c6cf2;\n  --selection-color: #7c6cf2;\n}\n\n/* Clipboard history cards */\n.clip-card {\n  border-radius: 10px;\n}\n";
  onDestroy(() => feedback.dispose());

  onMount(() => {
    let disposed = false;
    const unsubscribe = generalSettings.subscribe((next) => {
      // Preserve an unsaved draft when another window edits settings.
      if (draft === settings.customCss) draft = next.customCss;
      settings = next;
    });
    void generalSettings.initialize().then(() => {
      if (!disposed) loading = false;
    });
    return () => {
      disposed = true;
      unsubscribe();
    };
  });

  async function save() {
    if (busy) return;
    try {
      if (draft.length > MAX_CUSTOM_CSS_LENGTH) throw new Error(t("limit"));
      const compiled = compileCustomCss(draft);
      if (!compiled && draft.replace(/\/\*[\s\S]*?\*\//g, "").trim()) {
        throw new Error(t("invalid"));
      }
      busy = true;
      generalSettings.merge({ customCss: draft, customCssEnabled: true });
      await generalSettings.flush();
      feedback.show(t("saved"));
    } catch (error) {
      feedback.show(t("failed") + ": " + String(error), false);
    } finally {
      busy = false;
    }
  }

  async function toggle() {
    if (busy) return;
    busy = true;
    try {
      generalSettings.updateSetting("customCssEnabled", !settings.customCssEnabled);
      await generalSettings.flush();
      feedback.show(settings.customCssEnabled ? t("saved") : t("disabled"));
    } catch (error) {
      feedback.show(t("failed") + ": " + String(error), false);
    } finally {
      busy = false;
    }
  }
</script>

{#if showHeader}
  <header>
    <div>
      <h2>{t("title")}</h2>
      <p>{t("description")}</p>
    </div>
    <button
      class="close-button"
      aria-label={resolvePath($messages, "actions.close")}
      onclick={onclose}>×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  {#if loading}
    <p class="settings-state">{resolvePath($messages, "storage.readingConfig")}</p>
  {:else}
    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <div>
          <strong>{t("enabled")}</strong>
          <p>{t("enabledDescription")}</p>
        </div>
      </div>
      <button
        class="toggle-switch"
        class:active={settings.customCssEnabled}
        role="switch"
        aria-checked={settings.customCssEnabled}
        aria-label={t("enabled")}
        disabled={busy}
        onclick={toggle}><span class="toggle-knob"></span></button
      >
    </section>
    <section class="setting-card css-card" data-settings-search-id="appearance.custom-css">
      <div class="setting-heading">
        <div>
          <strong>{t("editor")}</strong>
          <p>{t("editorHint")}</p>
        </div>
      </div>
      <textarea
        class="settings-text-input css-editor"
        bind:value={draft}
        aria-label={t("editor")}
        maxlength={MAX_CUSTOM_CSS_LENGTH}
        spellcheck={false}
        autocapitalize="off"
        autocomplete="off"
        wrap="off"
        disabled={busy}></textarea>
      <div class="setting-actions-row">
        <button class="settings-action-btn" disabled={busy} onclick={save}>{t("save")}</button>
        <button
          class="settings-action-btn"
          disabled={busy || !dirty}
          onclick={() => (draft = settings.customCss)}>{t("resetDraft")}</button
        >
        <button
          class="settings-action-btn"
          disabled={busy || !!draft.trim()}
          onclick={() => (draft = example)}>{t("example")}</button
        >
        <button class="settings-action-btn" disabled={busy || !draft} onclick={() => (draft = "")}
          >{t("clear")}</button
        >
      </div>
      <p class="css-note">
        {draft.length.toLocaleString()} / {MAX_CUSTOM_CSS_LENGTH.toLocaleString()}{dirty
          ? " · " + t("unsaved")
          : ""}
      </p>
      <p class="css-note">{t("recovery")}</p>
    </section>
  {/if}
  {#if feedback.message}
    <p class="settings-feedback" class:success={feedback.success} role="status">
      {feedback.message}
    </p>
  {/if}
</div>

<style>
  .css-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .css-editor {
    width: 100%;
    min-height: 260px;
    resize: vertical;
    font-family: ui-monospace, Consolas, monospace;
    font-size: var(--settings-control-size, var(--font-size-secondary));
    line-height: 1.6;
    tab-size: 2;
  }
  .css-note {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--settings-note-size, var(--font-size-tiny));
  }
</style>
