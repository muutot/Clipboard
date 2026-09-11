<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { writeClipboardText } from "$lib/services/clipboard";
  import { invoke } from "@tauri-apps/api/core";

  interface Props {
    item: ClipboardItem;
    onocrupdate: (id: string, patch: Partial<ClipboardItem>) => void;
  }

  let { item, onocrupdate }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  let ocrFeedbackTimer: ReturnType<typeof setTimeout> | undefined;
  let regeneratingOcr = $state(false);
  let ocrFeedback = $state("");

  function copyText(text: string) {
    void writeClipboardText(text).catch((err) => console.error("Copy to clipboard failed:", err));
  }

  async function regenerateOcr() {
    if (item.kind !== "image" || regeneratingOcr) return;

    const targetId = item.id;
    regeneratingOcr = true;
    ocrFeedback = "";
    try {
      const queued = await invoke<boolean>("regenerate_clipboard_item_ocr", { id: targetId });
      if (!queued) throw new Error(_t("detail.ocrUnavailable"));

      // Route the patch through the parent so every copy of the entry
      // (items/indexedItems/searchCache/detailItem) stays in sync instead of
      // relying on this prop being a live deep proxy.
      onocrupdate(targetId, { ocrStatus: "pending", ocrText: undefined, ocrError: undefined });
      if (item.id === targetId) {
        ocrFeedback = _t("detail.regenerationQueued");
      }
    } catch (error) {
      if (item.id === targetId) {
        ocrFeedback = error instanceof Error ? error.message : String(error);
      }
    } finally {
      regeneratingOcr = false;
      if (ocrFeedbackTimer !== undefined) clearTimeout(ocrFeedbackTimer);
      if (item.id === targetId) {
        ocrFeedbackTimer = setTimeout(() => {
          ocrFeedbackTimer = undefined;
          if (item.id === targetId) ocrFeedback = "";
        }, 3000);
      }
    }
  }

  $effect(() => {
    return () => {
      if (ocrFeedbackTimer !== undefined) {
        clearTimeout(ocrFeedbackTimer);
        ocrFeedbackTimer = undefined;
      }
    };
  });
</script>

<div class="ocr-section">
  {#if item.kind === "image"}
    <div class="ocr-toolbar">
      {#if item.ocrStatus === "completed" && item.ocrText}
        <div class="ocr-status ocr-completed">
          <span class="ocr-dot"></span>
          {_t("detail.completed")}
        </div>
      {/if}
      <div class="ocr-actions">
        <button
          type="button"
          class="ocr-regenerate-btn"
          disabled={regeneratingOcr ||
            item.ocrStatus === "pending" ||
            item.ocrStatus === "processing"}
          onclick={regenerateOcr}
        >
          {regeneratingOcr ? _t("detail.regenerating") : _t("detail.regenerate")}
        </button>
        {#if item.ocrStatus === "completed" && item.ocrText}
          <button type="button" class="ocr-copy-btn" onclick={() => copyText(item.ocrText ?? "")}>
            {_t("detail.copyOcrText")}
          </button>
        {/if}
      </div>
    </div>
    {#if ocrFeedback}
      <div class="ocr-feedback">{ocrFeedback}</div>
    {/if}
  {/if}
  {#if item.ocrStatus === "completed" && item.ocrText}
    <pre class="ocr-content">{item.ocrText}</pre>
  {:else if item.ocrStatus === "pending" || item.ocrStatus === "processing"}
    <div class="ocr-status ocr-pending">
      <span class="ocr-dot"></span>
      {_t("detail.pending")}
    </div>
  {:else if item.ocrStatus === "failed"}
    <div class="ocr-empty ocr-failed">
      <AppIcon name="scan" size={32} strokeWidth={1.5} />
      <span>{_t("detail.ocrFailed")}</span>
      {#if item.ocrError}<small>{item.ocrError}</small>{/if}
    </div>
  {:else}
    <div class="ocr-empty">
      <AppIcon name="image" size={32} strokeWidth={1.5} />
      <span>{_t("detail.noOcr")}</span>
    </div>
  {/if}
</div>

<style>
  .ocr-section {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .ocr-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .ocr-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .ocr-regenerate-btn {
    padding: 5px 10px;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-secondary);
    background: var(--card-bg);
    font-size: 11px;
    cursor: pointer;
    transition:
      background 100ms ease,
      color 100ms ease,
      border-color 100ms ease;
  }

  .ocr-regenerate-btn:hover:not(:disabled) {
    border-color: var(--text-faint);
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .ocr-regenerate-btn:disabled {
    cursor: default;
    opacity: 0.55;
  }

  .ocr-feedback {
    color: var(--selection-color);
    font-size: 11px;
  }

  .ocr-status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .ocr-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .ocr-completed {
    color: color-mix(in srgb, var(--success-color) 75%, white);
  }

  .ocr-completed .ocr-dot {
    background: var(--success-color);
    box-shadow: 0 0 6px color-mix(in srgb, var(--success-color) 40%, transparent);
  }

  .ocr-copy-btn {
    padding: 2px 8px;
    font-size: 11px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    transition:
      background 0.15s,
      color 0.15s;
  }

  .ocr-copy-btn:hover {
    background: rgba(255, 255, 255, 0.15);
    color: var(--text-primary);
  }

  .ocr-pending {
    color: color-mix(in srgb, var(--warning-color) 75%, white);
  }

  .ocr-pending .ocr-dot {
    background: var(--warning-color);
    box-shadow: 0 0 6px color-mix(in srgb, var(--warning-color) 40%, transparent);
  }

  .ocr-content {
    margin: 0;
    padding: 14px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font:
      12px/1.6 "Cascadia Code",
      Consolas,
      monospace;
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .ocr-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    min-height: 140px;
    color: var(--text-faint);
    font-size: 12px;
  }

  .ocr-failed {
    color: var(--danger-color);
  }

  .ocr-failed small {
    max-width: 100%;
    color: color-mix(in srgb, var(--danger-color) 65%, white);
    text-align: center;
    overflow-wrap: anywhere;
  }
</style>
