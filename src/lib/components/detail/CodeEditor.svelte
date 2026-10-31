<script lang="ts">
  import { onMount } from "svelte";
  import EditableContextMenu from "$lib/components/common/EditableContextMenu.svelte";
  import { detectCodeLang, tokenizeCode } from "$lib/utils/content/code-tokenize";
  import { FILE_PREVIEW_LIMIT } from "$lib/utils/content/file-preview";

  interface Props {
    content: string;
    language?: string | null;
    editorLabel: string;
    placeholder?: string;
    /** Provided only in code mode — turns the language chip into a selector. */
    onlanguagechange?: (lang: string) => void;
    oncontentchange: (content: string) => void;
  }

  let {
    content,
    language = null,
    editorLabel,
    placeholder = "",
    onlanguagechange,
    oncontentchange,
  }: Props = $props();

  // Manual language overrides — ids are display names, mapped to highlight.js
  // grammars in code-tokenize.ts. "text"/"markdown" opt out of highlighting.
  const LANGUAGE_OPTIONS = [
    "JavaScript",
    "TypeScript",
    "Python",
    "Rust",
    "SQL",
    "HTML",
    "CSS",
    "JSON",
    "Shell",
    "YAML",
    "Go",
    "Java",
    "C",
    "C++",
    "C#",
    "markdown",
    "text",
  ];

  let editor = $state<HTMLElement | null>(null);
  let contextMenu = $state<{ x: number; y: number } | null>(null);
  let composing = $state(false);
  let lastEditorValue = "";

  /** 0-based line the caret sits on — drives the active-line highlight. */
  let caretLine = $state(0);
  /** Measured pixel top of each backdrop line — keeps the gutter aligned
   * even in wrap mode, where soft-wrapped rows shift line positions. */
  let lineTops = $state<number[]>([]);
  let wrap = $state(localStorage.getItem("clipboard.codeEditor.wrap") === "1");

  const LINE_HEIGHT_PX = 20.8; // 13px * 1.6

  function measureLineTops() {
    if (!editor) {
      lineTops = [];
      return;
    }
    const backdrop = editor.parentElement?.querySelector(".highlight-backdrop");
    if (!backdrop) {
      lineTops = [];
      return;
    }
    lineTops = Array.from(backdrop.querySelectorAll<HTMLElement>(".code-line")).map(
      (line) => line.offsetTop,
    );
  }

  function toggleWrap() {
    wrap = !wrap;
    try {
      localStorage.setItem("clipboard.codeEditor.wrap", wrap ? "1" : "0");
    } catch {
      // Storage unavailable — the toggle just stays session-scoped.
    }
  }

  function updateCaretLine() {
    if (!editor) return;
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || !editor.contains(selection.anchorNode)) return;
    const caret = selection.getRangeAt(0);
    const before = document.createRange();
    before.selectNodeContents(editor);
    before.setEnd(caret.endContainer, caret.endOffset);
    const offset = before.toString().length;
    caretLine = Math.min(
      content.slice(0, offset).split("\n").length - 1,
      Math.max(lineCount - 1, 0),
    );
  }

  function handleContextMenu(event: MouseEvent) {
    if (!editor) return;
    event.preventDefault();
    contextMenu = { x: event.clientX, y: event.clientY };
  }

  // A single trailing newline renders no extra line box in the editor (a
  // forced break at the very end creates no empty line), so the gutter must
  // not count it either — otherwise line 2 appears while the caret stays on
  // the end of line 1.
  const lineCount = $derived(Math.max(1, content.replace(/\n$/, "").split("\n").length));
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
      .map((line, index) => {
        const html = line
          .map((token) =>
            token.classes
              ? `<span class="${token.classes}">${escapeHtml(token.text)}</span>`
              : escapeHtml(token.text),
          )
          .join("");
        const active = index === caretLine ? " code-line-active" : "";
        // Empty lines get a zero-width space so the block keeps line height.
        return `<div class="code-line${active}">${line.length === 0 ? "\u200b" : html}</div>`;
      })
      .join("");
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

  $effect(() => {
    // Re-measure gutter line positions whenever the rendered lines change.
    // DOM reads below are plain (non-reactive); only these three states are
    // dependencies.
    content;
    wrap;
    highlightEnabled;
    queueMicrotask(measureLineTops);
  });

  onMount(() => {
    queueMicrotask(() => {
      if (!editor) return;
      editor.focus();
      moveCaretToEnd(editor);
      updateCaretLine();
      measureLineTops();
    });
    document.addEventListener("selectionchange", updateCaretLine);
    window.addEventListener("resize", measureLineTops);
    return () => {
      document.removeEventListener("selectionchange", updateCaretLine);
      window.removeEventListener("resize", measureLineTops);
    };
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

<div class="code-editor-shell" class:wrap>
  <div class="code-editor-toolbar">
    {#if onlanguagechange}
      <select
        class="language-select"
        value={language ?? ""}
        onchange={(event) => onlanguagechange(event.currentTarget.value)}
        aria-label={editorLabel}
      >
        <option value="">AUTO</option>
        {#each LANGUAGE_OPTIONS as option (option)}
          <option value={option}>{option}</option>
        {/each}
      </select>
    {:else}
      <span class="language-label">{languageLabel}</span>
    {/if}
    <button
      type="button"
      class="wrap-toggle"
      class:on={wrap}
      onclick={toggleWrap}
      title="Wrap"
      aria-pressed={wrap}
    >
      WRAP
    </button>
    <span class="editing-label">{editorLabel}</span>
  </div>

  <div class="code-source-pane">
    <div class="line-numbers" aria-hidden="true">
      {#each Array(lineCount) as _, index}
        <span style:top={`${lineTops[index] ?? index * LINE_HEIGHT_PX}px`}>{index + 1}</span>
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
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--code-bg);
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

  .editing-label {
    color: var(--text-faint);
    font-size: 10px;
  }

  .language-select {
    padding: 2px 4px;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font: inherit;
    font-size: 10px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .language-select:focus {
    outline: none;
    border-color: var(--selection-color);
  }

  .wrap-toggle {
    margin-left: auto;
    padding: 2px 6px;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-faint);
    background: var(--input-bg);
    font: inherit;
    font-size: 10px;
    letter-spacing: 0.05em;
    cursor: pointer;
  }

  .wrap-toggle.on {
    color: var(--selection-color);
    border-color: var(--selection-color);
  }

  /* Wrap mode: text soft-wraps inside the pane. The gutter stays visible —
     numbers are pixel-positioned from the backdrop's per-line offsets. */
  .code-editor-shell.wrap .code-input,
  .code-editor-shell.wrap .highlight-backdrop {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .code-editor-shell.wrap .editor-stack {
    width: 100%;
  }

  /* Active-line highlight lives in the {@html} backdrop — :global required. */
  .highlight-backdrop :global(.code-line-active) {
    background: color-mix(in srgb, var(--selection-color) 16%, transparent);
  }

  .code-source-pane {
    display: grid;
    flex: 1;
    grid-template-columns: auto minmax(0, 1fr);
    min-height: 16em;
    overflow: auto;
  }

  .line-numbers {
    position: relative;
    min-width: 38px;
    padding: 10px 0;
    border-right: 1px solid var(--border-subtle);
    color: var(--code-line-number);
    background: var(--code-gutter-bg);
    font:
      13px/1.6 "Cascadia Code",
      Consolas,
      "SFMono-Regular",
      monospace;
    text-align: right;
    user-select: none;
  }

  .line-numbers span {
    position: absolute;
    right: 0;
    height: 20.8px;
    line-height: 20.8px;
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
    color: var(--code-fg);
    font:
      13px/1.6 "Cascadia Code",
      Consolas,
      "SFMono-Regular",
      monospace;
    tab-size: 2;
    white-space: pre;
    pointer-events: none;
  }

  .code-input {
    min-height: 8em;
    margin: 0;
    padding: 10px 12px;
    outline: none;
    color: var(--code-fg);
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
    color: var(--code-fg);
  }

  /* IME composition text would be invisible over the backdrop — fall back
     to plain opaque text while composing. */
  .editor-stack.composing .highlight-backdrop {
    visibility: hidden;
  }

  .editor-stack.composing .code-input.highlighted {
    color: var(--code-fg);
  }

  .code-input:focus {
    outline: none;
  }

  .code-input:empty::before {
    color: var(--placeholder-color);
    content: attr(data-placeholder);
    pointer-events: none;
  }
</style>
