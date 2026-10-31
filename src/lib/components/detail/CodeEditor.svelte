<script lang="ts">
  import { onMount } from "svelte";
  import CodePreview from "$lib/components/detail/CodePreview.svelte";
  import EditableContextMenu from "$lib/components/common/EditableContextMenu.svelte";
  import { detectCodeLang, tokenizeCode } from "$lib/utils/content/code-tokenize";
  import { FILE_PREVIEW_LIMIT } from "$lib/utils/content/file-preview";

  interface Props {
    content: string;
    language?: string | null;
    editorLabel: string;
    previewLabel: string;
    placeholder?: string;
    oncontentchange: (content: string) => void;
  }

  let {
    content,
    language = null,
    editorLabel,
    previewLabel,
    placeholder = "",
    oncontentchange,
  }: Props = $props();

  let editor = $state<HTMLElement | null>(null);
  let contextMenu = $state<{ x: number; y: number } | null>(null);
  let composing = $state(false);
  let lastEditorValue = "";

  function handleContextMenu(event: MouseEvent) {
    if (!editor) return;
    event.preventDefault();
    contextMenu = { x: event.clientX, y: event.clientY };
  }

  const lineCount = $derived(Math.max(1, content.split("\n").length));
  const languageLabel = $derived(language || "CODE");

  // Live highlight backdrop: the editor paints its own text transparently on
  // top of a token-colored mirror, so native caret, IME and undo stay intact.
  // Oversized content skips the backdrop — tokenizing every keystroke on a
  // multi-hundred-KB draft would lag the input.
  const highlightEnabled = $derived(content.length <= FILE_PREVIEW_LIMIT && content.length > 0);

  function escapeHtml(text: string): string {
    return text
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;");
  }

  const backdropHtml = $derived.by(() => {
    if (!highlightEnabled) return "";
    const lang = language || detectCodeLang(content);
    return tokenizeCode(content, lang)
      .map((line) =>
        line
          .map((token) =>
            token.classes
              ? `<span class="${token.classes}">${escapeHtml(token.text)}</span>`
              : escapeHtml(token.text),
          )
          .join(""),
      )
      .join("\n");
  });

  $effect(() => {
    const currentEditor = editor;
    const nextContent = content;
    if (!currentEditor || nextContent === lastEditorValue) return;

    if (readEditorContent(currentEditor) !== nextContent) {
      currentEditor.textContent = nextContent;
    }
    lastEditorValue = nextContent;
  });

  onMount(() => {
    queueMicrotask(() => {
      if (!editor) return;
      editor.focus();
      moveCaretToEnd(editor);
    });
  });

  function readEditorContent(element: HTMLElement): string {
    return (element.textContent ?? "").replace(/\r\n?/g, "\n");
  }

  function syncContent(element: HTMLElement) {
    const nextContent = readEditorContent(element);
    lastEditorValue = nextContent;
    oncontentchange(nextContent);
  }

  function handleInput(event: Event) {
    syncContent(event.currentTarget as HTMLElement);
  }

  function handleKeydown(event: KeyboardEvent) {
    // Never touch keys while an IME composition is active — Enter confirms
    // the composition and must reach the IME, not insert a literal newline.
    if (event.isComposing) return;

    if (event.key === "Tab") {
      event.preventDefault();
      insertTextAtSelection("  ");
      return;
    }

    if (event.key === "Enter" && event.shiftKey) {
      // Shift+Enter breaks at the end of the current line wherever the caret
      // sits mid-line — the same result as pressing Enter at the line end.
      event.preventDefault();
      insertLineBreakAtLineEnd();
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      insertTextAtSelection("\n");
    }
  }

  function insertLineBreakAtLineEnd() {
    if (!editor) return;

    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || !editor.contains(selection.anchorNode)) {
      insertTextAtSelection("\n");
      return;
    }

    const range = selection.getRangeAt(0);
    const before = document.createRange();
    before.selectNodeContents(editor);
    before.setEnd(range.startContainer, range.startOffset);
    const caretOffset = before.toString().length;

    const text = editor.textContent ?? "";
    const lineEnd = text.indexOf("\n", caretOffset);
    moveCaretToOffset(editor, lineEnd === -1 ? text.length : lineEnd);
    insertTextAtSelection("\n");
  }

  function moveCaretToOffset(element: HTMLElement, target: number) {
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    let remaining = target;
    let node: Text | null = null;
    while (walker.nextNode()) {
      const current = walker.currentNode as Text;
      if (remaining <= current.data.length) {
        node = current;
        break;
      }
      remaining -= current.data.length;
    }

    const selection = window.getSelection();
    if (!selection) return;

    const range = document.createRange();
    if (node) {
      range.setStart(node, remaining);
    } else {
      range.selectNodeContents(element);
      range.collapse(false);
    }
    range.collapse(true);
    selection.removeAllRanges();
    selection.addRange(range);
  }

  function handlePaste(event: ClipboardEvent) {
    const text = event.clipboardData?.getData("text/plain");
    if (text === undefined) return;

    event.preventDefault();
    insertTextAtSelection(text.replace(/\r\n?/g, "\n"));
  }

  function insertTextAtSelection(text: string) {
    if (!editor) return;

    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || !editor.contains(selection.anchorNode)) {
      editor.textContent = `${readEditorContent(editor)}${text}`;
      moveCaretToEnd(editor);
      syncContent(editor);
      return;
    }

    const range = selection.getRangeAt(0);
    range.deleteContents();

    const textNode = document.createTextNode(text);
    range.insertNode(textNode);
    range.setStart(textNode, textNode.length);
    range.collapse(true);
    selection.removeAllRanges();
    selection.addRange(range);
    syncContent(editor);
  }

  function moveCaretToEnd(element: HTMLElement) {
    const selection = window.getSelection();
    if (!selection) return;

    const range = document.createRange();
    range.selectNodeContents(element);
    range.collapse(false);
    selection.removeAllRanges();
    selection.addRange(range);
  }
