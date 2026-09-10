import {
  isActivatableKeyboardTarget,
  isEditableKeyboardTarget,
  isItemActionShortcut,
  shortcutMatchesEvent,
} from "./keyboard";

/**
 * Pure keydown decision table extracted from the main route's global
 * handler. Given the event plus a plain-data snapshot of the UI state, it
 * returns the single action to perform — all DOM/store side effects stay in
 * the route, which keeps this module unit-testable without a component.
 *
 * Branch order mirrors the original handler exactly; even quirky guards
 * (e.g. item shortcuts compare a case-sensitive `event.key`) are preserved
 * so extraction cannot change behavior.
 */
export type KeyAction =
  | { type: "none"; prevent: boolean }
  | { type: "focus-search"; prevent: boolean }
  | { type: "quick-copy"; index: number; prevent: boolean }
  | { type: "set-filter"; filterId: string; focusTab: boolean; prevent: boolean }
  | { type: "move-selection"; delta: 1 | -1; prevent: boolean }
  | { type: "cycle-filter"; delta: 1 | -1; prevent: boolean }
  | { type: "activate-selected"; prevent: boolean }
  | { type: "open-detail"; id: string; prevent: boolean }
  | { type: "clear-selection"; prevent: boolean }
  | { type: "select-all"; prevent: boolean }
  | { type: "escape-hide-window"; prevent: boolean }
  | { type: "escape-toggle-tag"; tag: string; prevent: boolean }
  | { type: "bulk-copy"; prevent: boolean }
  | { type: "copy-item"; id: string; prevent: boolean }
  | { type: "bulk-delete"; prevent: boolean }
  | { type: "delete-item"; id: string; prevent: boolean }
  | { type: "bulk-favorite"; prevent: boolean }
  | { type: "toggle-favorite"; id: string; prevent: boolean }
  | { type: "tag-add"; prevent: boolean }
  | { type: "save-item"; id: string; prevent: boolean }
  | { type: "toggle-float"; prevent: boolean };

export interface KeyActionItem {
  id: string;
  favorite: boolean;
  kind: string;
}

export interface KeyActionFilter {
  id: string;
}

export interface KeyActionContext {
  hasEditing: boolean;
  hasFullscreen: boolean;
  hasTagDialog: boolean;
  hasDetail: boolean;
  /** Tag currently used as a list filter, if any. */
  tagFilter: string | null;
  isTauri: boolean;
  /** Focus is inside an editable element within the detail panel. */
  detailEditor: boolean;
  /** Focus is the search input (exempted from several editable guards). */
  isSearchInput: boolean;
  selectedId: string;
  selectedCount: number;
  filteredItems: KeyActionItem[];
  filters: KeyActionFilter[];
  activeFilter: string;
  filterShortcutBindings: Record<string, string[]>;
  moveSelectionDown: string[];
  moveSelectionUp: string[];
  switchFilterNext: string[];
  switchFilterPrev: string[];
  /** Bindings for the float-panel action (canonical default, empty disables). */
  toggleFloatBindings: string[];
}

function isTextInput(target: EventTarget | null): boolean {
  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
}

