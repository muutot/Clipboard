<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import {
    getStorageKindStats,
    permanentlyDeleteStorageKind,
    type StorageKind,
    type StorageKindStats,
  } from "$lib/services/storage";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { formatBytes } from "$lib/utils/format";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onfeedback: (message: string, success: boolean) => void;
  }

  let { onfeedback }: Props = $props();

  const storageKinds: readonly {
    kind: StorageKind;
    labelKey: "filter.text" | "filter.link" | "filter.image" | "filter.file";
    icon: "text" | "link" | "image" | "file";
  }[] = [
    { kind: "text", labelKey: "filter.text", icon: "text" },
    { kind: "link", labelKey: "filter.link", icon: "link" },
    { kind: "image", labelKey: "filter.image", icon: "image" },
    { kind: "file", labelKey: "filter.file", icon: "file" },
  ];

  let retentionPeriodDays = $state(90);
  let maxItemCount = $state(10000);
  let recycleBinDays = $state(30);
  let storageKindStats = $state<Record<StorageKind, StorageKindStats>>({
    text: { itemCount: 0, sizeBytes: 0 },
    link: { itemCount: 0, sizeBytes: 0 },
    image: { itemCount: 0, sizeBytes: 0 },
    file: { itemCount: 0, sizeBytes: 0 },
  });
  let storageKindStatsAvailable = $state(false);
  let deletingStorageKind = $state<StorageKind | null>(null);

  onMount(() => {
    void loadHistoryConfig();
    void loadStorageKindStats();

    // Keep the per-kind statistics fresh while this panel is visible.
    listen("clipboard-item-added", scheduleRefresh).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenAdd = unlisten;
    });
    listen("clipboard-history-invalidated", scheduleRefresh).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenInvalidated = unlisten;
    });
  });

  let disposed = false;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let unlistenAdd: (() => void) | undefined;
  let unlistenInvalidated: (() => void) | undefined;
  const scheduleRefresh = () => {
    if (refreshTimer !== undefined) clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => {
      refreshTimer = undefined;
      if (!disposed) void loadStorageKindStats();
    }, 250);
  };
  onDestroy(() => {
    disposed = true;
    if (refreshTimer !== undefined) clearTimeout(refreshTimer);
    unlistenAdd?.();
    unlistenInvalidated?.();
  });

  async function loadHistoryConfig() {
    try {
      const result = await invoke<{
        maxItems: number;
        retentionDays: number;
        recycleBinDays: number;
      }>("get_history_config");
      if (result) {
        maxItemCount = result.maxItems;
        retentionPeriodDays = result.retentionDays;
        recycleBinDays = result.recycleBinDays;
      }
    } catch (error) {
      console.error("Unable to load history config", error);
    }
  }

  async function saveHistoryConfig() {
    // HTML min/max attributes never block typed values; normalize here so
    // the inputs show the value that is actually persisted. The backend
    // setters clamp again as the authoritative guard.
    maxItemCount = clampHistoryNumber(maxItemCount, 100, 1_000_000, 100);
    retentionPeriodDays = clampHistoryNumber(retentionPeriodDays, 1, 365, 1);
    recycleBinDays = clampHistoryNumber(recycleBinDays, 0, 365, 0);
    try {
      await invoke("set_history_config", {
        maxItems: maxItemCount,
        retentionDays: retentionPeriodDays,
        recycleBinDays: recycleBinDays,
      });
    } catch (error) {
      console.error("Unable to save history config", error);
      onfeedback(error instanceof Error ? error.message : String(error), false);
    }
  }

  function clampHistoryNumber(value: number, min: number, max: number, fallback: number): number {
    if (!Number.isFinite(value)) return fallback;
    return Math.min(max, Math.max(min, Math.round(value)));
  }

  async function loadStorageKindStats(): Promise<boolean> {
    try {
      const entries = await Promise.all(
        storageKinds.map(async ({ kind }) => {
          const stats = await getStorageKindStats(kind);
          if (!stats) throw new Error("Storage kind statistics are unavailable");
          return [kind, stats] as const;
        }),
      );
      storageKindStats = Object.fromEntries(entries) as Record<StorageKind, StorageKindStats>;
      storageKindStatsAvailable = true;
      return true;
    } catch (error) {
      console.error("Unable to load storage kind statistics", error);
      storageKindStats = {
        text: { itemCount: 0, sizeBytes: 0 },
        link: { itemCount: 0, sizeBytes: 0 },
        image: { itemCount: 0, sizeBytes: 0 },
        file: { itemCount: 0, sizeBytes: 0 },
      };
      storageKindStatsAvailable = false;
      return false;
    }
  }

  function storageKindLabel(kind: StorageKind): string {
    return _t(storageKinds.find((entry) => entry.kind === kind)?.labelKey ?? "filter.text");
  }

  async function deleteStorageKind(kind: StorageKind) {
    if (deletingStorageKind) return;

    const label = storageKindLabel(kind);
    deletingStorageKind = kind;

    try {
      const freshStats = await getStorageKindStats(kind);
      if (!freshStats) throw new Error(_t("storage.storageUnavailable"));
      storageKindStats = { ...storageKindStats, [kind]: freshStats };
      storageKindStatsAvailable = true;
      if (freshStats.itemCount === 0) {
        onfeedback(_t("storage.deleteKindNoData", { kind: label }), true);
        return;
      }

      const confirmed = window.confirm(
        _t("storage.deleteKindConfirm", {
          kind: label,
          count: freshStats.itemCount,
          size: formatBytes(freshStats.sizeBytes),
        }),
      );
      if (!confirmed) return;

      const result = await permanentlyDeleteStorageKind(kind, freshStats);
      const warnings = [...result.warnings];
      try {
        storageKindStats = { ...storageKindStats, [kind]: await getStorageKindStats(kind) };
      } catch (error) {
        console.error("Unable to refresh storage statistics after deletion", error);
        warnings.push(_t("storage.deleteKindRefreshFailed"));
      }

      const params = {
        kind: label,
        count: result.deletedCount,
        size: formatBytes(result.deletedSizeBytes),
        files: result.removedFiles,
      };
      if (warnings.length > 0) {
        onfeedback(
          _t("storage.deleteKindPartial", {
            ...params,
            warning: warnings.join("; "),
          }),
          false,
        );
      } else {
        onfeedback(_t("storage.deleteKindSuccess", params), true);
      }
    } catch (error) {
      console.error(`Unable to permanently delete ${kind} storage`, error);
      await loadStorageKindStats();
      onfeedback(
        _t("storage.deleteKindFailed", {
          kind: label,
          error: error instanceof Error ? error.message : String(error),
        }),
        false,
      );
    } finally {
      deletingStorageKind = null;
    }
  }
