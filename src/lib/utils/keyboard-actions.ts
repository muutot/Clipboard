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
 * Item actions, quick-copy slots, and focus-search match the user's
 * configured chords from conf/keyboard.json (passed in via the context, with
 * canonical defaults as fallback); branch order mirrors the original handler
 * so overlapping custom chords resolve deterministically. Single-letter
 * chords compare case-insensitively, so CapsLock no longer disables them
 * (this intentionally differs from the pre-extraction `event.key`
 * comparison).
 */
export type KeyAction =
  | { type: "none"; prevent: boolean }
  | { type: "focus-search"; prevent: boolean }
  | { type: "quick-copy"; index: number; prevent: boolean }
  | { type: "set-filter"; filterId: string; focusTab: boolean; prevent: boolean }
  | { type: "move-selection"; delta: 1 | -1; prevent: boolean }
  | { type: "cycle-filter"; delta: 1 | -1; prevent: boolean }
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
  | { type: "toggle-float"; prevent: boolean }
  | { type: "quick-paste"; id: string; prevent: boolean };

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
  /** Bindings for the hide-window action (canonical default Escape, empty
   * disables). Rebinding away from Escape fires on the custom chord; Escape
   * itself only hides while it is still one of the bound chords. */
  hideWindowBindings: string[];
  /** Bindings for the quick-paste action (unbound by default). */
  quickPasteBindings: string[];
  /** Bindings for the clear-selection action (canonical default Backspace,
   * empty disables). Previously Backspace was hardcoded here, so rebinding
   * or disabling clearSelection in settings silently did nothing. */
  clearSelectionBindings: string[];
  /** Per-item action chords from conf/keyboard.json (absent falls back to the
   * canonical defaults, explicitly empty disables). Previously these actions
   * were hardcoded below, so rebinding them in settings silently did nothing. */
  itemBindings: Record<
    | "copyItem"
    | "deleteItem"
    | "favoriteItem"
    | "addTag"
    | "openDetail"
    | "downloadItem"
    | "selectAll",
    string[]
  >;
  /** `quickCopy1..9` chords in position order (empty disables that slot). */
  quickCopyBindings: string[][];
  /** Focus-search chords (default `/` and Ctrl+K). */
  focusSearchBindings: string[];
}

function isTextInput(target: EventTarget | null): boolean {
  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
}