export function resolveKeyAction(event: KeyboardEvent, ctx: KeyActionContext): KeyAction {
  const editableTarget = isEditableKeyboardTarget(event.target);
  const quickCopyIndex =
    (event.metaKey || event.ctrlKey) && /^[1-9]$/.test(event.key) ? Number(event.key) - 1 : null;

  if (event.key === "Escape") {
    if (event.defaultPrevented || ctx.hasEditing || ctx.hasFullscreen || ctx.hasTagDialog) {
      return { type: "none", prevent: false };
    }
    if (ctx.detailEditor) return { type: "none", prevent: false };
    if (ctx.hasDetail) {
      // DetailPanel handles its own Escape.
      return { type: "none", prevent: false };
    }
    if (ctx.tagFilter) return { type: "escape-toggle-tag", tag: ctx.tagFilter, prevent: false };
    if (ctx.isTauri) return { type: "escape-hide-window", prevent: false };
    return { type: "none", prevent: false };
  }

  if (
    (event.key === "/" && !editableTarget) ||
    ((event.ctrlKey || event.metaKey) && event.key === "k")
  ) {
    return { type: "focus-search", prevent: true };
  }

  if (quickCopyIndex !== null && (!editableTarget || ctx.isSearchInput)) {
    return { type: "quick-copy", index: quickCopyIndex, prevent: true };
  }

  // Dedicated action bindings win over generic filter shortcuts below.
  if (ctx.toggleFloatBindings.some((binding) => shortcutMatchesEvent(binding, event))) {
    return { type: "toggle-float", prevent: true };
  }

  const switchEditableTarget = !editableTarget || ctx.isSearchInput;
  if (!ctx.hasEditing && switchEditableTarget) {
    for (const [filterId, bindings] of Object.entries(ctx.filterShortcutBindings)) {
      if (bindings.some((binding) => shortcutMatchesEvent(binding, event))) {
        return { type: "set-filter", filterId, focusTab: !editableTarget, prevent: true };
      }
    }
  }

  if (ctx.moveSelectionDown.some((binding) => shortcutMatchesEvent(binding, event))) {
    return { type: "move-selection", delta: 1, prevent: true };
  }

  if (ctx.moveSelectionUp.some((binding) => shortcutMatchesEvent(binding, event))) {
    return { type: "move-selection", delta: -1, prevent: true };
  }

  if (editableTarget && !isItemActionShortcut(event)) return { type: "none", prevent: false };

  if (ctx.switchFilterNext.some((binding) => shortcutMatchesEvent(binding, event))) {
    if (isTextInput(event.target)) return { type: "none", prevent: false };
    return { type: "cycle-filter", delta: 1, prevent: true };
  }

  if (ctx.switchFilterPrev.some((binding) => shortcutMatchesEvent(binding, event))) {
    if (isTextInput(event.target)) return { type: "none", prevent: false };
    return { type: "cycle-filter", delta: -1, prevent: true };
  }

  if (event.key === "Enter") {
    if (isTextInput(event.target)) return { type: "none", prevent: false };
    if (isActivatableKeyboardTarget(event.target)) return { type: "none", prevent: false };
    return { type: "activate-selected", prevent: true };
  }

  if (event.key === " ") {
    if (isTextInput(event.target)) return { type: "none", prevent: false };
    if (isActivatableKeyboardTarget(event.target)) return { type: "none", prevent: false };
    if (ctx.selectedId) return { type: "open-detail", id: ctx.selectedId, prevent: true };
    return { type: "none", prevent: true };
  }

  if (event.key === "Backspace") {
    return { type: "clear-selection", prevent: false };
  }

  if ((event.metaKey || event.ctrlKey) && event.key === "a") {
    if (isTextInput(event.target)) return { type: "none", prevent: false };
    return { type: "select-all", prevent: true };
  }

  if ((event.ctrlKey || event.metaKey) && !event.shiftKey) {
    let item = ctx.filteredItems.find((i) => i.id === ctx.selectedId);
    if (!item && ctx.filteredItems.length > 0) {
      // Heal a selection that no longer matches the active filter so the
      // shortcut operates on a visible entry instead of silently no-opping.
      item = ctx.filteredItems[0];
    }
    if (!item) return { type: "none", prevent: false };

    if (event.key === "c") {
      if (ctx.selectedCount > 0) return { type: "bulk-copy", prevent: true };
      return { type: "copy-item", id: item.id, prevent: true };
    }
    if (event.key === "d") {
      if (ctx.selectedCount > 0) return { type: "bulk-delete", prevent: true };
      if (!item.favorite) return { type: "delete-item", id: item.id, prevent: true };
      return { type: "none", prevent: false };
    }
    if (event.key === "f") {
      if (ctx.selectedCount > 0) return { type: "bulk-favorite", prevent: true };
      return { type: "toggle-favorite", id: item.id, prevent: true };
    }
    if (event.key === "e") {
      return { type: "open-detail", id: item.id, prevent: true };
    }
    if (event.key === "t") {
      if (ctx.selectedCount > 0) return { type: "none", prevent: false };
      return { type: "tag-add", prevent: true };
    }
    if (event.key === "s") {
      if (item.kind === "image" || item.kind === "file") {
        return { type: "save-item", id: item.id, prevent: true };
      }
      return { type: "none", prevent: false };
    }
  }

  return { type: "none", prevent: false };
}
