<script lang="ts">
  import { onDestroy } from "svelte";
  import AppIcon from "#lib/components/card/AppIcon.svelte";
  import TransferPanel from "#lib/components/settings/TransferPanel.svelte";
  import EntryHeading from "#lib/components/settings/settings-entries/EntryHeading.svelte";
  import { messages, resolvePath } from "#lib/i18n/index.ts";
  import { rebuildSearchIndex, repairDatabase, type RepairResult } from "#lib/services/storage.ts";

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
  let disposed = false;

  onDestroy(() => {
    disposed = true;
  });

  async function doRepair() {
    if (disposed || repairLoading) return;
    repairLoading = true;
    repairResult = null;
    try {
      const result = await repairDatabase();
      if (disposed) return;
      repairResult = result;
      if (repairResult) {
        onfeedback(
          repairResult.integrityOk
            ? _t("storage.databaseIntegrityOk", {
                pages: repairResult.pageCount,
                free: repairResult.freelistCount,
              })
            : _t("storage.databaseRepairNeeded", { message: repairResult.integrityMessage }),
          repairResult.integrityOk,
        );
      }
    } catch (error) {
      if (disposed) return;
      console.error("Database repair failed", error);
      onfeedback(
        _t("storage.databaseRepairFailed", {
          message: error instanceof Error ? error.message : String(error),
        }),
        false,
      );
    } finally {
      if (!disposed) repairLoading = false;
    }
  }

  async function rebuildIndex() {
    if (disposed || rebuilding) return;
    rebuilding = true;
    try {
      const summary = await rebuildSearchIndex();
      if (disposed) return;
      onfeedback(
        _t("storage.rebuildComplete", {
          events: summary.processedEvents,
          docs: summary.upsertedDocuments,
        }),
        true,
      );
    } catch (error) {
      if (disposed) return;
      console.error("Unable to rebuild search index", error);
      onfeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      if (!disposed) rebuilding = false;
    }
  }
</script>

<div class="settings-scroll">
  <TransferPanel {onfeedback} {onadjustlimit} />

  <section class="setting-card toggle-card" data-settings-search-id="storage.search-index">
    <EntryHeading
      icon="search"
      label={_t("storage.searchIndexTitle")}
      desc={_t("storage.searchIndexDesc")}
    />
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
