<script lang="ts">
  import type { TagInfo } from "$lib/services/clipboard";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    tag: TagInfo;
    confirmDelete: boolean;
    colorPopoverOpen: boolean;
    ontoggleColorPopover: (name: string, event: MouseEvent) => void;
    oncommitRename: (tag: TagInfo, value: string) => void | Promise<void>;
    oncommitDelete: (tag: TagInfo) => void | Promise<void>;
    onreset: () => void;
  }

  let {
    tag,
    confirmDelete,
    colorPopoverOpen,
    ontoggleColorPopover,
    oncommitRename,
    oncommitDelete,
    onreset,
  }: Props = $props();
</script>

<section class="setting-card tag-row">
  <button
    type="button"
    class="tag-color-trigger"
    style={tag.color ? `--tag-accent: ${tag.color}` : undefined}
    aria-haspopup="dialog"
    aria-expanded={colorPopoverOpen}
    aria-label={_t("tags.color")}
    title={_t("tags.color")}
    onclick={(e) => ontoggleColorPopover(tag.name, e)}
  ></button>
  <input
    class="tag-name-input"
    value={tag.name}
    aria-label={_t("tags.renamePlaceholder")}
    onblur={(e) => oncommitRename(tag, (e.currentTarget as HTMLInputElement).value)}
    onkeydown={(e) => {
      if (e.key === "Enter") {
        (e.currentTarget as HTMLInputElement).blur();
      } else if (e.key === "Escape") {
        onreset();
      }
    }}
  />
  <span class="tag-sep" aria-hidden="true"></span>
  <span class="tag-count">{_t("tags.count", { count: tag.count })}</span>
  <button
    type="button"
    class="tag-delete"
    class:confirm={confirmDelete}
    onclick={() => oncommitDelete(tag)}
  >
    {confirmDelete ? _t("tags.deleteConfirm") : _t("tags.delete")}
  </button>
</section>

<style>
  .tag-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .tag-color-trigger {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 50%;
    background: color-mix(in srgb, var(--tag-accent) 45%, var(--surface-bg));
    cursor: pointer;
  }

  .tag-color-trigger:hover {
    box-shadow: 0 0 0 2px var(--hover-bg);
  }

  .tag-color-trigger[aria-expanded="true"] {
    outline: 2px solid var(--text-faint);
    outline-offset: 1px;
  }

  .tag-name-input {
    flex: 1;
    min-width: 0;
    padding: 4px 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-primary);
    background: transparent;
    font-size: 13px;
    font-weight: 600;
  }

  .tag-sep {
    flex-shrink: 0;
    width: 1px;
    height: 14px;
    background: var(--border-subtle);
  }

  .tag-name-input:hover {
    border-color: var(--border-color);
  }

  .tag-name-input:focus {
    outline: none;
    border-color: var(--text-faint);
    background: var(--input-bg, var(--surface-bg));
  }

  .tag-count {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-faint);
    white-space: nowrap;
  }

  .tag-delete {
    flex-shrink: 0;
    padding: 5px 10px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-secondary);
    background: transparent;
    font-size: 12px;
    cursor: pointer;
  }

  .tag-delete:hover {
    color: var(--danger-color);
    border-color: var(--danger-color);
  }

  .tag-delete.confirm {
    color: var(--danger-color);
    border-color: var(--danger-color);
  }
</style>
