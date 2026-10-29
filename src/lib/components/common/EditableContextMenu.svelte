<script lang="ts">
  import { untrack } from "svelte";
  import ContextMenu from "$lib/components/common/ContextMenu.svelte";
  import type { ContextMenuItem } from "$lib/components/common/ContextMenu.svelte";
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
    if (selection && selection.rangeCount > 0) {
      const range = selection.getRangeAt(0);
      if (
        initialTarget.contains(range.startContainer) &&
        initialTarget.contains(range.endContainer)
      )
        savedRange = range.cloneRange();
    }
  }

  function restoreSelection(element: HTMLElement) {
    element.focus();
    if (isField(element) || !savedRange) return;
    if (!element.contains(savedRange.startContainer) || !element.contains(savedRange.endContainer))
      return;
    const selection = window.getSelection();
    if (selection) {
      selection.removeAllRanges();
      selection.addRange(savedRange.cloneRange());
    }
  }

  /** Keep the action target alive independently of the parent menu's nullable props. */
  function captureEdit(element: HTMLElement) {
    const field = isField(element) ? element : null;
    const value = field ? field.value : element.innerHTML;
    const start = field?.selectionStart ?? value.length;
    const end = field?.selectionEnd ?? start;
    const selection = window.getSelection();
    const candidate = !field && selection?.rangeCount ? selection.getRangeAt(0) : null;
    const range =
      candidate &&
      element.contains(candidate.startContainer) &&
      element.contains(candidate.endContainer)
        ? candidate.cloneRange()
        : null;
    const selectedText = field ? value.slice(start, end) : (range?.toString() ?? "");

    function replace(text: string): boolean {
      if (!element.isConnected || !element.contains(document.activeElement)) return false;
      if (field) {
        if (field.disabled || field.readOnly || field.value !== value) return false;
        if (
          (field.selectionStart ?? value.length) !== start ||
          (field.selectionEnd ?? start) !== end
        )
          return false;
        replaceFieldSelection(field, text);
      } else {
        if (element.innerHTML !== value || element.getAttribute("contenteditable") === "false")
          return false;
        if (
          !range ||
          !element.contains(range.startContainer) ||
          !element.contains(range.endContainer)
        )
          return false;
        const current = window.getSelection();
        const live = current?.rangeCount ? current.getRangeAt(0) : null;
        if (
          !live ||
          live.startContainer !== range.startContainer ||
          live.startOffset !== range.startOffset ||
          live.endContainer !== range.endContainer ||
          live.endOffset !== range.endOffset
        )
          return false;
        if (text) document.execCommand("insertText", false, text);
        else {
          range.deleteContents();
          range.collapse(true);
          current!.removeAllRanges();
          current!.addRange(range);
          element.dispatchEvent(new Event("input", { bubbles: true }));
        }
      }
      return true;
    }
    return { selectedText, replace };
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
    const element = target;
    restoreSelection(element);

    if (id === "selectAll") {
      if (isField(element)) {
        element.select();
      } else {
        const range = document.createRange();
        range.selectNodeContents(element);
        const selection = window.getSelection();
        if (selection) {
          selection.removeAllRanges();
          selection.addRange(range);
        }
      }
      return;
    }

    const edit = captureEdit(element);
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
      edit.replace(text);
      return;
    }

    const text = edit.selectedText;
    if (!text) return;
    try {
      await writeClipboardText(text);
    } catch (error) {
      console.error("Unable to copy the selected text", error);
      showToast(_t("toast.copyFailed"), "error");
      return;
    }
    const cut = id === "cut" && edit.replace("");
    showToast(cut ? _t("toast.cutSuccess") : _t("toast.copySuccess"), "success");
  }
</script>

<ContextMenu {x} {y} {items} {onclose} onaction={handleAction} />