export function resolveKeyAction(event: KeyboardEvent, ctx: KeyActionContext): KeyAction {
  // IME composition: while composing (and on the synthetic keyCode-229
  // keydowns some engines emit), every key belongs to the IME. Cancelling a
  // composition with Escape must not also hide the window or fire any other
  // global shortcut.
  if (event.isComposing || event.keyCode === 229) {
    return { type: "none", prevent: false };
  }

  const editableTarget = isEditableKeyboardTarget(event.target);
  const matchesAny = (bindings: readonly string[]): boolean =>
    bindings.some((binding) => shortcutMatchesEvent(binding, event));

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
    // Escape hides the window only while it remains one of the bound
    // hideWindow chords; clearing the binding in settings disables it.
    if (ctx.isTauri && matchesAny(ctx.hideWindowBindings)) {
      return { type: "escape-hide-window", prevent: false };
    }
    return { type: "none", prevent: false };
  }

  if (
    matchesAny(ctx.focusSearchBindings) &&
    (!editableTarget || event.ctrlKey || event.metaKey || event.altKey)
  ) {
    // Default chords are `/` (outside editables) and Ctrl/⌘+K (everywhere);
    // a custom chord with non-typing modifiers (Ctrl/⌘/Alt) keeps the Ctrl+K
    // precedent, a bare one keeps the `/` precedent so typing is never
    // hijacked. Shift stays excluded: Shift+key types characters.
    return { type: "focus-search", prevent: true };
  }

  if (!editableTarget || ctx.isSearchInput) {
    const quickCopyIndex = ctx.quickCopyBindings.findIndex((bindings) =>
      bindings.some((binding) => shortcutMatchesEvent(binding, event)),
    );
    if (quickCopyIndex >= 0) {
      return { type: "quick-copy", index: quickCopyIndex, prevent: true };
    }
  }

  // Dedicated action bindings win over generic filter shortcuts below.
  if (ctx.toggleFloatBindings.some((binding) => shortcutMatchesEvent(binding, event))) {
    return { type: "toggle-float", prevent: true };
  }

  // Custom hide-window chords (e.g. Ctrl+H). Plain Escape is handled in the
  // dedicated Escape branch below so tag-filter and detail Escape semantics
  // keep their precedence.
  if (
    event.key !== "Escape" &&
    ctx.isTauri &&
    ctx.hideWindowBindings.some((binding) => shortcutMatchesEvent(binding, event))
  ) {
    return { type: "escape-hide-window", prevent: true };
  }

  // Quick paste targets the selected entry (healed to the first visible one
  // like the item shortcuts below), mirroring the copy/paste flows.
  if (ctx.quickPasteBindings.some((binding) => shortcutMatchesEvent(binding, event))) {
    let item = ctx.filteredItems.find((i) => i.id === ctx.selectedId);
    if (!item && ctx.filteredItems.length > 0) {
      item = ctx.filteredItems[0];
    }
    if (item) return { type: "quick-paste", id: item.id, prevent: true };
    return { type: "none", prevent: false };
  }

  const switchEditableTarget = !editableTarget || ctx.isSearchInput;
  if (!ctx.hasEditing && switchEditableTarget) {
    for (const [filterId, bindings] of Object.entries(ctx.filterShortcutBindings)) {
      if (bindings.some((binding) => shortcutMatchesEvent(binding, event))) {
        return { type: "set-filter", filterId, focusTab: !editableTarget, prevent: true };
      }
    }
  }

  // Editable targets keep their keys for typing: only item-action chords
  // punch through (Ctrl+A stays native so text selection keeps working).
  const punchThroughBindings = [
    ...ctx.itemBindings.copyItem,
    ...ctx.itemBindings.deleteItem,
    ...ctx.itemBindings.favoriteItem,
    ...ctx.itemBindings.addTag,
    ...ctx.itemBindings.openDetail,
    ...ctx.itemBindings.downloadItem,
  ];
  if (editableTarget && !isItemActionShortcut(event, punchThroughBindings)) {
    return { type: "none", prevent: false };
  }

  if (ctx.moveSelectionDown.some((binding) => shortcutMatchesEvent(binding, event))) {
    return { type: "move-selection", delta: 1, prevent: true };
  }

  if (ctx.moveSelectionUp.some((binding) => shortcutMatchesEvent(binding, event))) {
    return { type: "move-selection", delta: -1, prevent: true };
  }

  if (ctx.switchFilterNext.some((binding) => shortcutMatchesEvent(binding, event))) {
    if (isTextInput(event.target)) return { type: "none", prevent: false };
    return { type: "cycle-filter", delta: 1, prevent: true };
  }

  if (ctx.switchFilterPrev.some((binding) => shortcutMatchesEvent(binding, event))) {
    if (isTextInput(event.target)) return { type: "none", prevent: false };
    return { type: "cycle-filter", delta: -1, prevent: true };
  }

  if (event.key === "Enter") {
    // Editable surfaces (inputs, textareas, selects, contenteditable hosts
    // such as the detail code editor) keep Enter for themselves; only plain
    // surfaces and non-activatable cards activate the selection.
    if (editableTarget || isActivatableKeyboardTarget(event.target)) {
      return { type: "none", prevent: false };
    }
    // Enter activates through the copyItem binding (the default config binds
    // Enter to copyItem): removing that chord in settings now disables the
    // behavior instead of being silently ignored, and multi-selections take
    // the copyItem bulk semantics instead of copying only the anchor item.
    if (!matchesAny(ctx.itemBindings.copyItem)) {
      return { type: "none", prevent: false };
    }
    if (ctx.selectedCount > 0) return { type: "bulk-copy", prevent: true };
    if (!ctx.selectedId) return { type: "none", prevent: false };
    return { type: "copy-item", id: ctx.selectedId, prevent: true };
  }

  if (event.key === " ") {
    // Same editable guard as Enter: contenteditable hosts must keep Space
    // for typing instead of having it swallowed by open-detail.
    if (editableTarget || isActivatableKeyboardTarget(event.target)) {
      return { type: "none", prevent: false };
    }
    if (ctx.selectedId) return { type: "open-detail", id: ctx.selectedId, prevent: true };
    return { type: "none", prevent: true };
  }

  if (ctx.clearSelectionBindings.some((binding) => shortcutMatchesEvent(binding, event))) {
    return { type: "clear-selection", prevent: false };
  }

  if (matchesAny(ctx.itemBindings.selectAll)) {
    if (isTextInput(event.target)) return { type: "none", prevent: false };
    return { type: "select-all", prevent: true };
  }

  {
    let item = ctx.filteredItems.find((i) => i.id === ctx.selectedId);
    if (!item && ctx.filteredItems.length > 0) {
      // Heal a selection that no longer matches the active filter so the
      // shortcut operates on a visible entry instead of silently no-opping.
      item = ctx.filteredItems[0];
    }
    if (!item) return { type: "none", prevent: false };

    // Priority order mirrors the historical hardcoded branches: when the
    // user binds the same chord to two item actions, the first wins.
    if (matchesAny(ctx.itemBindings.copyItem)) {
      if (ctx.selectedCount > 0) return { type: "bulk-copy", prevent: true };
      return { type: "copy-item", id: item.id, prevent: true };
    }
    if (matchesAny(ctx.itemBindings.deleteItem)) {
      if (ctx.selectedCount > 0) return { type: "bulk-delete", prevent: true };
      if (!item.favorite) return { type: "delete-item", id: item.id, prevent: true };
      return { type: "none", prevent: false };
    }
    if (matchesAny(ctx.itemBindings.favoriteItem)) {
      if (ctx.selectedCount > 0) return { type: "bulk-favorite", prevent: true };
      return { type: "toggle-favorite", id: item.id, prevent: true };
    }
    if (matchesAny(ctx.itemBindings.openDetail)) {
      return { type: "open-detail", id: item.id, prevent: true };
    }
    if (matchesAny(ctx.itemBindings.addTag)) {
      if (ctx.selectedCount > 0) return { type: "none", prevent: false };
      return { type: "tag-add", prevent: true };
    }
    if (matchesAny(ctx.itemBindings.downloadItem)) {
      if (item.kind === "image" || item.kind === "file") {
        return { type: "save-item", id: item.id, prevent: true };
      }
      return { type: "none", prevent: false };
    }
  }

  return { type: "none", prevent: false };
}
