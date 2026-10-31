<script lang="ts">
  import { tick, untrack } from "svelte";
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import type { IconName } from "$lib/types/clipboard";
  import CustomSelect from "$lib/components/common/CustomSelect.svelte";
  import DetailEditActions from "$lib/components/detail/DetailEditActions.svelte";
  import DetailOcrTab from "$lib/components/detail/DetailOcrTab.svelte";
  import DetailFilePreview from "$lib/components/detail/DetailFilePreview.svelte";
  import DetailDetailsTab from "$lib/components/detail/DetailDetailsTab.svelte";
  import DetailTagsTab from "$lib/components/detail/DetailTagsTab.svelte";
  import DetailImagePreview from "$lib/components/detail/DetailImagePreview.svelte";
  import CodeEditor from "$lib/components/detail/CodeEditor.svelte";
  import CodePreview from "$lib/components/detail/CodePreview.svelte";
  import MarkdownPreview from "$lib/components/detail/MarkdownPreview.svelte";
  import type { ClipboardItem, OcrTextBlock } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { isEditableKeyboardTarget } from "$lib/utils/keyboard/keyboard";
  import {
    captureFocusRestore,
    getFocusableElements,
    trapTabFocus,
  } from "$lib/utils/keyboard/focus";
  import { formatRelativeTime } from "$lib/utils/content/time";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { getDisplayTitle } from "$lib/utils/content/display-text";
  import FindBar from "$lib/components/detail/FindBar.svelte";
  import { setCurrentMatch, unwrapMatches, wrapMatches } from "$lib/utils/content/find-highlight";

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
    searchQuery?: string;
    /** Edge-triggered Ctrl+F signal from the main route (monotonic seq). */
    findRequest?: { seq: number } | null;
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
    searchQuery = "",
    findRequest = null,
  }: Props = $props();

  let activeTab = $state<"preview" | "details" | "tags" | "ocr">("preview");
  let editing = $state(false);
  let editingTitle = $state(false);
  let editContent = $state("");
  // Manual language override for the code editor; null = auto-detect.
  let editLanguage = $state<string | null>(null);
  let editSession = 0;

  function beginContentEdit() {
    if (!item) return;
    editSession += 1;
    editLanguage = null;
    closeFind();
    editContent =
      item.kind === "image" || item.kind === "file"
        ? getDisplayTitle(item.title)
        : item.textContent || item.title;
    editing = true;
  }

  // --- Find in detail (Ctrl+F) ---
  // Highlight marks live only inside this panel's preview DOM; the main list
  // behind the panel is never touched. Only the text-like previews (plain
  // text, code, markdown) participate — images already highlight OCR blocks
  // via the main search query and files have no text preview here.
  let findOpen = $state(false);
  let findQuery = $state("");
  let findCount = $state(0);
  let findCurrent = $state(0);
  let previewEl = $state<HTMLElement | null>(null);
  let findBarRef = $state<{ focusInput: () => void } | null>(null);
  let lastFindSeq = 0;

  const findSupported = $derived(item?.kind === "text" || item?.kind === "link");

  function closeFind() {
    if (previewEl) unwrapMatches(previewEl);
    findCount = 0;
    findCurrent = 0;
    findOpen = false;
  }

  // Consume the route's Ctrl+F signal — see the effect below, which must run
  // AFTER the item-switch reset effect (on first mount both fire, and the
  // reset would otherwise close the bar the signal just opened).

  // Apply/refresh marks. Order matters: unwrap first, then re-wrap after the
  // DOM settles — the preview content may re-render for the same query
  // (item patch, tab switch). `findSupported`/`currentItemId` are `$derived`
  // values, so OCR polling patches (which replace the item object without
  // changing id/kind) never rebuild the marks.
  $effect(() => {
    if (!findOpen || editing || activeTab !== "preview" || !findSupported) {
      return;
    }
    const container = previewEl;
    const query = findQuery;
    const itemId = currentItemId;
    if (!container || itemId === null) return;
    unwrapMatches(container);
    void tick().then(() => {
      if (!findOpen || !previewEl || previewEl !== container) return;
      findCount = wrapMatches(container, query);
      if (findCount === 0) {
        findCurrent = 0;
      } else {
        findCurrent = Math.min(findCurrent, findCount - 1);
        setCurrentMatch(container, findCurrent);
      }
    });
  });

  function handleFindQueryChange(next: string) {
    findQuery = next;
    findCurrent = 0;
  }

  function handleFindNext() {
    if (findCount === 0) return;
    findCurrent = (findCurrent + 1) % findCount;
    if (previewEl) setCurrentMatch(previewEl, findCurrent);
  }

  function handleFindPrev() {
    if (findCount === 0) return;
    findCurrent = (findCurrent - 1 + findCount) % findCount;
    if (previewEl) setCurrentMatch(previewEl, findCurrent);
  }

  function endContentEdit() {
    editSession += 1;
    editing = false;
    editLanguage = null;
  }
  let editTitleContent = $state("");

  // Depend only on the item id so OCR polling patches (which replace the
  // item object every poll) don't flip the user back to the preview tab or
  // discard an in-progress edit for the same item.
  const currentItemId = $derived(item?.id ?? null);

  $effect(() => {
    const itemId = currentItemId;
    if (itemId !== null) {
      activeTab = "preview";
      endContentEdit();
      // The find draft belongs to the previous item's preview; marks would
      // point at stale text after the switch.
      closeFind();
      // The title rename draft belongs to the previous item. Keeping it
      // open across an item switch would save the old title onto the new
      // item when the pending input commits (blur/Enter).
      editingTitle = false;
      editTitleContent = "";
    }
  });

  // Consume the route's Ctrl+F signal: open (or refocus) the find bar only
  // when a text-like preview is actually shown. Consumed via untrack so the
  // effect never re-fires from findQuery/open state changes. This effect is
  // deliberately created AFTER the reset effect above: on first mount both
  // fire in creation order, and the reset's closeFind() would otherwise close
  // the bar this signal just opened.
  $effect(() => {
    const request = findRequest;
    if (!request || request.seq === lastFindSeq) return;
    lastFindSeq = request.seq;
    untrack(() => {
      if (!item || editing || activeTab !== "preview" || !findSupported) return;
      findOpen = true;
      tick().then(() => findBarRef?.focusInput());
    });
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
  const polledOcrStatus = $derived(item?.ocrStatus);

  $effect(() => {
    const itemId = polledItemId;
    const itemKind = polledItemKind;
    const status = polledOcrStatus;
    if (itemKind !== "image" || !itemId || !isTauriRuntime()) return;
    if (status === "completed" || status === "failed" || status === "none") return;

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
        blocks: OcrTextBlock[];
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
              ocrBlocks: result.blocks ?? [],
            });
          } else {
            onocrupdate(itemId, {
              ocrStatus: "none",
              ocrText: undefined,
              ocrBlocks: undefined,
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
    // Ctrl+S / Cmd+S saves the in-progress content edit. Works while focus is
    // inside the code editor; blocks the browser/webview save dialog even
    // when the draft is empty and the save itself is a no-op.
    if (
      editing &&
      !event.isComposing &&
      !event.altKey &&
      !event.shiftKey &&
      (event.ctrlKey || event.metaKey) &&
      event.key.toLowerCase() === "s" &&
      !event.defaultPrevented
    ) {
      event.preventDefault();
      saveEdit();
      return;
    }

    if (!item || event.key !== "Escape" || event.defaultPrevented) return;

    // An open find bar owns the first Escape, before the editor guard (the
    // find input would otherwise hit editorTarget and be misread as an
    // in-progress content edit). preventDefault also stops the route's
    // Escape chain (bulk-clear / hide-window) behind this handler.
    if (findOpen) {
      event.preventDefault();
      closeFind();
      return;
    }

    const editorTarget =
      isEditableKeyboardTarget(event.target) &&
      event.target instanceof Element &&
      event.target.closest(".detail-panel") !== null;
    if (editorTarget) {
      event.preventDefault();
      endContentEdit();
      editingTitle = false;
      return;
    }

    if (editing) {
      event.preventDefault();
      endContentEdit();
      return;
    }

    event.preventDefault();
    onclose();
  }

  async function saveEdit() {
    if (!item || !editContent.trim()) return;
    const request = { id: item.id, content: editContent, session: editSession };
    const saved = await onsaveedit(request.id, request.content.trim());
    if (
      saved !== false &&
      request.id === item?.id &&
      request.session === editSession &&
      request.content === editContent
    )
      endContentEdit();
  }

  function saveTitleEdit() {
    if (!item || !editTitleContent.trim()) {
      editingTitle = false;
      return;
    }
    onrenametitle(item.id, editTitleContent.trim());
    editingTitle = false;
  }

  function autofocus(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function saveAsNew() {
    if (!item || !editContent.trim()) return;
    onsaveasnew(item.id, item.title, editContent.trim());
    endContentEdit();
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
              use:autofocus
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
      {#if item.kind === "image"}
        <button
          class:active={activeTab === "ocr"}
          type="button"
          onclick={() => (activeTab = "ocr")}
        >
          {_t("detail.ocr")}
        </button>
      {/if}
    </nav>

    <div class="detail-body">
      {#if activeTab === "preview"}
        <div class="preview-section" class:edit-fill={editing} bind:this={previewEl}>
          {#if findOpen && findSupported && !editing}
            <FindBar
              bind:this={findBarRef}
              query={findQuery}
              count={findCount}
              current={findCurrent}
              placeholder={_t("detail.findPlaceholder")}
              label={_t("detail.findLabel")}
              prevTitle={_t("detail.findPrev")}
              nextTitle={_t("detail.findNext")}
              closeTitle={_t("detail.findClose")}
              onquerychange={handleFindQueryChange}
              onprev={handleFindPrev}
              onnext={handleFindNext}
              onclose={closeFind}
            />
          {/if}
          {#if item.kind === "image"}
            <DetailImagePreview {item} {onimagefullscreen} query={searchQuery} />
          {:else if item.kind === "file"}
            <DetailFilePreview {item} />
          {:else if isCode && !isMarkdown}
            {#if editing}
              <CodeEditor
                content={editContent}
                language={editLanguage ?? detectCodeLanguage(editContent)}
                editorLabel={_t("edit.edit")}
                placeholder={_t("edit.placeholder")}
                onlanguagechange={(lang) => (editLanguage = lang || null)}
                oncontentchange={(content) => (editContent = content)}
              />
              <DetailEditActions
                onsave={saveEdit}
                onsaveasnew={saveAsNew}
                oncancel={endContentEdit}
              />
            {:else}
              <CodePreview content={detailContent} />
            {/if}
          {:else if isMarkdown}
            {#if editing}
              <CodeEditor
                content={editContent}
                language="markdown"
                editorLabel={_t("edit.edit")}
                placeholder={_t("edit.placeholder")}
                oncontentchange={(content) => (editContent = content)}
              />
              <DetailEditActions
                onsave={saveEdit}
                onsaveasnew={saveAsNew}
                oncancel={endContentEdit}
              />
            {:else}
              <MarkdownPreview content={detailContent} />
            {/if}
          {:else}
            {#if editing}
              <CodeEditor
                content={editContent}
                language="text"
                editorLabel={_t("edit.edit")}
                placeholder={_t("edit.placeholder")}
                oncontentchange={(content) => (editContent = content)}
              />
              <DetailEditActions
                onsave={saveEdit}
                onsaveasnew={saveAsNew}
                oncancel={endContentEdit}
              />
            {:else}
              <pre class="content-full">{item.textContent || item.title}</pre>
            {/if}
          {/if}
        </div>

        {#if !editing}
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
                  invoke("reveal_in_explorer", { path: folder }).catch(() => {});
                }}
              >
                <AppIcon name="download" size={15} />
                {_t("detail.openFolder")}
              </button>
            {/if}
            {#if !editing && (item.kind === "text" || item.kind === "link")}
              <button type="button" onclick={beginContentEdit}>
                <AppIcon name="edit" size={15} />
                {_t("edit.edit")}
              </button>
            {/if}
            {#if !editing && (item.kind === "image" || item.kind === "file") && (!item.fileMeta || item.fileMeta.length <= 1)}
              <button
                type="button"
                onclick={() => {
                  editTitleContent = item.title;
                  editingTitle = true;
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
        {/if}
      {:else if activeTab === "details"}
        <DetailDetailsTab {item} kindLabel={getKindLabel(item.kind)} />
      {:else if activeTab === "tags"}
        <DetailTagsTab {item} {tagColors} {onsavetags} />
      {:else if activeTab === "ocr" && item.kind === "image"}
        <DetailOcrTab {item} {onocrupdate} query={searchQuery} />
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
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  /* While editing, the editor fills the whole body edge-to-edge: negative
     margins cancel the body padding, and the actions row re-adds its own
     spacing so the editor itself spans the full width and height. */
  .preview-section.edit-fill {
    height: calc(100% + 32px);
    margin: -16px;
    padding: 0 0 12px;
  }

  .preview-section.edit-fill :global(.edit-actions) {
    margin: 12px 16px 0;
  }

  /* Find-in-detail marks are inserted at runtime (DOM walker), so they must
   * be styled via :global. Same warning-color mix as the OCR match boxes. */
  .preview-section :global(mark[data-find-match]) {
    background: color-mix(in srgb, var(--warning-color) 24%, transparent);
    color: inherit;
  }

  .preview-section :global(mark[data-find-match].find-current) {
    border: 1px solid var(--warning-color);
    border-radius: 2px;
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
</style>
