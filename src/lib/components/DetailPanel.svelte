<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CodeEditor from "$lib/components/CodeEditor.svelte";
  import CodePreview from "$lib/components/CodePreview.svelte";
  import MarkdownPreview from "$lib/components/MarkdownPreview.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { isEditableKeyboardTarget } from "$lib/utils/keyboard";
  import { formatRelativeTime } from "$lib/utils/time";
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { untrack } from "svelte";
  import { get } from "svelte/store";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { writeClipboardText } from "$lib/services/clipboard";
  import { generalSettings } from "$lib/services/settings";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  function assetUrl(filePath: string | null | undefined): string | undefined {
    if (!filePath) return undefined;
    if (!isTauriRuntime()) return undefined;
    try {
      return convertFileSrc(filePath.replace(/\\/g, "/"));
    } catch {
      return undefined;
    }
  }

  interface Props {
    item: ClipboardItem | null;
    mode?: 'overlay' | 'split';
    startFullscreen?: boolean;
    onclose: () => void;
    oncopy: (id: string) => void;
    onedit: (id: string) => void;
    onsaveedit: (id: string, content: string) => void | Promise<boolean>;
    onrenametitle: (id: string, title: string) => void;
    onplainpaste: (id: string) => void;
    onduplicate: (id: string) => void;
    onsaveasnew: (id: string, title: string, content: string) => void;
    oncopyfilename: (id: string) => void;
  }

  let {
    item,
    mode = 'overlay',
    startFullscreen = $bindable(false),
    onclose,
    oncopy,
    onedit,
    onsaveedit,
    onrenametitle,
    onplainpaste,
    onduplicate,
    onsaveasnew,
    oncopyfilename,
  }: Props = $props();

  function copyText(text: string) {
    void writeClipboardText(text).catch(() => {});
  }

  let activeTab = $state<"preview" | "details" | "ocr">("preview");
  let editing = $state(false);
  let editingTitle = $state(false);
  let editContent = $state("");
  let editTitleContent = $state("");
  let imageFullscreen = $state(false);
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let isDragging = $state(false);
  let dragStartX = 0;
  let dragStartY = 0;
  let panStartX = 0;
  let panStartY = 0;
  let viewerWindow: any = null;
  let regeneratingOcr = $state(false);
  let ocrFeedback = $state("");
  let filePreviewState = $state<"idle" | "loading" | "ready" | "unavailable" | "failed">("idle");
  let filePreviewText = $state("");
  let filePreviewTruncated = $state(false);
  let filePreviewRequest = 0;

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

  function isTextFile(item: ClipboardItem): boolean {
    const mime = item.mimeType?.toLowerCase() ?? "";
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
    const extension = item.resourceMetadata?.extension?.toLowerCase();
    return extension !== undefined && TEXT_FILE_EXTENSIONS.has(extension);
  }

  function filePreviewPath(item: ClipboardItem): string | undefined {
    return (
      item.resourceMetadata?.storagePath ??
      item.resourceMetadata?.resourcePath ??
      item.resourcePath ??
      undefined
    );
  }

  async function openImageFullscreen() {
    if (!item?.previewPath && !item?.resourcePath) return;
    const filePath = item.previewPath || item.resourcePath;
    if (!filePath) return;

    if (get(generalSettings).imageFullscreenMode === "desktop" && isTauriRuntime()) {
      try {
        const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
        const { emit } = await import("@tauri-apps/api/event");
        const opacity = get(generalSettings).viewerBackdropOpacity / 100;

        const existing = await WebviewWindow.getByLabel("image-viewer");
        if (existing) {
          await existing.show();
          await existing.setFocus();
          await emit("viewer:open", { src: filePath, opacity });
          return;
        }

        viewerWindow = new WebviewWindow("image-viewer", {
          url: `/viewer?src=${encodeURIComponent(filePath)}&opacity=${opacity}`,
          title: "",
          width: 800,
          height: 600,
          center: true,
          decorations: false,
          transparent: true,
          maximized: true,
          skipTaskbar: true,
          alwaysOnTop: true,
        });
        viewerWindow.on("tauri://error", (e: any) => {
          console.error("[viewer] window error", e);
        });
      } catch (e) {
        console.error("[viewer] failed to open", e);
      }
      return;
    }

    // Overlay mode: show within app window
    setTimeout(() => {
      zoom = 1;
      panX = 0;
      panY = 0;
      imageFullscreen = true;
    }, 0);
  }

  async function closeImageFullscreen() {
    imageFullscreen = false;
    zoom = 1;
    panX = 0;
    panY = 0;
    if (viewerWindow) {
      try {
        await viewerWindow.hide();
      } catch {}
    }
  }

  $effect(() => {
    if (item && startFullscreen) {
      startFullscreen = false;
      openImageFullscreen();
    }
  });

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 0.9 : 1.1;
    zoom = Math.min(20, Math.max(0.1, zoom * delta));
  }

  function onMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    isDragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    panStartX = panX;
    panStartY = panY;
  }

  function onMouseMove(e: MouseEvent) {
    if (!isDragging) return;
    panX = panStartX + (e.clientX - dragStartX);
    panY = panStartY + (e.clientY - dragStartY);
  }

  function onMouseUp() {
    isDragging = false;
  }

  function onDblClick() {
    if (zoom !== 1 || panX !== 0 || panY !== 0) {
      zoom = 1;
      panX = 0;
      panY = 0;
    } else {
      zoom = 2;
    }
  }

  function formatFileSize(bytes: number): string {
    const units = ["B", "KB", "MB", "GB"];
    let value = bytes;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex += 1;
    }
    const precision = unitIndex === 0 || value >= 100 ? 0 : 1;
    return `${value.toFixed(precision)} ${units[unitIndex]}`;
  }

  async function regenerateOcr() {
    if (!item || item.kind !== "image" || regeneratingOcr) return;

    const targetItem = item;
    const targetId = targetItem.id;
    regeneratingOcr = true;
    ocrFeedback = "";
    try {
      const queued = await invoke<boolean>("regenerate_clipboard_item_ocr", { id: targetId });
      if (!queued) throw new Error(_t("detail.ocrUnavailable"));

      targetItem.ocrStatus = "pending";
      targetItem.ocrText = undefined;
      targetItem.ocrError = undefined;
      if (item?.id === targetId) {
        ocrFeedback = _t("detail.regenerationQueued");
      }
    } catch (error) {
      if (item?.id === targetId) {
        ocrFeedback = error instanceof Error ? error.message : String(error);
      }
    } finally {
      regeneratingOcr = false;
      setTimeout(() => {
        if (item?.id === targetId) {
          ocrFeedback = "";
        }
      }, 3000);
    }
  }

  $effect(() => {
    if (item) {
      untrack(() => {
        if (imageFullscreen) {
          closeImageFullscreen();
        }
      });
      imageFullscreen = false;
      zoom = 1;
      panX = 0;
      panY = 0;
      activeTab = "preview";
      editing = false;
    }
  });

  $effect(() => {
    if (imageFullscreen) {
      const url = assetUrl(item?.previewPath || item?.resourcePath);
      console.log("[fullscreen] active", { url, hasItem: !!item });
    }
  });

  $effect(() => {
    if (item?.kind !== "image" || !isTauriRuntime()) return;

    const targetItem = item;
    let disposed = false;
    let requestInFlight = false;
    const poll = () => {
      if (disposed || requestInFlight || targetItem.ocrStatus === "completed") return;

      requestInFlight = true;
      invoke<{
        fullText: string;
        status: "pending" | "processing" | "completed" | "failed";
        errorMessage: string | null;
      } | null>("get_clipboard_item_ocr", { id: targetItem.id })
        .then((result) => {
          if (disposed) return;
          if (result) {
            targetItem.ocrStatus = result.status;
            targetItem.ocrError = result.errorMessage ?? undefined;
            targetItem.ocrText = result.fullText || undefined;
          } else {
            targetItem.ocrStatus = "none";
            targetItem.ocrText = undefined;
            targetItem.ocrError = undefined;
          }
        })
        .catch(() => {
          if (disposed) return;
          targetItem.ocrStatus = "failed";
          targetItem.ocrError = _t("detail.ocrReadFailed");
        })
        .finally(() => {
          requestInFlight = false;
        });
    };

    poll();
    const interval = setInterval(poll, 2000);
    return () => {
      disposed = true;
      clearInterval(interval);
    };
  });

  $effect(() => {
    const target = item;
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

  const emails = $derived(
    item
      ? [
          ...new Set(
            [item.title, item.preview]
              .filter(Boolean)
              .join(" ")
              .match(/[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g) ?? [],
          ),
        ]
      : [],
  );
  const urls = $derived(
    item
      ? [
          ...new Set(
            [item.title, item.preview]
              .filter(Boolean)
              .join(" ")
              .match(/https?:\/\/[^\s)]+/g) ?? [],
          ),
        ]
      : [],
  );
  const phones = $derived(
    item
      ? [
          ...new Set(
            [item.title, item.preview]
              .filter(Boolean)
              .join(" ")
              .match(/(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4,}/g) ?? [],
          ),
        ]
      : [],
  );
  const colors = $derived(
    item
      ? [
          ...new Set(
            [item.title, item.preview]
              .filter(Boolean)
              .join(" ")
              .match(/#(?:[0-9a-fA-F]{3}){1,2}\b/g) ?? [],
          ),
        ]
      : [],
  );

  const hasSpecialMarkers = $derived(
    emails.length > 0 || urls.length > 0 || phones.length > 0 || colors.length > 0,
  );

  const detailContent = $derived(item ? item.textContent || item.title : "");
  const resourceMetadata = $derived(item?.resourceMetadata);
  const resourceFiles = $derived(item?.fileMeta ?? []);
  const rawMetadata = $derived(formatMetadataJson(item?.metadataJson));
  const isCode = $derived(item ? detectCodeLanguage(detailContent) !== null : false);
  const isMarkdown = $derived(
    item ? /^#{1,6}\s|^>\s|^-\s|^\*\*|^\`\`\`|^\[.+\]\(.+\)/m.test(detailContent) : false,
  );

  function detectCodeLanguage(text: string): string | null {
    const patterns: [RegExp, string][] = [
      [
        new RegExp(
          "^(import|export)\\s|interface\\s|type\\s\\w+\\s*=\\s*\\{|const\\s\\w+:\\s*\\w+|:\\s*(string|number|boolean|unknown|any)\\b|function\\s\\w+\\(|\\.\\.\\.\\w+|useState|useEffect|async\\s+function",
          "ms",
        ),
        "TypeScript",
      ],
      [new RegExp("^<\\w+[^>]*>|<\\/\\w+>|className=|useState|useEffect|props\\.", "m"), "JSX"],
      [
        new RegExp("^use\\s|^fn\\s|let\\s+mut|struct\\s|impl\\s|^\\s*pub\\s|^\\s*mod\\s", "m"),
        "Rust",
      ],
      [
        new RegExp("^def\\s|^import\\s\\w|^\\s*class\\s|^\\s*from\\s|print\\(|lambda\\s", "m"),
        "Python",
      ],
      [
        new RegExp(
          '^\\s*[{\\[]\\s*$|"[^"]*"\\s*:|^\\s*"|function\\s*\\(|require\\(|module\\.exports',
          "m",
        ),
        "JSON",
      ],
      [
        new RegExp("^<!DOCTYPE|<html|<head|<body|<div|<span|\\.class\\s*\\{|#id\\s*\\{", "m"),
        "HTML",
      ],
      [
        new RegExp(
          "^SELECT\\s|^INSERT\\s|^UPDATE\\s|^DELETE\\s|^CREATE\\s|^\\s*FROM\\s|^\\s*WHERE\\s",
          "mi",
        ),
        "SQL",
      ],
      [
        new RegExp("^#!/|^\\s*(echo|export|cd|ls|grep|mkdir|sudo|apt|npm|yarn|git)\\s", "m"),
        "Shell",
      ],
      [
        new RegExp("^\\.\\w+\\s*\\{|^\\s*color:|^\\s*margin:|^\\s*padding:|@media|@keyframes", "m"),
        "CSS",
      ],
      [
        new RegExp(
          "^(function|var|const|let)\\s|^\\s*console\\.|document\\.|window\\.|require\\(",
          "m",
        ),
        "JavaScript",
      ],
    ];

    for (const [regex, lang] of patterns) {
      if (regex.test(text)) return lang;
    }
    return null;
  }

  function formatDateTime(ts: number): string {
    return new Date(ts).toLocaleString();
  }

  function formatMetadataTime(ts: number | undefined): string {
    return ts === undefined ? _t("detail.unknown") : formatDateTime(ts);
  }

  function formatExactSize(bytes: number): string {
    return `${formatFileSize(bytes)} (${bytes.toLocaleString()} B)`;
  }

  function formatMetadataJson(metadataJson: string | null | undefined): string {
    if (!metadataJson) return "";
    try {
      return JSON.stringify(JSON.parse(metadataJson), null, 2);
    } catch {
      return metadataJson;
    }
  }

  function getKindLabel(kind: string): string {
    const map: Record<string, string> = {
      text: _t("filter.text"),
      link: _t("filter.link"),
      image: _t("filter.image"),
      file: _t("filter.file"),
    };
    return map[kind] ?? kind;
  }

  function getOcrStatusLabel(status: NonNullable<ClipboardItem["ocrStatus"]>): string {
    const map: Record<NonNullable<ClipboardItem["ocrStatus"]>, string> = {
      pending: _t("detail.pending"),
      processing: _t("detail.processing"),
      completed: _t("detail.completed"),
      failed: _t("detail.failed"),
      none: _t("detail.noOcr"),
    };
    return map[status];
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!item || event.key !== "Escape" || event.defaultPrevented) return;

    if (imageFullscreen) {
      event.preventDefault();
      closeImageFullscreen();
      return;
    }

    const editorTarget =
      isEditableKeyboardTarget(event.target) &&
      event.target instanceof Element &&
      event.target.closest(".detail-panel") !== null;
    if (editorTarget) {
      event.preventDefault();
      editing = false;
      editingTitle = false;
      return;
    }

    if (editing) {
      event.preventDefault();
      editing = false;
      return;
    }

    event.preventDefault();
    onclose();
  }

  async function saveEdit() {
    if (!item || !editContent.trim()) return;
    const saved = await onsaveedit(item.id, editContent.trim());
    if (saved !== false) editing = false;
  }

  function saveTitleEdit() {
    if (!item || !editTitleContent.trim()) {
      editingTitle = false;
      return;
    }
    onrenametitle(item.id, editTitleContent.trim());
    editingTitle = false;
  }

  function saveAsNew() {
    if (!item || !editContent.trim()) return;
    onsaveasnew(item.id, item.title, editContent.trim());
    editing = false;
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if item}
  {#if mode !== 'split'}
    <div
      class="detail-backdrop"
      class:fullscreen-backdrop={imageFullscreen}
      onclick={imageFullscreen ? closeImageFullscreen : onclose}
      aria-hidden="true"
    ></div>
  {/if}
  <div
    class="detail-panel"
    class:fullscreen={imageFullscreen}
    class:inline={mode === 'split'}
    role="dialog"
    aria-modal={mode !== 'split'}
    aria-label={_t("detail.title")}
  >
    <div class="detail-header" class:hidden={imageFullscreen} data-tauri-drag-region>
      <button class="back-btn" type="button" onclick={onclose} aria-label={_t("detail.back")}>
        <AppIcon name="chevron-left" size={18} strokeWidth={2} />
      </button>
      <div class="header-info">
        <span class="header-kind">{getKindLabel(item.kind)}</span>
        {#if editingTitle}
          <div class="header-edit-row">
            <input
              class="header-title-input"
              bind:value={editTitleContent}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  saveTitleEdit();
                }
                if (e.key === "Escape") {
                  editingTitle = false;
                }
              }}
              onblur={() => saveTitleEdit()}
            />
            <button class="header-save-btn" type="button" onclick={saveTitleEdit}>
              <AppIcon name="check" size={14} strokeWidth={2.5} />
            </button>
          </div>
        {:else}
          <span
            class="header-title"
            role="button"
            tabindex="0"
            aria-label={_t("edit.edit")}
            ondblclick={() => {
              editTitleContent = item.title;
              editingTitle = true;
            }}
            onkeydown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                editTitleContent = item.title;
                editingTitle = true;
              }
            }}
          >
            {#if item.kind === "file" && item.fileMeta && item.fileMeta.length > 1}
              <AppIcon name="file" size={15} /> {item.fileMeta.length} {_t("detail.files")}
            {:else}
              {item.title.split("\n")[0]}
            {/if}
          </span>
        {/if}
      </div>
    </div>

    <nav class="detail-tabs" aria-label={_t("detail.tabAriaLabel")}>
      <button
        class:active={activeTab === "preview"}
        type="button"
        onclick={() => (activeTab = "preview")}
      >
        {_t("detail.preview")}
      </button>
      <button
        class:active={activeTab === "details"}
        type="button"
        onclick={() => (activeTab = "details")}
      >
        {_t("detail.details")}
      </button>
      <button class:active={activeTab === "ocr"} type="button" onclick={() => (activeTab = "ocr")}>
        {_t("detail.ocr")}
      </button>
    </nav>

    <div class="detail-body">
      {#if activeTab === "preview"}
        <div class="preview-section">
          {#if item.kind === "image"}
            <div class="image-full-preview">
              {#if assetUrl(item.previewPath || item.resourcePath)}
                <img
                  src={assetUrl(item.previewPath || item.resourcePath)}
                  alt={item.preview || item.title}
                />
                <button
                  type="button"
                  class="image-fullscreen-btn"
                  onclick={(e) => {
                    e.stopPropagation();
                    openImageFullscreen();
                  }}
                  aria-label={_t("detail.fullscreenPreview")}
                >
                  <AppIcon name="maximize" size={16} strokeWidth={2} />
                </button>
              {:else}
                <div class="image-placeholder">
                  <AppIcon name="image" size={48} strokeWidth={1.5} />
                  {#if item.imageMeta}
                    <span class="image-meta">{item.imageMeta.width} × {item.imageMeta.height}</span>
                  {/if}
                </div>
              {/if}
            </div>
          {:else if item.kind === "file"}
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
                      <span class="file-tree-size">{formatFileSize(file.size)}</span>
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
                <div class="file-preview-state file-preview-error">
                  {_t("detail.filePreviewFailed")}
                </div>
              {:else if filePreviewState === "unavailable" && isTextFile(item)}
                <div class="file-preview-state">{_t("detail.filePreviewUnavailable")}</div>
              {:else}
                <AppIcon name="file" size={48} strokeWidth={1.5} />
                <strong>{item.fileName ?? item.title}</strong>
                {#if item.sizeBytes}
                  <span class="file-tree-size">{formatFileSize(item.sizeBytes)}</span>
                {/if}
              {/if}
            </div>
          {:else if isCode && !isMarkdown}
            {#if editing}
              <CodeEditor
                content={editContent}
                language={detectCodeLanguage(editContent)}
                editorLabel={_t("edit.edit")}
                previewLabel={_t("detail.preview")}
                placeholder={_t("edit.placeholder")}
                oncontentchange={(content) => (editContent = content)}
              />
              <div class="edit-actions">
                <button type="button" class="edit-save" onclick={saveEdit}>
                  <AppIcon name="check" size={14} strokeWidth={2.5} />
                  {_t("edit.save")}
                </button>
                <button type="button" class="edit-save-as-new" onclick={saveAsNew}>
                  <AppIcon name="copy" size={14} strokeWidth={2.5} />
                  {_t("edit.saveAsNew")}
                </button>
                <button type="button" class="edit-cancel" onclick={() => (editing = false)}>
                  <AppIcon name="x" size={14} strokeWidth={2.5} />
                  {_t("edit.cancel")}
                </button>
              </div>
            {:else}
              <CodePreview content={detailContent} />
            {/if}
          {:else if isMarkdown}
            {#if editing}
              <div class="edit-area">
                <textarea
                  bind:value={editContent}
                  rows={Math.min(20, Math.max(5, editContent.split("\n").length))}
                  placeholder={_t("edit.placeholder")}></textarea>
              </div>
              <div class="edit-actions">
                <button type="button" class="edit-save" onclick={saveEdit}>
                  <AppIcon name="check" size={14} strokeWidth={2.5} />
                  {_t("edit.save")}
                </button>
                <button type="button" class="edit-save-as-new" onclick={saveAsNew}>
                  <AppIcon name="copy" size={14} strokeWidth={2.5} />
                  {_t("edit.saveAsNew")}
                </button>
                <button type="button" class="edit-cancel" onclick={() => (editing = false)}>
                  <AppIcon name="x" size={14} strokeWidth={2.5} />
                  {_t("edit.cancel")}
                </button>
              </div>
            {:else}
              <MarkdownPreview content={detailContent} />
            {/if}
          {:else}
            {#if editing}
              <div class="edit-area">
                <textarea
                  bind:value={editContent}
                  rows={Math.min(20, Math.max(5, editContent.split("\n").length))}
                  placeholder={_t("edit.placeholder")}></textarea>
              </div>
              <div class="edit-actions">
                <button type="button" class="edit-save" onclick={saveEdit}>
                  <AppIcon name="check" size={14} strokeWidth={2.5} />
                  {_t("edit.save")}
                </button>
                <button type="button" class="edit-save-as-new" onclick={saveAsNew}>
                  <AppIcon name="copy" size={14} strokeWidth={2.5} />
                  {_t("edit.saveAsNew")}
                </button>
                <button type="button" class="edit-cancel" onclick={() => (editing = false)}>
                  <AppIcon name="x" size={14} strokeWidth={2.5} />
                  {_t("edit.cancel")}
                </button>
              </div>
            {:else}
              <pre class="content-full">{item.textContent || item.title}</pre>
            {/if}
          {/if}
        </div>

        <div class="detail-actions">
          <button type="button" onclick={() => oncopy(item.id)}>
            <AppIcon name="copy" size={15} />
            {_t("card.copy")}
          </button>
          {#if (item.kind === "image" || item.kind === "file") && item.resourcePath}
            <button
              type="button"
              onclick={() => invoke("reveal_in_explorer", { path: item.resourcePath })}
            >
              <AppIcon name="file" size={15} />
              {_t("detail.locateFile")}
            </button>
            <button
              type="button"
              onclick={() => {
                const folder = item.resourcePath!.replace(/[^\\/]+$/, "");
                invoke("open_external_url", { url: folder });
              }}
            >
              <AppIcon name="download" size={15} />
              {_t("detail.openFolder")}
            </button>
          {/if}
          {#if !editing && (item.kind === "text" || item.kind === "link")}
            <button
              type="button"
              onclick={() => {
                editContent = item.textContent || item.title;
                editing = true;
              }}
            >
              <AppIcon name="edit" size={15} />
              {_t("edit.edit")}
            </button>
          {/if}
          {#if !editing && (item.kind === "image" || item.kind === "file") && (!item.fileMeta || item.fileMeta.length <= 1)}
            <button
              type="button"
              onclick={() => {
                editContent = item.title.split("\n")[0];
                editing = true;
              }}
            >
              <AppIcon name="edit" size={15} />
              {_t("edit.editFileName")}
            </button>
          {/if}
          {#if item.kind === "image" || item.kind === "file"}
            <button type="button" onclick={() => oncopyfilename(item.id)}>
              <AppIcon name="file" size={15} />
              {_t("copy.copyFileName")}
            </button>
          {:else}
            <button type="button" onclick={() => onplainpaste(item.id)}>
              <AppIcon name="type" size={15} />
              {_t("copy.plainText")}
            </button>
          {/if}
        </div>
      {:else if activeTab === "details"}
        <div class="details-section">
          <dl class="detail-list">
            <div class="detail-row">
              <dt><AppIcon name="info" size={14} /> {_t("detail.sourceApp")}</dt>
              <dd>{item.sourceApp}</dd>
            </div>
            <div class="detail-row">
              <dt><AppIcon name="file" size={14} /> {_t("detail.contentType")}</dt>
              <dd>{getKindLabel(item.kind)}</dd>
            </div>
            <div class="detail-row">
              <dt><AppIcon name="clock" size={14} /> {_t("detail.copyTime")}</dt>
              <dd>{formatDateTime(item.createdAt)}</dd>
            </div>
            <div class="detail-row">
              <dt><AppIcon name="ruler" size={14} /> {_t("detail.size")}</dt>
              <dd>{item.sizeLabel}</dd>
            </div>
            {#if item.detailLabel}
              <div class="detail-row">
                <dt><AppIcon name="image" size={14} /> {_t("detail.fileInfo")}</dt>
                <dd>{item.detailLabel}</dd>
              </div>
            {/if}
            {#if item.mimeType}
              <div class="detail-row">
                <dt><AppIcon name="mime" size={14} /> {_t("detail.mimeInfo")}</dt>
                <dd><code>{item.mimeType}</code></dd>
              </div>
            {/if}
            {#if resourceMetadata?.extension}
              <div class="detail-row">
                <dt><AppIcon name="file" size={14} /> {_t("detail.extension")}</dt>
                <dd><code>.{resourceMetadata.extension}</code></dd>
              </div>
            {/if}
            {#if item.fileName && (item.kind === "image" || item.kind === "file")}
              <div class="detail-row">
                <dt><AppIcon name="file" size={14} /> {_t("detail.fileName")}</dt>
                <dd>{item.fileName}</dd>
              </div>
            {/if}
            {#if item.ocrStatus && item.ocrStatus !== "none"}
              <div class="detail-row">
                <dt><AppIcon name="scan" size={14} /> {_t("detail.ocrStatus")}</dt>
                <dd
                  class="ocr-badge"
                  class:ocr-completed={item.ocrStatus === "completed"}
                  class:ocr-pending={item.ocrStatus === "pending" ||
                    item.ocrStatus === "processing"}
                  class:ocr-failed={item.ocrStatus === "failed"}
                >
                  {getOcrStatusLabel(item.ocrStatus)}
                </dd>
              </div>
            {/if}
            {#if item.kind === "image" && item.imageMeta}
              <div class="detail-row">
                <dt><AppIcon name="image" size={14} /> {_t("detail.dimensions")}</dt>
                <dd>{item.imageMeta.width} × {item.imageMeta.height}</dd>
              </div>
            {/if}
            {#if item.kind === "file" && resourceFiles.length > 0}
              <div class="detail-row">
                <dt><AppIcon name="file" size={14} /> {_t("detail.fileCount")}</dt>
                <dd>{resourceFiles.length}</dd>
              </div>
            {/if}
            {#if item.kind === "image" && (item.resourcePath || resourceMetadata?.resourcePath)}
              <div class="detail-row path-row">
                <dt><AppIcon name="file" size={14} /> {_t("detail.resourcePath")}</dt>
                <dd class="path-value">
                  <code>{resourceMetadata?.resourcePath ?? item.resourcePath}</code>
                </dd>
              </div>
            {/if}
            {#if item.kind === "image" && resourceMetadata?.storagePath && resourceMetadata.storagePath !== resourceMetadata.resourcePath}
              <div class="detail-row path-row">
                <dt><AppIcon name="file" size={14} /> {_t("detail.storagePath")}</dt>
                <dd class="path-value"><code>{resourceMetadata.storagePath}</code></dd>
              </div>
            {/if}
            {#if item.kind === "image" && resourceMetadata?.previewPath && resourceMetadata.previewPath !== resourceMetadata.resourcePath}
              <div class="detail-row path-row">
                <dt><AppIcon name="image" size={14} /> {_t("detail.previewPath")}</dt>
                <dd class="path-value"><code>{resourceMetadata.previewPath}</code></dd>
              </div>
            {/if}
            {#if item.kind === "image" && resourceMetadata?.originalPath}
              <div class="detail-row path-row">
                <dt><AppIcon name="file" size={14} /> {_t("detail.originalPath")}</dt>
                <dd class="path-value"><code>{resourceMetadata.originalPath}</code></dd>
              </div>
            {/if}
            {#if item.kind === "image" && resourceMetadata?.contentHash}
              <div class="detail-row">
                <dt><AppIcon name="info" size={14} /> {_t("detail.contentHash")}</dt>
                <dd class="path-value"><code>{resourceMetadata.contentHash}</code></dd>
              </div>
            {/if}
          </dl>

          {#if item.kind === "file" && resourceFiles.length > 0}
            <section class="resource-file-list">
              <strong class="resource-section-title">{_t("detail.resourceMetadata")}</strong>
              {#each resourceFiles as file, index}
                <article class="resource-file-card">
                  <div class="resource-file-heading">
                    <span class="resource-file-index">{index + 1}</span>
                    <strong>{file.name}</strong>
                    <span>{formatExactSize(file.sizeBytes)}</span>
                  </div>
                  <dl class="resource-file-details">
                    {#if file.mimeType}
                      <div>
                        <dt>{_t("detail.mimeInfo")}</dt>
                        <dd><code>{file.mimeType}</code></dd>
                      </div>
                    {/if}
                    {#if file.extension}
                      <div>
                        <dt>{_t("detail.extension")}</dt>
                        <dd><code>.{file.extension}</code></dd>
                      </div>
                    {/if}
                    {#if file.storagePath}
                      <div class="resource-path-detail">
                        <dt>{_t("detail.storagePath")}</dt>
                        <dd><code>{file.storagePath}</code></dd>
                      </div>
                    {/if}
                    {#if file.originalPath}
                      <div class="resource-path-detail">
                        <dt>{_t("detail.originalPath")}</dt>
                        <dd><code>{file.originalPath}</code></dd>
                      </div>
                    {/if}
                    {#if file.contentHash}
                      <div class="resource-path-detail">
                        <dt>{_t("detail.contentHash")}</dt>
                        <dd><code>{file.contentHash}</code></dd>
                      </div>
                    {/if}
                    <div>
                      <dt>{_t("detail.managedCopy")}</dt>
                      <dd>
                        {file.copied === undefined
                          ? _t("detail.unknown")
                          : file.copied
                            ? _t("detail.yes")
                            : _t("detail.no")}
                      </dd>
                    </div>
                    <div>
                      <dt>{_t("detail.createdTime")}</dt>
                      <dd>{formatMetadataTime(file.createdAtMs)}</dd>
                    </div>
                    <div>
                      <dt>{_t("detail.modifiedTime")}</dt>
                      <dd>{formatMetadataTime(file.modifiedAtMs)}</dd>
                    </div>
                    <div>
                      <dt>{_t("detail.accessedTime")}</dt>
                      <dd>{formatMetadataTime(file.accessedAtMs)}</dd>
                    </div>
                    <div>
                      <dt>{_t("detail.readOnly")}</dt>
                      <dd>
                        {file.readOnly === undefined
                          ? _t("detail.unknown")
                          : file.readOnly
                            ? _t("detail.yes")
                            : _t("detail.no")}
                      </dd>
                    </div>
                    <div>
                      <dt>{_t("detail.directory")}</dt>
                      <dd>
                        {file.isDirectory === undefined
                          ? _t("detail.unknown")
                          : file.isDirectory
                            ? _t("detail.yes")
                            : _t("detail.no")}
                      </dd>
                    </div>
                  </dl>
                </article>
              {/each}
            </section>
          {/if}

          {#if rawMetadata}
            <details class="raw-metadata">
              <summary>{_t("detail.rawMetadata")}</summary>
              <pre>{rawMetadata}</pre>
            </details>
          {/if}

          {#if hasSpecialMarkers}
            <div class="special-section">
              <strong class="special-title">{_t("detail.specialMarkers")}</strong>
              <div class="markers-list">
                {#each emails as email}
                  <div class="marker-item">
                    <AppIcon name="mail" size={13} />
                    <span>{email}</span>
                    <button type="button" onclick={() => copyText(email)}>
                      <AppIcon name="copy" size={11} />
                    </button>
                  </div>
                {/each}
                {#each urls as url}
                  <div class="marker-item">
                    <AppIcon name="globe" size={13} />
                    <a href={url} target="_blank" rel="noopener noreferrer">{url}</a>
                    <button type="button" onclick={() => copyText(url)}>
                      <AppIcon name="copy" size={11} />
                    </button>
                  </div>
                {/each}
                {#each phones as phone}
                  <div class="marker-item">
                    <AppIcon name="phone" size={13} />
                    <span>{phone}</span>
                    <button type="button" onclick={() => copyText(phone)}>
                      <AppIcon name="copy" size={11} />
                    </button>
                  </div>
                {/each}
                {#each colors as color}
                  <div class="marker-item color-marker">
                    <span class="color-swatch" style="background:{color}"></span>
                    <code>{color}</code>
                    <button type="button" onclick={() => copyText(color)}>
                      <AppIcon name="copy" size={11} />
                    </button>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {:else if activeTab === "ocr"}
        <div class="ocr-section">
          {#if item.kind === "image"}
            <div class="ocr-toolbar">
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
            </div>
            {#if ocrFeedback}
              <div class="ocr-feedback">{ocrFeedback}</div>
            {/if}
          {/if}
          {#if item.ocrStatus === "completed" && item.ocrText}
            <div class="ocr-status ocr-completed">
              <span class="ocr-dot"></span>
              {_t("detail.completed")}
              <button
                type="button"
                class="ocr-copy-btn"
                onclick={() => copyText(item.ocrText ?? "")}
              >
                {_t("detail.copyOcrText")}
              </button>
            </div>
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
      {/if}
    </div>
  </div>

  {#if imageFullscreen}
    <div
      class="image-viewer-overlay"
      style="background: rgba(0, 0, 0, {get(generalSettings).viewerBackdropOpacity / 100})"
      onwheel={onWheel}
      onmousedown={onMouseDown}
      onmousemove={onMouseMove}
      onmouseup={onMouseUp}
      onmouseleave={onMouseUp}
      ondblclick={onDblClick}
      role="presentation"
    >
      <button
        type="button"
        class="viewer-close-btn"
        onclick={closeImageFullscreen}
        aria-label={_t("actions.close")}
      >
        <AppIcon name="x" size={20} strokeWidth={2.5} />
      </button>
      <div class="viewer-zoom-hint">{Math.round(zoom * 100)}%</div>
      <img
        class="viewer-image"
        class:dragging={isDragging}
        src={assetUrl(item.previewPath || item.resourcePath)}
        alt={item.preview || item.title}
        draggable="false"
        style="transform: translate({panX}px, {panY}px) scale({zoom})"
        onload={() => console.log("[fullscreen] image loaded")}
        onerror={(e) => console.error("[fullscreen] image load error", e)}
      />
    </div>
  {/if}
{/if}

<style>
  .detail-backdrop {
    position: fixed;
    z-index: 51;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(3px);
  }

  .detail-backdrop.fullscreen-backdrop {
    display: none;
  }

  .detail-panel {
    position: fixed;
    z-index: 52;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(520px, 100vw);
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--border-color);
    background: var(--bg-settings);
    box-shadow: -8px 0 32px rgba(0, 0, 0, 0.5);
    animation: slide-in 220ms ease-out;
  }

  .detail-panel.fullscreen {
    display: none;
  }

  .detail-panel.inline {
    position: relative;
    z-index: auto;
    top: auto;
    right: auto;
    bottom: auto;
    width: 100%;
    border-left: none;
    box-shadow: none;
    animation: none;
    overflow-y: auto;
  }

  @keyframes slide-in {
    from {
      transform: translateX(100%);
    }
    to {
      transform: translateX(0);
    }
  }

  .detail-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .back-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 7px;
    color: var(--text-muted);
    background: var(--card-bg);
    cursor: pointer;
    transition:
      color 100ms ease,
      background 100ms ease;
  }

  .back-btn:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .header-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .header-kind {
    color: var(--text-muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .header-title {
    overflow: hidden;
    color: var(--text-primary);
    font-size: var(--font-size-base, 13px);
    font-weight: 540;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .header-edit-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    flex: 1;
  }

  .header-title-input {
    flex: 1;
    min-width: 0;
    padding: 2px 8px;
    border: 1px solid var(--selection-color);
    border-radius: 5px;
    color: var(--text-primary);
    background: var(--input-bg);
    font-size: 13px;
    outline: none;
  }

  .header-save-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-muted);
    background: var(--card-bg);
    cursor: pointer;
    flex-shrink: 0;
  }

  .header-save-btn:hover {
    color: var(--success-color);
    border-color: var(--success-color);
  }

  .detail-tabs {
    display: flex;
    gap: 2px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .detail-tabs button {
    padding: 7px 16px;
    border: 0;
    border-radius: 6px;
    color: var(--text-muted);
    background: transparent;
    font: inherit;
    font-size: var(--font-size-secondary, 12px);
    cursor: pointer;
    transition:
      color 100ms ease,
      background 100ms ease;
  }

  .detail-tabs button:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .detail-tabs button.active {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .detail-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px;
    scrollbar-width: thin;
    scrollbar-color: var(--scrollbar-color) transparent;
  }

  .preview-section {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .content-full {
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
    overflow-x: auto;
    max-height: 360px;
    overflow-y: auto;
  }

  .image-full-preview,
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

  .image-full-preview img {
    max-width: 100%;
    max-height: 400px;
    object-fit: contain;
    border-radius: 4px;
  }

  .image-placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .image-meta {
    color: var(--text-muted);
    font-size: 11px;
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

  .detail-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding-top: 8px;
  }

  .detail-actions button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border: 1px solid var(--border-color);
    border-radius: 7px;
    color: var(--text-secondary);
    background: var(--card-bg);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      background 100ms ease,
      border-color 100ms ease;
  }

  .detail-actions button:hover {
    background: var(--hover-bg);
    border-color: var(--text-faint);
    color: var(--text-primary);
  }

  .details-section {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .detail-list {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    overflow: hidden;
    background: var(--border-subtle);
  }

  .detail-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: var(--input-bg);
    gap: 12px;
  }

  .detail-row dt {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 500;
    flex-shrink: 0;
  }

  .detail-row dd {
    margin: 0;
    color: var(--text-primary);
    font-size: 11px;
    text-align: right;
    word-break: break-all;
  }

  .detail-row dd code {
    padding: 2px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: 10px;
  }

  .detail-row.path-row {
    align-items: flex-start;
  }

  .detail-row .path-value {
    min-width: 0;
    max-width: 68%;
    text-align: left;
  }

  .detail-row .path-value code {
    display: block;
    max-width: 100%;
    white-space: normal;
    overflow-wrap: anywhere;
    word-break: break-word;
  }

  .resource-file-list,
  .resource-section-title {
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 560;
  }

  .resource-file-card {
    padding: 10px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    background: var(--input-bg);
  }

  .resource-file-heading {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--text-primary);
    font-size: 11px;
  }

  .resource-file-heading strong {
    min-width: 0;
    flex: 1;
    overflow-wrap: anywhere;
  }

  .resource-file-heading > span:last-child {
    flex-shrink: 0;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  .resource-file-index {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-muted);
    background: var(--input-bg);
    font-size: 10px;
  }

  .resource-file-details {
    display: grid;
    gap: 1px;
    margin: 9px 0 0 26px;
    padding: 0;
  }

  .resource-file-details > div {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 4px 0;
    border-top: 1px solid var(--border-subtle);
  }

  .resource-file-details dt {
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 10px;
  }

  .resource-file-details dd {
    min-width: 0;
    margin: 0;
    color: var(--text-secondary);
    font-size: 10px;
    text-align: right;
    overflow-wrap: anywhere;
  }

  .resource-file-details dd code {
    color: var(--text-secondary);
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: 9.5px;
    overflow-wrap: anywhere;
    word-break: break-word;
  }

  .resource-file-details .resource-path-detail dd {
    max-width: 70%;
    text-align: left;
  }

  .raw-metadata pre {
    max-height: 360px;
    margin: 0;
    padding: 10px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font:
      11px/1.55 "Cascadia Code",
      Consolas,
      monospace;
    white-space: pre-wrap;
    overflow: auto;
    overflow-wrap: anywhere;
  }

  .raw-metadata {
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    background: var(--input-bg);
  }

  .raw-metadata summary {
    padding: 9px 12px;
    color: var(--text-muted);
    font-size: 10px;
    cursor: pointer;
  }

  .raw-metadata pre {
    max-height: 280px;
    margin: 0 8px 8px;
  }

  .ocr-badge {
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 10px;
  }

  .ocr-badge.ocr-completed {
    border: 1px solid color-mix(in srgb, var(--success-color) 40%, transparent);
    color: color-mix(in srgb, var(--success-color) 75%, white);
    background: color-mix(in srgb, var(--success-color) 12%, transparent);
  }

  .ocr-badge.ocr-pending {
    border: 1px solid color-mix(in srgb, var(--warning-color) 40%, transparent);
    color: color-mix(in srgb, var(--warning-color) 75%, white);
    background: color-mix(in srgb, var(--warning-color) 12%, transparent);
  }

  .ocr-badge.ocr-failed {
    border: 1px solid color-mix(in srgb, var(--danger-color) 40%, transparent);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, transparent);
  }

  .special-section {
    padding: 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    background: var(--input-bg);
  }

  .special-title {
    display: block;
    margin-bottom: 10px;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 560;
  }

  .markers-list {
    display: grid;
    gap: 6px;
  }

  .marker-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 11px;
    background: var(--input-bg);
  }

  .marker-item span,
  .marker-item a {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .marker-item a {
    color: var(--selection-color);
    text-decoration: none;
    white-space: normal;
    word-break: break-all;
  }

  .marker-item a:hover {
    text-decoration: underline;
  }

  .marker-item button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text-faint);
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    transition: color 100ms ease;
  }

  .marker-item button:hover {
    color: var(--text-secondary);
  }

  .color-swatch {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
    flex-shrink: 0;
  }

  .color-marker code {
    padding: 2px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font:
      10px "Cascadia Code",
      Consolas,
      monospace;
  }

  .ocr-section {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .ocr-toolbar {
    display: flex;
    justify-content: flex-end;
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
    margin-left: auto;
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

  @media (max-width: 520px) {
    .detail-panel {
      width: 100vw;
    }
  }

  .edit-area {
    position: relative;
    z-index: 4;
    padding: 4px;
  }

  .edit-area textarea {
    width: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    border: 1px solid var(--selection-color);
    border-radius: 7px;
    color: var(--text-primary);
    background: var(--input-bg);
    font:
      12px/1.55 "Cascadia Code",
      Consolas,
      monospace;
    resize: vertical;
    outline: none;
  }

  .edit-actions {
    display: flex;
    gap: 6px;
    margin-top: 8px;
    justify-content: flex-end;
  }

  .edit-actions button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 12px;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-muted);
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

  .image-full-preview {
    position: relative;
  }

  .image-fullscreen-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-secondary);
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(4px);
    cursor: pointer;
    opacity: 0;
    transition:
      opacity 150ms ease,
      background 150ms ease;
  }

  .image-full-preview:hover .image-fullscreen-btn {
    opacity: 1;
  }

  .image-fullscreen-btn:hover {
    color: var(--text-primary);
    background: rgba(0, 0, 0, 0.75);
  }

  .image-viewer-overlay {
    position: fixed;
    z-index: 100;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.92);
    cursor: grab;
    user-select: none;
  }

  .image-viewer-overlay:active {
    cursor: grabbing;
  }

  .viewer-image {
    max-width: 90vw;
    max-height: 90vh;
    object-fit: contain;
    transform-origin: center center;
    transition: transform 0.05s linear;
    pointer-events: none;
  }

  .viewer-image.dragging {
    transition: none;
  }

  .viewer-close-btn {
    position: fixed;
    top: 16px;
    right: 16px;
    z-index: 101;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    color: var(--text-secondary);
    background: rgba(30, 30, 30, 0.7);
    backdrop-filter: blur(6px);
    cursor: pointer;
    transition:
      color 120ms ease,
      background 120ms ease;
  }

  .viewer-close-btn:hover {
    color: var(--text-primary);
    background: rgba(60, 60, 60, 0.8);
  }

  .viewer-zoom-hint {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 101;
    padding: 4px 14px;
    border-radius: 6px;
    color: var(--text-secondary);
    background: rgba(30, 30, 30, 0.7);
    backdrop-filter: blur(6px);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
</style>
