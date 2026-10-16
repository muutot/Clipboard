<script lang="ts">
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    selectedCount: number;
    allSelectedFavorites: boolean;
    ondeselect: () => void;
    oncopy: () => void;
    onpasteall: () => void;
    ondelete: () => void;
    onfavorite: () => void;
  }

  let {
    selectedCount,
    allSelectedFavorites,
    ondeselect,
    oncopy,
    onpasteall,
    ondelete,
    onfavorite,
  }: Props = $props();
</script>

{#if selectedCount > 0}
  <div class="bulk-bar">
    <span class="bulk-count">{_t("bulk.totalN", { count: selectedCount })}</span>
    <div class="bulk-actions">
      <button type="button" onclick={oncopy}>
        <AppIcon name="copy" size={14} />
        <span>{_t("bulk.copy")}</span>
      </button>
      <button type="button" onclick={onpasteall}>
        <AppIcon name="clipboard" size={14} />
        <span>{_t("bulk.pasteAll")}</span>
      </button>
      <button type="button" class="danger" onclick={ondelete}>
        <AppIcon name="trash" size={14} />
        <span>{_t("bulk.delete")}</span>
      </button>
      <button type="button" onclick={onfavorite}>
        <AppIcon name="star" size={14} />
        <span>{allSelectedFavorites ? _t("bulk.unfavorite") : _t("bulk.favorite")}</span>
      </button>
    </div>
    <button type="button" class="bulk-deselect" onclick={ondeselect} title={_t("bulk.deselectAll")}>
      <AppIcon name="x" size={14} strokeWidth={2.5} />
    </button>
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

  .bulk-count {
    font-size: 11.5px;
    color: var(--text-secondary);
    font-weight: 500;
  }

  .bulk-deselect {
    display: inline-flex;
    align-items: center;
    margin-left: auto;
    padding: 4px 8px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
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
