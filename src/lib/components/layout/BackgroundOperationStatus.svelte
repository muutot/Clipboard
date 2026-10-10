<script lang="ts">
  import { messages, resolvePath } from "$lib/i18n";
  import type { createOperationMonitor } from "$lib/services/background-operations.svelte";
  import { formatBytes } from "$lib/utils/content/format";
  let { operation }: { operation: ReturnType<typeof createOperationMonitor> } = $props();
  const t = (key: string) => resolvePath($messages, key);
</script>

{#if operation.active && operation.snapshot}
  <div class="setting-actions-row">
    <p class="settings-state" role="status">
      {operation.snapshot.status === "cancelling"
        ? t("operations.cancelling")
        : t("operations." + operation.snapshot.phase)}
      {#if operation.snapshot.phase === "transferring"}
        · {formatBytes(operation.snapshot.completed)}
      {:else if operation.snapshot.total !== null}
        · {operation.snapshot.completed} / {operation.snapshot.total}
      {/if}
    </p>
    <button
      class="settings-action-btn"
      disabled={operation.cancelling || operation.snapshot.status === "cancelling"}
      onclick={() => operation.cancel()}>{t("operations.cancel")}</button
    >
  </div>
{:else if operation.snapshot?.status === "cancelled"}
  <p class="settings-state" role="status">{t("operations.cancelled")}</p>
{/if}
{#if operation.error}
  <p class="settings-state" role="alert">{t("operations.statusFailed")}: {operation.error}</p>
{/if}
