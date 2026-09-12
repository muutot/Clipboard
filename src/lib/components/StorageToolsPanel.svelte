<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import TransferPanel from "$lib/components/TransferPanel.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { rebuildSearchIndex, repairDatabase, type RepairResult } from "$lib/services/storage";
  import { invoke } from "@tauri-apps/api/core";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onfeedback: (message: string, success: boolean) => void;
    onadjustlimit: () => void;
  }

  let { onfeedback, onadjustlimit }: Props = $props();

  let rebuilding = $state(false);
  let repairLoading = $state(false);
  let repairResult = $state<RepairResult | null>(null);
  let maxItemCount = $state(10000);

  onMount(() => {
    void loadMaxItemCount();
  });

  async function loadMaxItemCount() {
    try {
      const result = await invoke<{ maxItems: number }>("get_history_config");
      if (result) maxItemCount = result.maxItems;
    } catch (error) {
      console.error("Unable to load history config", error);
    }
  }

  async function doRepair() {
    repairLoading = true;
    repairResult = null;
    try {
      repairResult = await repairDatabase();
      if (repairResult) {
        onfeedback(
          repairResult.integrityOk
            ? `Database integrity OK (${repairResult.pageCount} pages, ${repairResult.freelistCount} free)`
            : `Database repair needed: ${repairResult.integrityMessage}`,
          repairResult.integrityOk,
        );
      }
    } catch (error) {
      console.error("Database repair failed", error);
      onfeedback(
        "Database repair failed: " + (error instanceof Error ? error.message : String(error)),
        false,
      );
    } finally {
      repairLoading = false;
    }
  }

  async function rebuildIndex() {
    rebuilding = true;
    try {
      const summary = await rebuildSearchIndex();
      onfeedback(
        _t("storage.rebuildComplete", {
          events: summary.processedEvents,
          docs: summary.upsertedDocuments,
        }),
        true,
      );
    } catch (error) {
      console.error("Unable to rebuild search index", error);
      onfeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      rebuilding = false;
    }
  }
</script>

<div class="settings-scroll">
  <TransferPanel {maxItemCount} {onfeedback} {onadjustlimit} />

  <section class="setting-card toggle-card" data-settings-search-id="storage.search-index">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="search" size={17} /></span>
      <div>
        <strong>{_t("storage.searchIndexTitle")}</strong>
        <p>{_t("storage.searchIndexDesc")}</p>
      </div>
    </div>
    <button type="button" class="settings-action-btn" disabled={rebuilding} onclick={rebuildIndex}>
      {rebuilding ? _t("storage.rebuilding") : _t("storage.rebuildIndex")}
    </button>
  </section>

  <section
    class="setting-card setting-card-row"
    data-settings-search-id="storage.database-maintenance"
  >
    <span class="setting-icon"><AppIcon name="settings" size={17} /></span>
    <span class="setting-label">{_t("storage.databaseMaintenance")}</span>
    <button type="button" disabled={repairLoading} onclick={doRepair}>
      {repairLoading ? _t("storage.checkingDatabase") : _t("storage.checkDatabase")}
    </button>
  </section>

  {#if repairResult}
    <div class="repair-result">
      <span class:ok={repairResult.integrityOk} class:fail={!repairResult.integrityOk}>
        {repairResult.integrityOk ? _t("storage.integrityOk") : _t("storage.integrityProblem")}
      </span>
      <code>{repairResult.integrityMessage}</code>
    </div>
  {/if}
</div>

<style>
  .repair-result {
    margin-top: 10px;
    padding: 8px 9px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-control-radius);
    background: var(--input-bg);
    font-size: var(--settings-description-size);
  }

  .repair-result span.ok {
    color: color-mix(in srgb, var(--success-color) 75%, white);
  }

  .repair-result span.fail {
    color: color-mix(in srgb, var(--danger-color) 75%, white);
  }

  .repair-result code {
    display: block;
    margin-top: 4px;
    color: var(--text-secondary);
    font-size: var(--settings-note-size);
  }
</style>
