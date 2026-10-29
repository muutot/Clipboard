<script lang="ts">
  import { onMount } from "svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import {
    getSourceMappingRules,
    setSourceMappingRules,
    type SourceMappingRule,
  } from "$lib/services/clipboard";
  import { refreshSourceMappingRules } from "$lib/services/source-mapping.svelte";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
    onfeedback: (message: string, success: boolean) => void;
  }

  let { onclose, showHeader = true, onfeedback }: Props = $props();

  let rules = $state<SourceMappingRule[]>([]);
  let loading = $state(true);
  let rulesSaving = $state(false);
  const busy = $derived(rulesSaving);

  onMount(() => {
    void loadRules();
  });

  async function loadRules() {
    try {
      rules = (await getSourceMappingRules()) ?? [];
    } catch (error) {
      console.error("Unable to load source mapping rules", error);
    } finally {
      loading = false;
    }
  }

  function addRule() {
    rules = [...rules, { domain: "", titleKeyword: "", name: "" }];
  }

  function removeRule(index: number) {
    rules = rules.filter((_, i) => i !== index);
  }

  async function saveRules() {
    if (busy) return;
    rulesSaving = true;
    try {
      const snapshot = rules.map((rule) => ({
        domain: rule.domain.trim(),
        titleKeyword: rule.titleKeyword.trim(),
        name: rule.name.trim(),
      }));
      const saved = await setSourceMappingRules(snapshot);
      rules = saved ?? rules;
      // Another window may have saved newer rules while this response was in
      // flight. Refresh from the backend rather than publishing this snapshot.
      await refreshSourceMappingRules();
      onfeedback(_t("sourceMapping.saved"), true);
    } catch (error) {
      onfeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      rulesSaving = false;
    }
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("capture.title")}</span>
      <h2>{_t("sourceMapping.title")}</h2>
      <p>{_t("sourceMapping.description")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card" data-settings-search-id="capture.sourceMapping">
    <div class="setting-heading">
      <div>
        <strong>{_t("sourceMapping.title")}</strong>
        <p>{_t("sourceMapping.description")}</p>
      </div>
    </div>
    {#if loading}
      <p class="settings-state">{_t("storage.readingConfig")}</p>
    {:else}
      {#each rules as rule, index (index)}
        <fieldset disabled={busy}>
          <div class="mapping-row">
            <input
              class="mapping-domain"
              type="text"
              bind:value={rule.domain}
              maxlength={253}
              placeholder={_t("sourceMapping.domainPlaceholder")}
              aria-label={_t("sourceMapping.domainPlaceholder")}
              spellcheck={false}
            />
            <button
              type="button"
              class="mapping-remove"
              aria-label={_t("sourceMapping.deleteRule")}
              title={_t("sourceMapping.deleteRule")}
              onclick={() => removeRule(index)}>×</button
            >
          </div>
          <div class="mapping-row">
            <input
              class="settings-text-input mapping-keyword"
              type="text"
              bind:value={rule.titleKeyword}
              maxlength={100}
              placeholder={_t("sourceMapping.keywordPlaceholder")}
              aria-label={_t("sourceMapping.keywordPlaceholder")}
              spellcheck={false}
            />
            <input
              class="settings-text-input mapping-name"
              type="text"
              bind:value={rule.name}
              maxlength={64}
              placeholder={_t("sourceMapping.namePlaceholder")}
              aria-label={_t("sourceMapping.namePlaceholder")}
              spellcheck={false}
            />
          </div>
        </fieldset>
      {/each}
      <div class="mapping-actions">
        <button type="button" class="mapping-add" disabled={busy} onclick={addRule}>
          {_t("sourceMapping.addRule")}
        </button>
        <button
          type="button"
          class="mapping-save"
          disabled={busy || !isTauriRuntime()}
          onclick={() => void saveRules()}
        >
          {_t("sourceMapping.saveRules")}
        </button>
      </div>
    {/if}
  </section>
</div>

<style>
  fieldset {
    min-width: 0;
    border: 0;
    padding: 0 0 8px;
    margin: 0;
  }
  .mapping-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }
  .mapping-domain,
  .mapping-keyword,
  .mapping-name {
    min-width: 0;
    padding: 5px 8px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    background: var(--input-bg, var(--surface-bg));
    font-size: 12px;
  }
  .mapping-domain {
    flex: 3;
  }
  .mapping-keyword {
    flex: 3;
  }
  .mapping-name {
    flex: 2;
  }
  .mapping-domain:focus,
  .mapping-keyword:focus,
  .mapping-name:focus {
    outline: none;
    border-color: var(--text-faint);
  }
  .mapping-remove {
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
  .mapping-remove:hover {
    color: var(--danger-color);
    border-color: var(--border-color);
  }
  .mapping-actions {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .mapping-add,
  .mapping-save {
    padding: 5px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    background: var(--card-bg);
    cursor: pointer;
    font-size: 11.5px;
    font-weight: 500;
  }
  .mapping-add:hover,
  .mapping-save:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }
  .mapping-save:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
