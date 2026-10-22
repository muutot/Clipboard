<script lang="ts">
  import { onMount } from "svelte";
  import ContextMenu from "$lib/components/common/ContextMenu.svelte";
  import type { ContextMenuItem } from "$lib/components/common/ContextMenu.svelte";
  import EditableContextMenu from "$lib/components/common/EditableContextMenu.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { writeClipboardText } from "$lib/services/clipboard";
  import { showToast } from "$lib/services/toast";
  import {
    notifyContextMenuOpened,
    onContextMenuOpened,
    trackContextMenuOpen,
  } from "$lib/services/context-menu";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  const EDITABLE_SELECTOR = "input, textarea, [contenteditable]:not([contenteditable='false'])";

  type OpenMenu =
    | { kind: "editable"; x: number; y: number; target: HTMLElement }
    | { kind: "selection"; x: number; y: number; text: string };

  let menu = $state<OpenMenu | null>(null);

  const selectionItems = $derived<ContextMenuItem[]>([
    { id: "copy", label: _t("edit.copy"), icon: "copy" },
  ]);

  function findEditable(target: EventTarget | null): HTMLElement | null {
    if (!(target instanceof Element)) return null;
    return target.closest<HTMLElement>(EDITABLE_SELECTOR);
  }

  // Owns every right-click surface so WebView2 never paints its own menu
  // (copy link / print / inspect) inside a desktop window. Cards, tag chips,
  // detail editors and float rows already prevent the default and open their
  // own menu, so those events arrive here already handled.
  function handleContextMenu(event: MouseEvent) {
    if (event.defaultPrevented) return;
    event.preventDefault();
    notifyContextMenuOpened();

    const editable = findEditable(event.target);
    if (editable) {
      menu = { kind: "editable", x: event.clientX, y: event.clientY, target: editable };
      return;
    }

    const selection = window.getSelection();
    const text = selection && !selection.isCollapsed ? selection.toString() : "";
    menu = text.trim() ? { kind: "selection", x: event.clientX, y: event.clientY, text } : null;
  }

  async function handleSelectionAction(id: string) {
    if (id !== "copy" || menu?.kind !== "selection") return;
    const text = menu.text;
    try {
      await writeClipboardText(text);
      showToast(_t("toast.copySuccess"), "success");
    } catch (error) {
      console.error("Unable to copy the selected text", error);
      showToast(_t("toast.copyFailed"), "error");
    }
  }

  $effect(() => {
    if (!menu) return;
    return trackContextMenuOpen();
  });

  $effect(() => onContextMenuOpened(() => (menu = null)));

  onMount(() => {
    window.addEventListener("contextmenu", handleContextMenu);
    return () => window.removeEventListener("contextmenu", handleContextMenu);
  });
</script>

{#if menu?.kind === "editable"}
  <EditableContextMenu x={menu.x} y={menu.y} target={menu.target} onclose={() => (menu = null)} />
{:else if menu?.kind === "selection"}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    items={selectionItems}
    onclose={() => (menu = null)}
    onaction={handleSelectionAction}
  />
{/if}