</script>

<div class="settings-scroll">
  <section class="setting-card setting-card-row" data-settings-search-id="storage.retention-period">
    <span class="setting-icon"><AppIcon name="filter" size={17} /></span>
    <span class="setting-label">{_t("captureSettings.retentionPeriod")}</span>
    <input
      type="number"
      bind:value={retentionPeriodDays}
      min="1"
      max="365"
      onchange={saveHistoryConfig}
    />
    <span class="number-suffix">{_t("captureSettings.days")}</span>
  </section>

  <section class="setting-card setting-card-row" data-settings-search-id="storage.max-item-count">
    <span class="setting-icon"><AppIcon name="file" size={17} /></span>
    <span class="setting-label">{_t("captureSettings.maxItemCount")}</span>
    <input
      type="number"
      bind:value={maxItemCount}
      min="100"
      step="100"
      onchange={saveHistoryConfig}
    />
    <span class="number-suffix">{_t("storage.recordCountUnit")}</span>
  </section>

  <section class="setting-card setting-card-row" data-settings-search-id="storage.recycle-bin-days">
    <span class="setting-icon"><AppIcon name="trash" size={17} /></span>
    <span class="setting-label">{_t("captureSettings.recycleBinDays")}</span>
    <input
      type="number"
      bind:value={recycleBinDays}
      min="0"
      max="365"
      onchange={saveHistoryConfig}
    />
    <span class="number-suffix">{_t("captureSettings.days")}</span>
  </section>

  <section
    class="setting-card storage-kind-delete-card"
    data-settings-search-id="storage.delete-by-kind"
  >
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="trash" size={17} /></span>
      <div>
        <strong>{_t("storage.deleteByKindTitle")}</strong>
        <p>{_t("storage.deleteByKindDesc")}</p>
      </div>
    </div>
    <div class="storage-kind-delete-list">
      {#each storageKinds as entry (entry.kind)}
        <div class="storage-kind-delete-row">
          <span class="storage-kind-icon"><AppIcon name={entry.icon} size={15} /></span>
          <div class="storage-kind-delete-copy">
            <strong>{_t(entry.labelKey)}</strong>
            <span>
              {storageKindStatsAvailable
                ? _t("storage.deleteKindCount", {
                    count: storageKindStats[entry.kind].itemCount,
                    size: formatBytes(storageKindStats[entry.kind].sizeBytes),
                  })
                : "—"}
            </span>
          </div>
          <button
            type="button"
            class="danger-action"
            disabled={!storageKindStatsAvailable ||
              deletingStorageKind !== null ||
              storageKindStats[entry.kind].itemCount === 0}
            onclick={() => deleteStorageKind(entry.kind)}
          >
            {deletingStorageKind === entry.kind
              ? _t("storage.deletingKind")
              : _t("storage.deleteKindAction")}
          </button>
        </div>
      {/each}
    </div>
    <p class="storage-kind-delete-scope">{_t("storage.deleteByKindScope")}</p>
  </section>
</div>

<style>
  .storage-kind-delete-list {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
    margin-top: 12px;
  }

  .storage-kind-delete-row {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
    padding: 9px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-control-radius);
    background: var(--input-bg);
  }

  .storage-kind-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 27px;
    height: 27px;
    flex-shrink: 0;
    border-radius: var(--settings-icon-radius);
    color: var(--text-muted);
    background: var(--hover-bg);
  }

  .storage-kind-delete-copy {
    display: grid;
    min-width: 0;
    flex: 1;
    gap: 2px;
  }

  .storage-kind-delete-copy strong {
    color: var(--text-primary);
    font-size: var(--settings-heading-size);
    font-weight: 560;
  }

  .storage-kind-delete-copy span,
  .storage-kind-delete-scope {
    color: var(--text-muted);
    font-size: var(--settings-description-size);
  }

  .storage-kind-delete-scope {
    margin: 9px 0 0;
    line-height: 1.45;
  }

  .danger-action {
    min-width: 68px;
    padding: 6px 9px;
    border: 1px solid color-mix(in srgb, var(--danger-color) 35%, transparent);
    border-radius: var(--settings-control-radius);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, var(--surface-bg));
    font: inherit;
    font-size: var(--settings-control-size);
    white-space: nowrap;
  }

  .danger-action:hover:not(:disabled) {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--danger-color) 35%, var(--surface-bg));
  }

  .danger-action:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
