<script lang="ts">
  import { onDestroy } from "svelte";
  import { messages, resolvePath } from "#lib/i18n/index.ts";
  import { isTauriRuntime } from "#lib/services/runtime.ts";
  import {
    createResourceBackup,
    previewResourceBackup,
    restoreResourceBackup,
    type BackupPreview,
  } from "#lib/services/storage.ts";
  import { formatBytes } from "#lib/utils/content/format.ts";
  import {
    createOperationMonitor,
    isOperationCancelled,
  } from "#lib/services/background-operations.svelte.ts";
  import BackgroundOperationStatus from "../layout/BackgroundOperationStatus.svelte";
  import EntryHeading from "./settings-entries/EntryHeading.svelte";
  let {
    onfeedback,
    onadjustlimit,
  }: { onfeedback: (message: string, success: boolean) => void; onadjustlimit: () => void } =
    $props();
  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);
  let busy = $state(false);
  const operation = createOperationMonitor("backup");
  const unavailable = $derived(busy || operation.active);
  let preview = $state<BackupPreview | null>(null);
  let selectedPath = $state("");
  let truncated = $state(0);
  let disposed = false;

  onDestroy(() => {
    disposed = true;
  });

  async function run(action: "create" | "preview" | "restore") {
    if (disposed || unavailable || !isTauriRuntime()) return;
    busy = true;
    try {
      const { open, save } = await import("@tauri-apps/plugin-dialog");
      if (disposed) return;
      const filters = [{ name: "Clipboard backup", extensions: ["clipbackup"] }];
      if (action === "create") {
        const path = await save({ defaultPath: "clipboard.clipbackup", filters });
        if (disposed || !path) return;
        const result = await createResourceBackup(path);
        if (disposed) return;
        onfeedback(
          _t("storage.exportSuccess", { path, size: formatBytes(result.byteCount) }),
          true,
        );
      } else if (action === "preview") {
        const path = await open({ multiple: false, directory: false, filters });
        if (typeof path === "string") {
          const nextPreview = await previewResourceBackup(path);
          if (disposed) return;
          preview = nextPreview;
          selectedPath = path;
        }
      } else if (preview) {
        const result = await restoreResourceBackup(selectedPath, preview.fingerprint);
        if (disposed) return;
        preview = null;
        truncated = result.pendingTruncation;
        onfeedback(
          result.errors.length
            ? result.errors.join("; ")
            : _t("storage.importSuccess", {
                imported: result.importedCount,
                skipped: result.skippedCount,
              }),
          result.errors.length === 0,
        );
      }
    } catch (error) {
      if (disposed) return;
      onfeedback(
        isOperationCancelled(error)
          ? _t("operations.cancelled")
          : error instanceof Error
            ? error.message
            : String(error),
        isOperationCancelled(error),
      );
    } finally {
      if (!disposed) busy = false;
      await operation.refresh();
    }
  }
</script>

<section
  class="setting-card"
  data-settings-search-id="storage.resource-backup"
  aria-busy={unavailable}
>
  <EntryHeading label={_t("backup.title")} desc={_t("backup.description")} />
  <div class="setting-actions-row">
    <button
      class="settings-action-btn"
      disabled={unavailable || !isTauriRuntime()}
      onclick={() => run("create")}>{_t("backup.create")}</button
    >
    <button
      class="settings-action-btn"
      disabled={unavailable || !isTauriRuntime()}
      onclick={() => run("preview")}>{_t("backup.preview")}</button
    >
  </div>
  <BackgroundOperationStatus {operation} />
  {#if busy && !operation.active}<p class="settings-state" role="status">
      {_t("backup.working")}
    </p>{/if}
  {#if preview}
    <p class="settings-state">{selectedPath}</p>
    <p class="settings-state">
      {_t("backup.summary", {
        items: preview.itemCount,
        duplicates: preview.duplicateCount,
        resources: preview.resourceCount,
        size: formatBytes(preview.resourceBytes),
      })}
    </p>
    <button class="settings-action-btn" disabled={unavailable} onclick={() => run("restore")}
      >{_t("backup.restore")}</button
    >
  {/if}
  {#if truncated > 0}<p class="settings-state">{_t("backup.retention", { count: truncated })}</p>
    <button class="settings-action-btn" onclick={onadjustlimit}
      >{_t("storage.importAdjustLimit")}</button
    >{/if}
</section>
