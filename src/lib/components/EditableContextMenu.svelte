<script lang="ts">
  import { untrack } from "svelte";
  import ContextMenu from "$lib/components/ContextMenu.svelte";
  import type { ContextMenuItem } from "$lib/components/ContextMenu.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { readClipboardText, writeClipboardText } from "$lib/services/clipboard";
  import { showToast } from "$lib/services/toast";

  interface Props {
    x: number;
    y: number;
    /** The focused editable (`<textarea>`/`<input>`/contenteditable) to act on. */
    target: HTMLElement;
    onclose: () => void;
  }

  let { x, y, target, onclose }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  const items: ContextMenuItem[] = [
    { id: "cut", label: _t("edit.cut"), icon: "scissors" },
    { id: "copy", label: _t("edit.copy"), icon: "copy" },
    { id: "paste", label: _t("edit.paste"), icon: "clipboard" },
    { id: "selectAll", label: _t("edit.selectAll"), icon: "check" },
  ];

  function isField(el: HTMLElement): el is HTMLTextAreaElement | HTMLInputElement {
    return el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement;
  }

  // A contenteditable drops its DOM selection when the menu button steals
  // focus, so snapshot the range now and restore it before the action runs.
  // Text fields keep `selectionStart/End` across blur, so they need nothing.
  let savedRange: Range | null = null;
  const initialTarget = untrack(() => target);
  if (!isField(initialTarget)) {
    const selection = window.getSelection();
    if (selection && selection.rangeCount > 0 && !selection.isCollapsed) {
      savedRange = selection.getRangeAt(0).cloneRange();
    }
  }

  function restoreSelection() {
    target.focus();
    if (isField(target) || !savedRange) return;
    const selection = window.getSelection();
    if (selection) {
      selection.removeAllRanges();
      selection.addRange(savedRange.cloneRange());
    }
  }

  function replaceFieldSelection(
    field: HTMLTextAreaElement | HTMLInputElement,
    text: string,
  ): void {
    const start = field.selectionStart ?? field.value.length;
    const end = field.selectionEnd ?? start;
    field.setRangeText(text, start, end, "end");
    // `setRangeText` does not emit `input`; dispatch it so Svelte's bind:value
    // (and the contenteditable `oninput` handlers) pick up the change.
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }

  async function handleAction(id: string) {
    restoreSelection();

    if (id === "selectAll") {
      if (isField(target)) {
        target.select();
      } else {
        const range = document.createRange();
        range.selectNodeContents(target);
        const selection = window.getSelection();
        if (selection) {
          selection.removeAllRanges();
          selection.addRange(range);
        }
      }
      return;
    }

    if (id === "paste") {
      let text = "";
      try {
        text = await readClipboardText();
      } catch (error) {
        console.error("Unable to read the clipboard", error);
        showToast(_t("toast.pasteFailed"), "error");
        return;
      }
      if (!text) return;
      if (isField(target)) replaceFieldSelection(target, text);
      else document.execCommand("insertText", false, text);
      return;
    }

    const text = isField(target)
      ? target.value.slice(target.selectionStart ?? 0, target.selectionEnd ?? 0)
      : (savedRange?.toString() ?? "");
    if (!text) return;
    try {
      await writeClipboardText(text);
    } catch (error) {
      console.error("Unable to copy the selected text", error);
      showToast(_t("toast.copyFailed"), "error");
      return;
    }
    if (id === "cut") {
      if (isField(target)) {
        replaceFieldSelection(target, "");
      } else if (savedRange) {
        savedRange.deleteContents();
        savedRange.collapse(true);
        target.dispatchEvent(new Event("input", { bubbles: true }));
      }
    }
    showToast(_t("toast.copySuccess"), "success");
  }
</script>

<ContextMenu {x} {y} {items} {onclose} onaction={handleAction} />
