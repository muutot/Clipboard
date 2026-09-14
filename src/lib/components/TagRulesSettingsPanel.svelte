<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { getAutoTagRules, setAutoTagRules, type AutoTagRule } from "$lib/services/clipboard";
  import { createFeedback } from "$lib/utils/feedback.svelte";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let rules = $state<AutoTagRule[]>([]);
  let rulesSaving = $state(false);
  let feedback = createFeedback(3000);

  onDestroy(() => feedback.dispose());

  onMount(() => {
    void loadRules();
  });

  async function loadRules() {
    rules = (await getAutoTagRules()) ?? [];
  }

  function addRule() {
    rules = [...rules, { pattern: "", tag: "" }];
  }

  function removeRule(index: number) {
    rules = rules.filter((_, i) => i !== index);
  }

  async function saveRules() {
    if (rulesSaving) return;
    rulesSaving = true;
    try {
      const saved = await setAutoTagRules(
        rules.map((rule) => ({ pattern: rule.pattern.trim(), tag: rule.tag.trim() })),
      );
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
    {#each rules as rule, index (index)}
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
    {/each}
    <div class="autotag-actions">
      <button type="button" class="autotag-add" onclick={addRule}>
        {_t("tags.autoTagAdd")}
      </button>
      <button
        type="button"
        class="autotag-save"
        disabled={rulesSaving || !isTauriRuntime()}
        onclick={() => void saveRules()}
      >
        {_t("tags.autoTagSave")}
      </button>
    </div>
  </section>

  {#if feedback.message}
    <p class="settings-feedback" class:success={feedback.success} role="status">
      {feedback.message}
    </p>
  {/if}
</div>

<style>
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
