<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    selectedCount: number;
    selectedActiveCount: number;
    selectedDeletedCount: number;
    activeFilter: string;
    allSelectedFavorites: boolean;
    ondeselect: () => void;
    oncopy: () => void;
    ondelete: () => void;
    onfavorite: () => void;
    onrestore: () => void;
    onpermanentdelete: () => void;
  }

  let {
    selectedCount,
    selectedActiveCount,
    selectedDeletedCount,
    activeFilter,
    allSelectedFavorites,
    ondeselect,
    oncopy,
    ondelete,
    onfavorite,
    onrestore,
    onpermanentdelete,
  }: Props = $props();
</script>

{#if selectedCount > 0}
  <div class="bulk-bar">
    <button type="button" class="bulk-deselect" onclick={ondeselect} title={_t("bulk.deselectAll")}>
      <AppIcon name="x" size={14} strokeWidth={2.5} />
      <span>{selectedCount}</span>
    </button>
    <div class="bulk-actions">
      <button type="button" onclick={oncopy}>
        <AppIcon name="copy" size={14} />
        <span>{_t("bulk.copyN", { count: selectedCount })}</span>
      </button>
      {#if selectedActiveCount > 0 && activeFilter !== "favorite"}
        <button type="button" class="danger" onclick={ondelete}>
          <AppIcon name="trash" size={14} />
          <span>{_t("bulk.deleteN", { count: selectedActiveCount })}</span>
        </button>
      {/if}
      <button type="button" onclick={onfavorite}>
        <AppIcon name="star" size={14} />
        <span
          >{allSelectedFavorites
            ? _t("bulk.unfavoriteN", { count: selectedCount })
            : _t("bulk.favoriteN", { count: selectedCount })}</span
        >
      </button>
      {#if selectedDeletedCount > 0}
        <button type="button" onclick={onrestore}>
          <AppIcon name="restore" size={14} />
          <span>{_t("bulk.restoreN", { count: selectedDeletedCount })}</span>
        </button>
        <button type="button" class="danger" onclick={onpermanentdelete}>
          <AppIcon name="trash" size={14} />
          <span>{_t("bulk.permanentDeleteN", { count: selectedDeletedCount })}</span>
        </button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .bulk-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 14px;
    border-top: 1px solid var(--border-subtle);
    background: var(--input-bg);
  }

  .bulk-deselect {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border: 1px solid var(--border-color);
    border-radius: 14px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
    font-size: 11.5px;
    transition: color 100ms ease;
  }

  .bulk-deselect:hover {
    color: var(--text-secondary);
  }

  .bulk-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .bulk-actions button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 5px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    background: var(--card-bg);
    cursor: pointer;
    font-size: 11.5px;
    font-weight: 500;
    line-height: 1;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .bulk-actions button:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .bulk-actions button.danger {
    border-color: color-mix(in srgb, var(--danger-color) 30%, transparent);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
  }

  .bulk-actions button.danger:hover {
    border-color: color-mix(in srgb, var(--danger-color) 50%, transparent);
    background: color-mix(in srgb, var(--danger-color) 10%, transparent);
    color: color-mix(in srgb, var(--danger-color) 85%, white);
  }
</style>
