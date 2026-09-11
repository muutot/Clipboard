<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";

  interface Props {
    onsave: () => void;
    onsaveasnew: () => void;
    oncancel: () => void;
  }

  let { onsave, onsaveasnew, oncancel }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);
</script>

<div class="edit-actions">
  <button type="button" class="edit-save" onclick={onsave}>
    <AppIcon name="check" size={14} strokeWidth={2.5} />
    {_t("edit.save")}
  </button>
  <button type="button" class="edit-save-as-new" onclick={onsaveasnew}>
    <AppIcon name="copy" size={14} strokeWidth={2.5} />
    {_t("edit.saveAsNew")}
  </button>
  <button type="button" class="edit-cancel" onclick={oncancel}>
    <AppIcon name="x" size={14} strokeWidth={2.5} />
    {_t("edit.cancel")}
  </button>
</div>

<style>
  .edit-actions {
    display: flex;
    gap: 6px;
    margin-top: 8px;
    justify-content: flex-end;
  }

  .edit-actions button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 5px 12px;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-muted);
    line-height: 1;
    background: var(--card-bg);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      background 100ms ease,
      border-color 100ms ease;
  }

  .edit-actions button.edit-save {
    color: var(--text-primary);
    background: var(--hover-bg);
    border-color: var(--text-faint);
  }

  .edit-actions button.edit-save:hover {
    color: #fff;
    background: var(--border-color);
    border-color: var(--text-faint);
  }

  .edit-actions button.edit-save-as-new {
    color: var(--text-muted);
    background: var(--card-bg);
    border-color: var(--border-color);
  }

  .edit-actions button.edit-save-as-new:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
    border-color: var(--text-faint);
  }

  .edit-actions button.edit-save-as-new:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .edit-actions button.edit-cancel:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }
</style>
