<script lang="ts">
  import { onMount } from "svelte";
  import CodePreview from "$lib/components/CodePreview.svelte";

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
  let lastEditorValue = "";

  const lineCount = $derived(Math.max(1, content.split("\n").length));
  const languageLabel = $derived(language || "CODE");

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
    if (event.key === "Tab") {
      event.preventDefault();
      insertTextAtSelection("  ");
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      insertTextAtSelection("\n");
    }
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
    <div
      bind:this={editor}
      class="code-input"
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
    ></div>
  </div>

  <div class="live-preview">
    <div class="preview-label">{previewLabel}</div>
    <CodePreview {content} {language} />
  </div>
</div>

<style>
  .code-editor-shell {
    overflow: hidden;
    border: 1px solid #3a3a3a;
    border-radius: 8px;
    background: #141414;
    box-shadow: 0 0 0 1px rgba(74, 168, 255, 0.12);
  }

  .code-editor-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 34px;
    padding: 0 10px;
    border-bottom: 1px solid #2e2e2e;
    background: #181818;
  }

  .language-label {
    padding: 3px 8px;
    border: 1px solid #3a3a3a;
    border-radius: 5px;
    color: #999;
    font-size: 10px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .editing-label,
  .preview-label {
    color: #777;
    font-size: 10px;
  }

  .code-source-pane {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    max-height: 320px;
    overflow: auto;
    scrollbar-color: #696969 transparent;
    scrollbar-width: thin;
  }

  .line-numbers {
    display: flex;
    flex-direction: column;
    min-width: 38px;
    padding: 10px 0;
    border-right: 1px solid #292929;
    color: #555;
    background: #121212;
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

  .code-input {
    min-height: 8em;
    margin: 0;
    padding: 10px 12px;
    outline: none;
    color: #d4d4d4;
    background: #141414;
    caret-color: #8fc7ff;
    font:
      13px/1.6 "Cascadia Code",
      Consolas,
      "SFMono-Regular",
      monospace;
    tab-size: 2;
    white-space: pre;
  }

  .code-input:focus {
    background: #16191d;
  }

  .code-input:empty::before {
    color: #555;
    content: attr(data-placeholder);
    pointer-events: none;
  }

  .live-preview {
    padding: 9px 10px 10px;
    border-top: 1px solid #2e2e2e;
    background: #111;
  }

  .preview-label {
    display: block;
    margin: 0 0 7px 2px;
  }

  :global(.live-preview .code-block) {
    border-color: #292929;
    background: #121212;
  }
</style>
