<script lang="ts">
  import { tick, untrack } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { IconName } from "$lib/types/clipboard";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import DetailEditActions from "$lib/components/DetailEditActions.svelte";
  import DetailOcrTab from "$lib/components/DetailOcrTab.svelte";
  import DetailFilePreview from "$lib/components/DetailFilePreview.svelte";
  import DetailDetailsTab from "$lib/components/DetailDetailsTab.svelte";
  import DetailTagsTab from "$lib/components/DetailTagsTab.svelte";
  import DetailImagePreview from "$lib/components/DetailImagePreview.svelte";
  import CodeEditor from "$lib/components/CodeEditor.svelte";
  import CodePreview from "$lib/components/CodePreview.svelte";
  import MarkdownPreview from "$lib/components/MarkdownPreview.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { isEditableKeyboardTarget } from "$lib/utils/keyboard";
  import { captureFocusRestore, getFocusableElements, trapTabFocus } from "$lib/utils/focus";
  import { formatRelativeTime } from "$lib/utils/time";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { getDisplayTitle } from "$lib/services/clipboard";

  const MARKDOWN_RE = /^#{1,6}\s|^>\s|^-\s|^\*\*|^\`\`\`|^\[.+\]\(.+\)/m;
  const CODE_PATTERNS: [RegExp, string][] = [
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
    [new RegExp("^<!DOCTYPE|<html|<head|<body|<div|<span|\\.class\\s*\\{|#id\\s*\\{", "m"), "HTML"],
    [
      new RegExp(
        "^SELECT\\s|^INSERT\\s|^UPDATE\\s|^DELETE\\s|^CREATE\\s|^\\s*FROM\\s|^\\s*WHERE\\s",
        "mi",
      ),
      "SQL",
    ],
    [new RegExp("^#!/|^\\s*(echo|export|cd|ls|grep|mkdir|sudo|apt|npm|yarn|git)\\s", "m"), "Shell"],
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

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    item: ClipboardItem | null;
    mode?: "overlay" | "split";
    onclose: () => void;
    oncopy: (id: string) => void;
    onedit: (id: string) => void;
    onsaveedit: (id: string, content: string) => void | Promise<boolean>;
    onrenametitle: (id: string, title: string) => void;
    onplainpaste: (id: string) => void;
    onformatpaste: (id: string) => void;
    oncleanpaste: (id: string) => void;
    onduplicate: (id: string) => void;
    onsaveasnew: (id: string, title: string, content: string) => void;
    oncopyfilename: (id: string) => void;
    oncopyPath: (id: string) => void;
    onimagefullscreen?: (id: string) => void;
    onsavetags: (id: string, tags: string[]) => void;
    onocrupdate: (id: string, patch: Partial<ClipboardItem>) => void;
    tagColors?: Record<string, string>;
  }

  let {
    item,
    mode = "overlay",
    onclose,
    oncopy,
    onedit,
    onsaveedit,
    onrenametitle,
    onplainpaste,
    onformatpaste,
    oncleanpaste,
    onduplicate,
    onsaveasnew,
    oncopyfilename,
    oncopyPath,
    onimagefullscreen,
    onsavetags,
    onocrupdate,
    tagColors = {},
  }: Props = $props();

  let activeTab = $state<"preview" | "details" | "tags" | "ocr">("preview");
  let editing = $state(false);
  let editingTitle = $state(false);
  let editContent = $state("");
  let editTitleContent = $state("");

  // Depend only on the item id so OCR polling patches (which replace the
  // item object every poll) don't flip the user back to the preview tab or
  // discard an in-progress edit for the same item.
  const currentItemId = $derived(item?.id ?? null);

  $effect(() => {
    const itemId = currentItemId;
    if (itemId !== null) {
      activeTab = "preview";
      editing = false;
      // The title rename draft belongs to the previous item. Keeping it
      // open across an item switch would save the old title onto the new
      // item when the pending input commits (blur/Enter).
      editingTitle = false;
      editTitleContent = "";
    }
  });

  // Modal focus management (only in overlay mode; split mode has no
  // backdrop and must not trap focus). Depend only on the derived boolean
  // so item identity changes (e.g. OCR polling patches) don't steal focus.
  let panelEl = $state<HTMLElement | null>(null);
  const overlayOpen = $derived(item !== null && mode !== "split");

  $effect(() => {
    if (!overlayOpen) return;
    const restoreFocus = captureFocusRestore();
    tick().then(() => {
      if (panelEl) getFocusableElements(panelEl)[0]?.focus();
    });
    return restoreFocus;
  });

  // Depend only on the item id/kind values, through `$derived`: reading the
  // `item` prop directly registers a reference-granular dependency, and every
  // OCR patch replaces the object (even when the status is unchanged), which
  // re-ran the polling effect, reset `requestInFlight`, and fired an immediate
  // poll — an IPC-speed busy poll. A `$derived` only invalidates downstream
  // when the value itself changes.
  const polledItemId = $derived(item?.id ?? null);
  const polledItemKind = $derived(item?.kind ?? null);

  $effect(() => {
    const itemId = polledItemId;
    const itemKind = polledItemKind;
    if (itemKind !== "image" || !itemId || !isTauriRuntime()) return;

    let disposed = false;
    let requestInFlight = false;
    // Cleared once the record reaches a terminal OCR state; without this
    // the interval stayed alive (waking every 2s to early-return) until the
    // panel closed or a different item was selected.
    let interval: ReturnType<typeof setInterval> | undefined;
    const poll = () => {
      if (disposed || requestInFlight) return;
      // The staleness check reads `item` and `ocrStatus`; keep those reads out
      // of the effect's dependency set (the first synchronous `poll()` below
      // runs inside the tracked phase).
      const stale = untrack(() => {
        const current = item;
        return (
          !current ||
          current.id !== itemId ||
          current.ocrStatus === "completed" ||
          current.ocrStatus === "failed" ||
          current.ocrStatus === "none"
        );
      });
      if (stale) {
        if (interval !== undefined) {
          clearInterval(interval);
          interval = undefined;
        }
        return;
      }

      requestInFlight = true;
      invoke<{
        fullText: string;
        status: "pending" | "processing" | "completed" | "failed";
        errorMessage: string | null;
      } | null>("get_clipboard_item_ocr", { id: itemId })
        .then((result) => {
          if (disposed) return;
          if (result) {
            onocrupdate(itemId, {
              ocrStatus: result.status,
              ocrError: result.errorMessage ?? undefined,
              ocrText: result.fullText || undefined,
            });
          } else {
            onocrupdate(itemId, {
              ocrStatus: "none",
              ocrText: undefined,
              ocrError: undefined,
            });
          }
        })
        .catch(() => {
          if (disposed) return;
          onocrupdate(itemId, { ocrStatus: "failed", ocrError: _t("detail.ocrReadFailed") });
        })
        .finally(() => {
          requestInFlight = false;
        });
    };

    poll();
    interval = setInterval(poll, 2000);
    return () => {
      disposed = true;
      if (interval !== undefined) clearInterval(interval);
    };
  });

  const detailContent = $derived(item ? item.textContent || item.title : "");
  const isCode = $derived(item ? detectCodeLanguage(detailContent) !== null : false);
  const isMarkdown = $derived(item ? MARKDOWN_RE.test(detailContent) : false);

  function detectCodeLanguage(text: string): string | null {
    for (const [regex, lang] of CODE_PATTERNS) {
      if (regex.test(text)) return lang;
    }
    return null;
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

  function handleKeydown(event: KeyboardEvent) {
    if (!item || event.key !== "Escape" || event.defaultPrevented) return;

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
  {#if mode !== "split"}
    <div class="detail-backdrop" onclick={onclose} aria-hidden="true"></div>
  {/if}
  <div
    class="detail-panel"
    class:inline={mode === "split"}
    bind:this={panelEl}
    onkeydowncapture={(event) => {
      if (mode !== "split") trapTabFocus(panelEl, event);
    }}
    role="dialog"
    aria-modal={mode !== "split"}
    aria-label={_t("detail.title")}
  >
    <div class="detail-header" data-tauri-drag-region>
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
              {getDisplayTitle(item.title)}
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
      <button
        class:active={activeTab === "tags"}
        type="button"
        onclick={() => (activeTab = "tags")}
      >
        {_t("detail.tags")}
      </button>
      <button class:active={activeTab === "ocr"} type="button" onclick={() => (activeTab = "ocr")}>
        {_t("detail.ocr")}
      </button>
    </nav>

    <div class="detail-body">
      {#if activeTab === "preview"}
        <div class="preview-section">
          {#if item.kind === "image"}
            <DetailImagePreview {item} {onimagefullscreen} />
          {:else if item.kind === "file"}
            <DetailFilePreview {item} />
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
              <DetailEditActions
                onsave={saveEdit}
                onsaveasnew={saveAsNew}
                oncancel={() => (editing = false)}
              />
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
              <DetailEditActions
                onsave={saveEdit}
                onsaveasnew={saveAsNew}
                oncancel={() => (editing = false)}
              />
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
              <DetailEditActions
                onsave={saveEdit}
                onsaveasnew={saveAsNew}
                oncancel={() => (editing = false)}
              />
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
          {#if item.kind === "image" || item.kind === "file"}
            <button type="button" onclick={() => oncopyPath(item.id)}>
              <AppIcon name="link" size={15} />
              {_t("card.copyPath")}
            </button>
          {/if}
          {#if (item.kind === "image" || item.kind === "file") && item.resourcePath}
            <button
              type="button"
              onclick={() => {
                invoke("reveal_in_explorer", { path: item.resourcePath }).catch(() => {});
              }}
            >
              <AppIcon name="file" size={15} />
              {_t("detail.locateFile")}
            </button>
            <button
              type="button"
              onclick={() => {
                const folder = item.resourcePath!.replace(/[^\\/]+$/, "");
                invoke("open_external_url", { url: folder }).catch(() => {});
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
                editContent = getDisplayTitle(item.title);
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
            {#if item.htmlContent}
              <button type="button" onclick={() => onformatpaste(item.id)}>
                <AppIcon name="clipboard" size={15} />
                {_t("card.pasteFormat")}
              </button>
            {/if}
            <button type="button" onclick={() => oncleanpaste(item.id)}>
              <AppIcon name="scan" size={15} />
              {_t("card.cleanPaste")}
            </button>
          {/if}
        </div>
      {:else if activeTab === "details"}
        <DetailDetailsTab {item} kindLabel={getKindLabel(item.kind)} />
      {:else if activeTab === "tags"}
        <DetailTagsTab {item} {tagColors} {onsavetags} />
      {:else if activeTab === "ocr"}
        <DetailOcrTab {item} {onocrupdate} />
      {/if}
    </div>
  </div>
{/if}

<style>
  .detail-backdrop {
    position: fixed;
    z-index: 51;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(3px);
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

  .detail-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding-top: 8px;
  }

  .detail-actions button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 8px 14px;
    border: 1px solid var(--border-color);
    border-radius: 7px;
    color: var(--text-secondary);
    background: var(--card-bg);
    font: inherit;
    font-size: 11.5px;
    line-height: 1;
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
</style>
