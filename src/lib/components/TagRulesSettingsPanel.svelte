<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { getAutoTagRules, setAutoTagRules, type AutoTagRule } from "$lib/services/clipboard";
  import { createFeedback } from "$lib/utils/feedback.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import {
    previewAutoTagRules,
    previewAutoTagHistory,
    applyAutoTagHistory,
    type AutoTagHistoryPreview,
  } from "$lib/services/clipboard";
  import type { ClipboardKind } from "$lib/types/clipboard";
  import {
    createOperationMonitor,
    isOperationCancelled,
  } from "$lib/services/background-operations.svelte";
  import BackgroundOperationStatus from "./BackgroundOperationStatus.svelte";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let rules = $state<AutoTagRule[]>([]);
  let loading = $state(true);
  let rulesSaving = $state(false);
  const operation = createOperationMonitor("tags");
  const busy = $derived(rulesSaving || operation.active);
  let feedback = createFeedback(3000);
  let sampleText = $state("");
  let sampleSource = $state("");
  let sampleKind = $state<ClipboardKind>("text");
  let sampleTags = $state<string[] | null>(null);
  let preview = $state<AutoTagHistoryPreview | null>(null);
  const kindOptions = $derived(
    ["text", "link", "image", "file"].map((value) => ({ value, label: _t("filter." + value) })),
  );
  const rulesKey = $derived(JSON.stringify(rules));
  $effect(() => {
    void sampleText;
    void sampleSource;
    void sampleKind;
    sampleTags = null;
  });
  $effect(() => {
    void rulesKey;
    sampleTags = null;
    preview = null;
  });
  const snapshotRules = () =>
    rules.map((rule) => ({
      pattern: rule.pattern.trim(),
      tag: rule.tag.trim(),
      sourceApp: rule.sourceApp?.trim() ?? "",
      kind: rule.kind ?? null,
    }));

  async function runPreview(mode: "sample" | "history" | "apply") {
    if (busy) return;
    rulesSaving = true;
    try {
      const snapshot = snapshotRules();
      if (mode === "sample")
        sampleTags = await previewAutoTagRules(snapshot, sampleText, sampleSource, sampleKind);
      else if (mode === "history") preview = await previewAutoTagHistory(snapshot);
      else {
        const result = await applyAutoTagHistory(snapshot);
        preview = null;
        feedback.show(_t("autoTags.applied", { count: result.changedCount }));
      }
    } catch (error) {
      feedback.show(
        isOperationCancelled(error)
          ? _t("operations.cancelled")
          : error instanceof Error
            ? error.message
            : String(error),
        isOperationCancelled(error),
      );
    } finally {
      rulesSaving = false;
      await operation.refresh();
    }
  }

  onDestroy(() => feedback.dispose());

  onMount(() => {
    void loadRules();
  });

  async function loadRules() {
    try {
      rules = (await getAutoTagRules()) ?? [];
    } catch (error) {
      console.error("Unable to load auto-tag rules", error);
    } finally {
      loading = false;
    }
  }

  function addRule() {
    rules = [...rules, { pattern: "", tag: "" }];
  }

  function removeRule(index: number) {
    rules = rules.filter((_, i) => i !== index);
  }

  async function saveRules() {
    if (busy) return;
    rulesSaving = true;
    try {
      const saved = await setAutoTagRules(snapshotRules());
      rules = saved ?? rules;
      feedback.show(_t("tags.autoTagSaved"));
    } catch (error) {
      feedback.show(error instanceof Error ? error.message : String(error), false);
    } finally {
      rulesSaving = false;
    }
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("storage.tagsTab")}</span>
      <h2>{_t("tags.autoTagTitle")}</h2>
      <p>{_t("tags.autoTagDescription")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card">
    <div class="setting-heading">
      <div>
        <strong>{_t("tags.autoTagTitle")}</strong>
        <p>{_t("tags.autoTagDescription")}</p>
      </div>
    </div>
    {#if loading}
      <p class="settings-state">{_t("storage.readingConfig")}</p>
    {:else}
      {#each rules as rule, index (index)}
        <fieldset disabled={busy}>
          <div class="autotag-row">
            <input
              class="autotag-pattern"
              type="text"
              bind:value={rule.pattern}
              placeholder={_t("tags.autoTagPatternPlaceholder")}
              aria-label={_t("tags.autoTagPatternPlaceholder")}
              spellcheck={false}
            />
            <input
              class="autotag-tag"
              type="text"
              bind:value={rule.tag}
              placeholder={_t("tags.autoTagTagPlaceholder")}
              aria-label={_t("tags.autoTagTagPlaceholder")}
              spellcheck={false}
            />
            <button
              type="button"
              class="autotag-remove"
              aria-label={_t("tags.autoTagDelete")}
              title={_t("tags.autoTagDelete")}
              onclick={() => removeRule(index)}>×</button
            >
          </div>
          <div class="autotag-row">
            <input
              class="settings-text-input autotag-source"
              bind:value={rule.sourceApp}
              maxlength={256}
              placeholder={_t("autoTags.source")}
              aria-label={_t("autoTags.source")}
            />
            <CustomSelect
              value={rule.kind ?? ""}
              options={[{ value: "", label: _t("autoTags.allTypes") }, ...kindOptions]}
              ariaLabel={_t("autoTags.kind")}
              disabled={busy}
              onchange={(value) => (rule.kind = (value || null) as ClipboardKind | null)}
            />
          </div>
        </fieldset>
      {/each}
      <div class="autotag-actions">
        <button type="button" class="autotag-add" disabled={busy} onclick={addRule}>
          {_t("tags.autoTagAdd")}
        </button>
        <button
          type="button"
          class="autotag-save"
          disabled={busy || !isTauriRuntime()}
          onclick={() => void saveRules()}
        >
          {_t("tags.autoTagSave")}
        </button>
      </div>
    {/if}
  </section>

  {#if !loading}
    <section class="setting-card" data-settings-search-id="tags.auto-tag-preview">
      <div class="setting-heading">
        <div>
          <strong>{_t("autoTags.previewTitle")}</strong>
          <p>{_t("autoTags.conditionsHint")}</p>
        </div>
      </div>
      <textarea
        disabled={busy}
        class="settings-text-input sample-text"
        maxlength={10000}
        bind:value={sampleText}
        aria-label={_t("autoTags.sample")}
        placeholder={_t("autoTags.sample")}></textarea>
      <div class="autotag-row">
        <input
          class="settings-text-input autotag-source"
          bind:value={sampleSource}
          disabled={busy}
          maxlength={256}
          aria-label={_t("autoTags.sampleSource")}
          placeholder={_t("autoTags.sampleSource")}
        />
        <CustomSelect
          value={sampleKind}
          disabled={busy}
          options={kindOptions}
          ariaLabel={_t("autoTags.kind")}
          onchange={(value) => (sampleKind = value as ClipboardKind)}
        />
      </div>
      <div class="autotag-actions">
        <button
          class="settings-action-btn"
          disabled={busy || !isTauriRuntime()}
          onclick={() => runPreview("sample")}>{_t("autoTags.test")}</button
        >
      </div>
      {#if sampleTags !== null}<p class="preview-result" role="status">
          {sampleTags.join(", ") || _t("autoTags.noMatch")}
        </p>{/if}
    </section>
    <section class="setting-card" data-settings-search-id="tags.auto-tag-history">
      <div class="setting-heading">
        <div>
          <strong>{_t("autoTags.historyTitle")}</strong>
          <p>{_t("autoTags.historyHint")}</p>
        </div>
      </div>
      <div class="autotag-actions">
        <button
          class="settings-action-btn"
          disabled={busy || !isTauriRuntime()}
          onclick={() => runPreview("history")}>{_t("autoTags.previewHistory")}</button
        >
        <button
          class="settings-action-btn"
          disabled={busy || !preview?.changedCount || !isTauriRuntime()}
          onclick={() => runPreview("apply")}>{_t("autoTags.applyHistory")}</button
        >
      </div>
      <BackgroundOperationStatus {operation} />
      {#if preview}
        <p class="preview-result">
          {_t("autoTags.counts", { matched: preview.matchedCount, changed: preview.changedCount })}
        </p>
        <ul>
          {#each preview.samples as sample}<li>
              <span>{sample.title}</span> → {sample.tags.join(", ")}
            </li>{/each}
        </ul>
      {/if}
    </section>
  {/if}

  {#if feedback.message}
    <p class="settings-feedback" class:success={feedback.success} role="status">
      {feedback.message}
    </p>
  {/if}
</div>

<style>
  fieldset {
    min-width: 0;
    border: 0;
    padding: 0 0 8px;
    margin: 0;
  }
  .autotag-source {
    flex: 1;
    min-width: 0;
  }
  .sample-text {
    width: 100%;
    min-height: 70px;
    resize: vertical;
  }
  .preview-result,
  li {
    font-size: var(--settings-description-size);
    color: var(--text-secondary);
    overflow-wrap: anywhere;
  }
  ul {
    margin: 8px 0 0;
    padding-left: 18px;
  }
  .autotag-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }

  .autotag-pattern {
    flex: 3;
    min-width: 0;
  }

  .autotag-tag {
    flex: 2;
    min-width: 0;
  }

  .autotag-pattern,
  .autotag-tag {
    padding: 5px 8px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    background: var(--input-bg, var(--surface-bg));
    font-size: 12px;
  }

  .autotag-pattern:focus,
  .autotag-tag:focus {
    outline: none;
    border-color: var(--text-faint);
  }

  .autotag-remove {
    flex-shrink: 0;
    padding: 2px 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
    font-size: 14px;
    line-height: 1.4;
  }

  .autotag-remove:hover {
    color: var(--danger-color);
    border-color: var(--border-color);
  }

  .autotag-actions {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }

  .autotag-add,
  .autotag-save {
    padding: 5px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    background: var(--card-bg);
    cursor: pointer;
    font-size: 11.5px;
    font-weight: 500;
  }

  .autotag-add:hover,
  .autotag-save:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .autotag-save:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
