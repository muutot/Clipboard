<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import MarkdownPreview from "$lib/components/MarkdownPreview.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type { UpdateInfo } from "$lib/services/update";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    result: UpdateInfo;
    onclose: () => void;
    mode?: "current" | "available";
  }

  let { result, onclose, mode = "available" }: Props = $props();

  let dialog = $state<HTMLDialogElement | null>(null);

  function formatDate(value: string | null): string {
    if (!value) return "";
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString();
  }

  onMount(() => {
    queueMicrotask(() => {
      dialog?.showModal();
    });
  });
</script>

<dialog
  bind:this={dialog}
  class="update-dialog"
  {onclose}
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    }
  }}
>
  <div class="update-dialog-header" class:update-dialog-header--current={mode === "current"}>
    <span class="update-dialog-icon"
      ><AppIcon name={mode === "current" ? "file" : "info"} size={18} /></span
    >
    <div class="update-dialog-title">
      <strong
        >{mode === "current" ? _t("about.releaseNotes") : _t("about.updateDialogTitle")}</strong
      >
      <p class="update-dialog-version">
        v{result.latestVersion}
        {#if result.publishedAt}
          <span class="update-dialog-date">· {formatDate(result.publishedAt)}</span>
        {/if}
      </p>
    </div>
    <button
      type="button"
      class="update-dialog-close"
      aria-label={_t("about.close")}
      onclick={onclose}
    >
      <AppIcon name="x" size={16} />
    </button>
  </div>

  {#if result.releaseNotes}
    <div class="update-dialog-body">
      <MarkdownPreview content={result.releaseNotes} />
    </div>
  {/if}

  <div class="update-dialog-footer">
    <button type="button" class="update-dialog-btn update-dialog-btn--secondary" onclick={onclose}>
      {_t("about.close")}
    </button>
    {#if mode === "available"}
      <a
        class="update-dialog-btn update-dialog-btn--primary"
        href={result.releaseUrl}
        target="_blank"
        rel="noopener noreferrer"
        onclick={onclose}
      >
        {_t("about.download")}
      </a>
    {/if}
  </div>
</dialog>

<style>
  .update-dialog {
    width: min(560px, calc(100vw - 48px));
    max-height: calc(100vh - 96px);
    display: flex;
    flex-direction: column;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 12px;
    background: var(--surface-bg);
    color: var(--text-primary);
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.32),
      0 2px 8px rgba(0, 0, 0, 0.16);
    overflow: hidden;
  }

  .update-dialog::backdrop {
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(2px);
  }

  .update-dialog-header {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 16px 16px 12px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .update-dialog-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: 8px;
    background: color-mix(in srgb, var(--info-color, #3e63dd) 14%, transparent);
    color: var(--info-color, #3e63dd);
  }

  .update-dialog-header--current .update-dialog-icon {
    background: color-mix(in srgb, var(--text-muted) 14%, transparent);
    color: var(--text-muted);
  }

  .update-dialog-title {
    flex: 1;
    min-width: 0;
  }

  .update-dialog-title strong {
    font-size: 14px;
    font-weight: 600;
  }

  .update-dialog-version {
    margin: 2px 0 0;
    font-size: var(--settings-description-size);
    color: var(--text-muted);
  }

  .update-dialog-date {
    color: var(--text-muted);
    opacity: 0.8;
  }

  .update-dialog-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .update-dialog-close:hover {
    background: var(--hover-bg);
    color: var(--text-primary);
  }

  .update-dialog-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 16px;
  }

  .update-dialog-body :global(.markdown-body) {
    padding: 20px;
    border-color: var(--border-subtle);
    background: var(--input-bg);
    color: var(--text-secondary);
  }

  .update-dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid var(--border-subtle);
    background: var(--surface-bg);
  }

  .update-dialog-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 34px;
    padding: 0 16px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius);
    font: inherit;
    font-size: var(--settings-control-size);
    font-weight: 500;
    text-decoration: none;
    cursor: pointer;
    white-space: nowrap;
    transition:
      background 100ms ease,
      color 100ms ease,
      border-color 100ms ease;
  }

  .update-dialog-btn--secondary {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .update-dialog-btn--secondary:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .update-dialog-btn--primary {
    color: #fff;
    border-color: var(--info-color, #3e63dd);
    background: var(--info-color, #3e63dd);
  }

  .update-dialog-btn--primary:hover {
    filter: brightness(1.08);
  }
</style>
