<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { formatBytes, assetUrl } from "$lib/utils/format";
  import { isTauriRuntime } from "$lib/services/runtime";

  interface Props {
    item: ClipboardItem;
  }

  let { item }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  const FILE_PREVIEW_LIMIT = 512 * 1024;
  const TEXT_FILE_EXTENSIONS = new Set([
    "c",
    "cc",
    "conf",
    "cpp",
    "css",
    "csv",
    "h",
    "hpp",
    "html",
    "htm",
    "ini",
    "java",
    "js",
    "json",
    "jsx",
    "log",
    "md",
    "mjs",
    "py",
    "rs",
    "sh",
    "sql",
    "svg",
    "toml",
    "ts",
    "tsx",
    "txt",
    "vue",
    "xml",
    "yaml",
    "yml",
  ]);

  let filePreviewState = $state<"idle" | "loading" | "ready" | "unavailable" | "failed">("idle");
  let filePreviewText = $state("");
  let filePreviewTruncated = $state(false);
  let filePreviewRequest = 0;
  let loadedPreviewKey = "";

  function isTextFile(target: ClipboardItem): boolean {
    const mime = target.mimeType?.toLowerCase() ?? "";
    if (
      mime.startsWith("text/") ||
      [
        "application/json",
        "application/javascript",
        "application/xml",
        "application/yaml",
        "application/toml",
      ].includes(mime)
    ) {
      return true;
    }
    const extension = target.resourceMetadata?.extension?.toLowerCase();
    return extension !== undefined && TEXT_FILE_EXTENSIONS.has(extension);
  }

  function filePreviewPath(target: ClipboardItem): string | undefined {
    return (
      target.resourceMetadata?.storagePath ??
      target.resourceMetadata?.resourcePath ??
      target.resourcePath ??
      undefined
    );
  }

  $effect(() => {
    const target = item;
    // Reading the `item` proxy tracks every property, so any unrelated update
    // (e.g. saving tags or a title rename) would replace the object and
    // re-fetch the preview. Only the fields that determine the preview are
    // meaningful: bail out when none of them changed.
    const previewKey = target
      ? [
          target.kind,
          target.fileMeta?.length ?? 0,
          target.mimeType ?? "",
          target.resourceMetadata?.extension ?? "",
          filePreviewPath(target) ?? "",
        ].join("\u0000")
      : "";
    if (previewKey === loadedPreviewKey) return;
    loadedPreviewKey = previewKey;

    const request = ++filePreviewRequest;
    filePreviewText = "";
    filePreviewTruncated = false;

    if (
      !target ||
      target.kind !== "file" ||
      (target.fileMeta?.length ?? 0) !== 1 ||
      !isTextFile(target)
    ) {
      filePreviewState = target?.kind === "file" ? "unavailable" : "idle";
      return;
    }

    const path = filePreviewPath(target);
    const url = assetUrl(path);
    if (!url || !isTauriRuntime()) {
      filePreviewState = "unavailable";
      return;
    }

    filePreviewState = "loading";
    void fetch(url)
      .then(async (response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const buffer = await response.arrayBuffer();
        const truncated = buffer.byteLength > FILE_PREVIEW_LIMIT;
        const bytes = truncated ? buffer.slice(0, FILE_PREVIEW_LIMIT) : buffer;
        const text = new TextDecoder("utf-8", { fatal: false }).decode(bytes);
        if (request !== filePreviewRequest) return;
        filePreviewText = text;
        filePreviewTruncated = truncated;
        filePreviewState = "ready";
      })
      .catch(() => {
        if (request === filePreviewRequest) filePreviewState = "failed";
      });
  });
</script>

<div class="file-full-preview">
  {#if item.fileMeta && item.fileMeta.length > 1}
    <div class="file-tree">
      {#each item.fileMeta as file, i}
        <div class="file-tree-item">
          <span class="file-tree-icon">
            {#if i === item.fileMeta!.length - 1}
              └─
            {:else}
              ├─
            {/if}
          </span>
          <span class="file-tree-name">{file.name}</span>
          <span class="file-tree-size">{formatBytes(file.size)}</span>
        </div>
      {/each}
    </div>
  {:else if filePreviewState === "loading"}
    <div class="file-preview-state">{_t("detail.filePreviewLoading")}</div>
  {:else if filePreviewState === "ready"}
    <pre class="file-content-preview">{filePreviewText}</pre>
    {#if filePreviewTruncated}
      <span class="file-preview-note"
        >{_t("detail.filePreviewTruncated", {
          size: Math.round(FILE_PREVIEW_LIMIT / 1024),
        })}</span
      >
    {/if}
  {:else if filePreviewState === "failed"}
    <div class="file-preview-state file-preview-error">{_t("detail.filePreviewFailed")}</div>
  {:else if filePreviewState === "unavailable" && isTextFile(item)}
    <div class="file-preview-state">{_t("detail.filePreviewUnavailable")}</div>
  {:else}
    <AppIcon name="file" size={48} strokeWidth={1.5} />
    <strong>{item.fileName ?? item.title}</strong>
    {#if item.sizeBytes}
      <span class="file-tree-size">{formatBytes(item.sizeBytes)}</span>
    {/if}
  {/if}
</div>

<style>
  .file-full-preview {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    min-height: 180px;
    padding: 16px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    color: var(--text-muted);
    background: var(--input-bg);
  }

  .file-full-preview strong {
    color: var(--text-primary);
    font-size: 13px;
  }

  .file-full-preview span {
    color: var(--text-muted);
    font-size: 11px;
  }

  .file-preview-state {
    max-width: 100%;
    color: var(--text-muted);
    font-size: 11px;
    text-align: center;
  }

  .file-preview-error {
    color: var(--danger-color);
  }

  .file-content-preview {
    width: 100%;
    max-height: 400px;
    box-sizing: border-box;
    margin: 0;
    padding: 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font:
      11px/1.55 "Cascadia Code",
      Consolas,
      monospace;
    text-align: left;
    white-space: pre-wrap;
    overflow: auto;
    overflow-wrap: anywhere;
  }

  .file-preview-note {
    max-width: 100%;
    color: var(--text-muted);
    font-size: 10px;
    text-align: center;
  }

  .file-tree {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 300px;
    overflow-y: auto;
    width: 100%;
    padding: 8px;
    background: var(--input-bg);
    border-radius: 6px;
    font-family: monospace;
    font-size: 13px;
  }

  .file-tree-item {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 0;
  }

  .file-tree-icon {
    color: var(--text-faint);
    white-space: pre;
    flex-shrink: 0;
  }

  .file-tree-name {
    color: var(--text-secondary);
    word-break: break-all;
    flex: 1;
  }

  .file-tree-size {
    color: var(--text-muted);
    white-space: nowrap;
    margin-left: 8px;
    flex-shrink: 0;
  }
</style>