</script>

<div class="code-editor-shell">
  <div class="code-editor-toolbar">
    <span class="language-label">{languageLabel}</span>
    <span class="editing-label">{editorLabel}</span>
  </div>

  <div class="code-source-pane">
    <div class="line-numbers" aria-hidden="true">
      {#each Array(lineCount) as _, index}
        <span>{index + 1}</span>
      {/each}
    </div>
    <div class="editor-stack" class:composing>
      {#if backdropHtml}
        <div class="highlight-backdrop" aria-hidden="true">
          {@html backdropHtml}
        </div>
      {/if}
      <div
        bind:this={editor}
        class="code-input"
        class:highlighted={!!backdropHtml}
        contenteditable="true"
        role="textbox"
        tabindex="0"
        aria-label={editorLabel}
        aria-multiline="true"
        spellcheck="false"
        data-placeholder={placeholder}
        oninput={handleInput}
        onkeydown={handleKeydown}
        onpaste={handlePaste}
        oncontextmenu={handleContextMenu}
        oncompositionstart={() => (composing = true)}
        oncompositionend={(event) => {
          composing = false;
          handleInput(event);
        }}
      ></div>
    </div>
  </div>

  <div class="live-preview">
    <div class="preview-label">{previewLabel}</div>
    <CodePreview {content} {language} />
  </div>
</div>

{#if contextMenu && editor}
  <EditableContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    target={editor}
    onclose={() => (contextMenu = null)}
  />
{/if}

<style>
  .code-editor-shell {
    overflow: hidden;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--input-bg);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--selection-color) 12%, transparent);
  }

  .code-editor-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 34px;
    padding: 0 10px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--statusbar-bg);
  }

  .language-label {
    padding: 3px 8px;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-muted);
    font-size: 10px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .editing-label,
  .preview-label {
    color: var(--text-faint);
    font-size: 10px;
  }

  .code-source-pane {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    max-height: 320px;
    overflow: auto;
  }

  .line-numbers {
    display: flex;
    flex-direction: column;
    min-width: 38px;
    padding: 10px 0;
    border-right: 1px solid var(--border-subtle);
    color: var(--text-faint);
    background: var(--statusbar-bg);
    font:
      13px/1.6 "Cascadia Code",
      Consolas,
      "SFMono-Regular",
      monospace;
    text-align: right;
    user-select: none;
  }

  .line-numbers span {
    padding: 0 9px 0 6px;
  }

  .editor-stack {
    position: relative;
    width: max-content;
    min-width: 100%;
  }

  /* Token-colored mirror underneath the editor. Geometry must match
     .code-input exactly (font, padding, white-space, tab-size) so the
     transparent editor glyphs align with the visible ones. */
  .highlight-backdrop {
    position: absolute;
    inset: 0;
    padding: 10px 12px;
    color: var(--text-primary);
    font:
      13px/1.6 "Cascadia Code",
      Consolas,
      "SFMono-Regular",
      monospace;
    tab-size: 2;
    white-space: pre;
    pointer-events: none;
  }

  /* Niche exception: syntax token hues are a fixed palette (content-defined
     rendering); color-mix with --text-primary shifts luminance with the
     theme so tokens stay legible on both dark and light surfaces. */
  .highlight-backdrop :global(.token-keyword) {
    color: color-mix(in srgb, #c586c0 75%, var(--text-primary));
  }

  .highlight-backdrop :global(.token-string) {
    color: color-mix(in srgb, #ce9178 75%, var(--text-primary));
  }

  .highlight-backdrop :global(.token-comment) {
    color: color-mix(in srgb, #6a9955 70%, var(--text-primary));
    font-style: italic;
  }

  .highlight-backdrop :global(.token-number) {
    color: color-mix(in srgb, #b5cea8 65%, var(--text-primary));
  }

  .highlight-backdrop :global(.token-key) {
    color: color-mix(in srgb, #9cdcfe 65%, var(--text-primary));
  }

  .code-input {
    min-height: 8em;
    margin: 0;
    padding: 10px 12px;
    outline: none;
    color: var(--text-primary);
    background: transparent;
    caret-color: var(--selection-color);
    font:
      13px/1.6 "Cascadia Code",
      Consolas,
      "SFMono-Regular",
      monospace;
    tab-size: 2;
    white-space: pre;
  }

  /* With the backdrop painting the glyphs, the editor text itself becomes
     transparent; selection restores the color so selected text stays
     readable above the selection background. */
  .code-input.highlighted {
    color: transparent;
  }

  .code-input.highlighted::selection {
    background: color-mix(in srgb, var(--selection-color) 40%, transparent);
    color: var(--text-primary);
  }

  /* IME composition text would be invisible over the backdrop — fall back
     to plain opaque text while composing. */
  .editor-stack.composing .highlight-backdrop {
    visibility: hidden;
  }

  .editor-stack.composing .code-input.highlighted {
    color: var(--text-primary);
  }

  .code-input:focus {
    background: color-mix(in srgb, var(--selection-color) 6%, var(--input-bg));
  }

  .code-input:empty::before {
    color: var(--placeholder-color);
    content: attr(data-placeholder);
    pointer-events: none;
  }

  .live-preview {
    padding: 9px 10px 10px;
    border-top: 1px solid var(--border-subtle);
    background: var(--statusbar-bg);
  }

  .preview-label {
    display: block;
    margin: 0 0 7px 2px;
  }

  :global(.live-preview .code-block) {
    border-color: var(--border-subtle);
    background: var(--input-bg);
  }
</style>
