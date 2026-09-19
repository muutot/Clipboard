import { describe, expect, it } from "vitest";
import { defaultShortcutsFor } from "../keyboard-defaults";
import { resolveKeyAction, type KeyActionContext, type KeyActionItem } from "./keyboard-actions";

function keyEvent(
  init: Partial<KeyboardEvent> & { key: string; cancelable?: boolean },
): KeyboardEvent {
  return new KeyboardEvent("keydown", {
    key: init.key,
    ctrlKey: init.ctrlKey ?? false,
    altKey: init.altKey ?? false,
    shiftKey: init.shiftKey ?? false,
    metaKey: init.metaKey ?? false,
    cancelable: init.cancelable ?? false,
    bubbles: true,
  });
}

const ITEMS: KeyActionItem[] = [
  { id: "a", favorite: false, kind: "text" },
  { id: "b", favorite: true, kind: "image" },
];

function ctx(overrides: Partial<KeyActionContext> = {}): KeyActionContext {
  return {
    hasEditing: false,
    hasFullscreen: false,
    hasTagDialog: false,
    hasDetail: false,
    tagFilter: null,
    isTauri: false,
    detailEditor: false,
    isSearchInput: false,
    selectedId: "a",
    selectedCount: 0,
    filteredItems: ITEMS,
    filters: [{ id: "all" }, { id: "text" }],
    activeFilter: "all",
    filterShortcutBindings: { text: ["Alt+2"] },
    moveSelectionDown: ["ArrowDown"],
    moveSelectionUp: ["ArrowUp"],
    switchFilterNext: ["Alt+ArrowRight"],
    switchFilterPrev: ["Alt+ArrowLeft"],
    toggleFloatBindings: defaultShortcutsFor("toggleFloatPanel"),
    hideWindowBindings: defaultShortcutsFor("hideWindow"),
    quickPasteBindings: defaultShortcutsFor("quickPaste"),
    clearSelectionBindings: defaultShortcutsFor("clearSelection"),
    itemBindings: {
      copyItem: defaultShortcutsFor("copyItem"),
      deleteItem: defaultShortcutsFor("deleteItem"),
      favoriteItem: defaultShortcutsFor("favoriteItem"),
      addTag: defaultShortcutsFor("addTag"),
      openDetail: defaultShortcutsFor("openDetail"),
      downloadItem: defaultShortcutsFor("downloadItem"),
      selectAll: defaultShortcutsFor("selectAll"),
    },
    quickCopyBindings: Array.from({ length: 9 }, (_, index) =>
      defaultShortcutsFor(`quickCopy${index + 1}`),
    ),
    focusSearchBindings: defaultShortcutsFor("focusSearch"),
    ...overrides,
  };
}

function targetOn(tag: string, attrs: Record<string, string> = {}): HTMLElement {
  const el = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, value);
  document.body.appendChild(el);
  return el;
}

describe("resolveKeyAction — Escape", () => {
  it("yields none while something else owns Escape", () => {
    for (const override of [
      { hasEditing: true },
      { hasFullscreen: true },
      { hasTagDialog: true },
      { detailEditor: true },
      { hasDetail: true },
    ] as Partial<KeyActionContext>[]) {
      expect(resolveKeyAction(keyEvent({ key: "Escape" }), ctx(override))).toEqual({
        type: "none",
        prevent: false,
      });
    }
    const prevented = keyEvent({ key: "Escape", cancelable: true });
    prevented.preventDefault();
    expect(resolveKeyAction(prevented, ctx())).toEqual({ type: "none", prevent: false });
  });

  it("toggles the tag filter, hides the window on Tauri, else does nothing", () => {
    expect(resolveKeyAction(keyEvent({ key: "Escape" }), ctx({ tagFilter: "work" }))).toEqual({
      type: "escape-toggle-tag",
      tag: "work",
      prevent: false,
    });
    expect(resolveKeyAction(keyEvent({ key: "Escape" }), ctx({ isTauri: true }))).toEqual({
      type: "escape-hide-window",
      prevent: false,
    });
    expect(resolveKeyAction(keyEvent({ key: "Escape" }), ctx())).toEqual({
      type: "none",
      prevent: false,
    });
  });

  it("honors hideWindow rebinding and disabling", () => {
    // Clearing the binding disables Escape-hide.
    expect(
      resolveKeyAction(keyEvent({ key: "Escape" }), ctx({ isTauri: true, hideWindowBindings: [] }))
        .type,
    ).toBe("none");
    // A rebound chord fires instead; plain Escape no longer hides.
    expect(
      resolveKeyAction(
        keyEvent({ key: "h", ctrlKey: true }),
        ctx({ isTauri: true, hideWindowBindings: ["Ctrl+H"] }),
      ),
    ).toEqual({ type: "escape-hide-window", prevent: true });
    expect(
      resolveKeyAction(
        keyEvent({ key: "Escape" }),
        ctx({ isTauri: true, hideWindowBindings: ["Ctrl+H"] }),
      ).type,
    ).toBe("none");
    // Custom chords never fire outside Tauri (browser preview).
    expect(resolveKeyAction(keyEvent({ key: "h", ctrlKey: true }), ctx()).type).toBe("none");
  });
});

describe("resolveKeyAction — search and quick copy", () => {
  it("focuses search on / and Ctrl+K", () => {
    expect(resolveKeyAction(keyEvent({ key: "/" }), ctx()).type).toBe("focus-search");
    expect(resolveKeyAction(keyEvent({ key: "k", ctrlKey: true }), ctx()).type).toBe(
      "focus-search",
    );
  });

  it("maps Ctrl+digit to a quick-copy index", () => {
    expect(resolveKeyAction(keyEvent({ key: "1", ctrlKey: true }), ctx())).toEqual({
      type: "quick-copy",
      index: 0,
      prevent: true,
    });
  });

  it("toggles the float panel on its binding, winning over filter shortcuts", () => {
    expect(resolveKeyAction(keyEvent({ key: "v", altKey: true }), ctx())).toEqual({
      type: "toggle-float",
      prevent: true,
    });
    expect(
      resolveKeyAction(
        keyEvent({ key: "v", altKey: true }),
        ctx({ filterShortcutBindings: { all: ["Alt+V"] } }),
      ).type,
    ).toBe("toggle-float");
    expect(
      resolveKeyAction(keyEvent({ key: "v", altKey: true }), ctx({ toggleFloatBindings: [] })).type,
    ).toBe("none");
  });

  it("quick-pastes the selected item on its binding, unbound by default", () => {
    expect(resolveKeyAction(keyEvent({ key: "q", ctrlKey: true }), ctx()).type).toBe("none");
    expect(
      resolveKeyAction(
        keyEvent({ key: "q", ctrlKey: true }),
        ctx({ quickPasteBindings: ["Ctrl+Q"] }),
      ),
    ).toEqual({
      type: "quick-paste",
      id: "a",
      prevent: true,
    });
    // A selection that no longer matches the filter heals to the first
    // visible entry instead of silently no-opping.
    expect(
      resolveKeyAction(
        keyEvent({ key: "q", ctrlKey: true }),
        ctx({ quickPasteBindings: ["Ctrl+Q"], selectedId: "zzz" }),
      ),
    ).toEqual({
      type: "quick-paste",
      id: "a",
      prevent: true,
    });
    expect(
      resolveKeyAction(
        keyEvent({ key: "q", ctrlKey: true }),
        ctx({ quickPasteBindings: ["Ctrl+Q"], filteredItems: [] }),
      ).type,
    ).toBe("none");
  });
});

describe("resolveKeyAction — filters and movement", () => {
  it("resolves filter shortcuts with tab focus only outside editables", () => {
    expect(resolveKeyAction(keyEvent({ key: "2", altKey: true }), ctx())).toMatchObject({
      type: "set-filter",
      filterId: "text",
      focusTab: true,
    });
    const input = targetOn("input");
    const fromSearch = Object.defineProperty(keyEvent({ key: "2", altKey: true }), "target", {
      value: input,
    });
    expect(resolveKeyAction(fromSearch, ctx({ isSearchInput: true }))).toMatchObject({
      type: "set-filter",
      filterId: "text",
      focusTab: false,
    });
    input.remove();
  });

  it("resolves arrow movement", () => {
    expect(resolveKeyAction(keyEvent({ key: "ArrowDown" }), ctx())).toEqual({
      type: "move-selection",
      delta: 1,
      prevent: true,
    });
    expect(resolveKeyAction(keyEvent({ key: "ArrowUp" }), ctx())).toEqual({
      type: "move-selection",
      delta: -1,
      prevent: true,
    });
  });

  it("keeps arrow keys inside editable targets", () => {
    const input = targetOn("input");
    const withTarget = (e: KeyboardEvent) => Object.defineProperty(e, "target", { value: input });
    expect(resolveKeyAction(withTarget(keyEvent({ key: "ArrowDown" })), ctx()).type).toBe("none");
    expect(resolveKeyAction(withTarget(keyEvent({ key: "ArrowUp" })), ctx()).type).toBe("none");
    input.remove();
  });

  it("lets editables keep their keys except item-action shortcuts", () => {
    const input = targetOn("input");
    const withTarget = (e: KeyboardEvent) => Object.defineProperty(e, "target", { value: input });
    expect(resolveKeyAction(withTarget(keyEvent({ key: "z", ctrlKey: true })), ctx()).type).toBe(
      "none",
    );
    expect(resolveKeyAction(withTarget(keyEvent({ key: "c", ctrlKey: true })), ctx()).type).toBe(
      "copy-item",
    );
    input.remove();
  });

  it("cycles filters but not from text inputs", () => {
    expect(resolveKeyAction(keyEvent({ key: "ArrowRight", altKey: true }), ctx())).toMatchObject({
      type: "cycle-filter",
      delta: 1,
    });
    const input = targetOn("input");
    const event = Object.defineProperty(keyEvent({ key: "ArrowRight", altKey: true }), "target", {
      value: input,
    });
    expect(resolveKeyAction(event, ctx()).type).toBe("none");
    input.remove();
  });
});

describe("resolveKeyAction — Enter, Space, Backspace, select-all", () => {
  it("activates selection from plain surfaces only", () => {
    const div = targetOn("div");
    const onDiv = Object.defineProperty(keyEvent({ key: "Enter" }), "target", { value: div });
    expect(resolveKeyAction(onDiv, ctx()).type).toBe("activate-selected");
    const button = targetOn("button");
    const onButton = Object.defineProperty(keyEvent({ key: "Enter" }), "target", {
      value: button,
    });
    expect(resolveKeyAction(onButton, ctx()).type).toBe("none");
    const option = targetOn("div", { role: "option" });
    const onOption = Object.defineProperty(keyEvent({ key: "Enter" }), "target", {
      value: option,
    });
    expect(resolveKeyAction(onOption, ctx()).type).toBe("activate-selected");
    div.remove();
    button.remove();
    option.remove();
  });

  it("opens detail on Space, preventing default even without a selection", () => {
    expect(resolveKeyAction(keyEvent({ key: " " }), ctx())).toEqual({
      type: "open-detail",
      id: "a",
      prevent: true,
    });
    expect(resolveKeyAction(keyEvent({ key: " " }), ctx({ selectedId: "" }))).toEqual({
      type: "none",
      prevent: true,
    });
  });

  it("keeps Enter and Space inside contenteditable hosts (code editor)", () => {
    const editor = targetOn("div", { contenteditable: "true" });
    const onEditorEnter = Object.defineProperty(keyEvent({ key: "Enter" }), "target", {
      value: editor,
    });
    expect(resolveKeyAction(onEditorEnter, ctx())).toEqual({ type: "none", prevent: false });
    const onEditorSpace = Object.defineProperty(keyEvent({ key: " " }), "target", {
      value: editor,
    });
    expect(resolveKeyAction(onEditorSpace, ctx())).toEqual({ type: "none", prevent: false });
    editor.remove();
  });

  it("clears selection on Backspace and selects all on Ctrl+A", () => {
    expect(resolveKeyAction(keyEvent({ key: "Backspace" }), ctx()).type).toBe("clear-selection");
    expect(resolveKeyAction(keyEvent({ key: "a", ctrlKey: true }), ctx())).toEqual({
      type: "select-all",
      prevent: true,
    });
  });

  it("honors clearSelection rebinding and disabling from settings", () => {
    // Empty bindings disable the action instead of keeping Backspace active.
    expect(
      resolveKeyAction(keyEvent({ key: "Backspace" }), ctx({ clearSelectionBindings: [] })).type,
    ).toBe("none");
    // A rebound chord fires on the new key and leaves the old one native.
    expect(
      resolveKeyAction(
        keyEvent({ key: "x", altKey: true }),
        ctx({ clearSelectionBindings: ["Alt+X"] }),
      ).type,
    ).toBe("clear-selection");
    expect(
      resolveKeyAction(keyEvent({ key: "Backspace" }), ctx({ clearSelectionBindings: ["Alt+X"] }))
        .type,
    ).toBe("none");
  });
});

describe("resolveKeyAction — item shortcuts", () => {
  it("heals a stale selection to the first visible entry", () => {
    expect(
      resolveKeyAction(keyEvent({ key: "c", ctrlKey: true }), ctx({ selectedId: "gone" })),
    ).toEqual({ type: "copy-item", id: "a", prevent: true });
  });

  it("prefers bulk variants under multiselection", () => {
    const bulk = ctx({ selectedCount: 2 });
    expect(resolveKeyAction(keyEvent({ key: "c", ctrlKey: true }), bulk).type).toBe("bulk-copy");
    expect(resolveKeyAction(keyEvent({ key: "d", ctrlKey: true }), bulk).type).toBe("bulk-delete");
    expect(resolveKeyAction(keyEvent({ key: "f", ctrlKey: true }), bulk).type).toBe(
      "bulk-favorite",
    );
  });

  it("skips single delete on favorites and tag-add under multiselection", () => {
    expect(
      resolveKeyAction(keyEvent({ key: "d", ctrlKey: true }), ctx({ selectedId: "b" })).type,
    ).toBe("none");
    expect(
      resolveKeyAction(keyEvent({ key: "t", ctrlKey: true }), ctx({ selectedCount: 1 })).type,
    ).toBe("none");
    expect(resolveKeyAction(keyEvent({ key: "t", ctrlKey: true }), ctx()).type).toBe("tag-add");
  });

  it("saves only media kinds and ignores unknown letters", () => {
    expect(resolveKeyAction(keyEvent({ key: "s", ctrlKey: true }), ctx()).type).toBe("none");
    expect(
      resolveKeyAction(keyEvent({ key: "s", ctrlKey: true }), ctx({ selectedId: "b" })).type,
    ).toBe("save-item");
    expect(resolveKeyAction(keyEvent({ key: "x", ctrlKey: true }), ctx()).type).toBe("none");
    expect(
      resolveKeyAction(keyEvent({ key: "c", ctrlKey: true }), ctx({ filteredItems: [] })),
    ).toEqual({ type: "none", prevent: false });
  });

  it("matches single-letter chords case-insensitively (CapsLock Ctrl+C copies)", () => {
    // Canonical binding matching normalizes letter case, so CapsLock no
    // longer disables the shortcut. This intentionally differs from the old
    // case-sensitive `event.key` comparison.
    expect(resolveKeyAction(keyEvent({ key: "C", ctrlKey: true }), ctx()).type).toBe("copy-item");
  });

  it("honors custom item bindings and disabled actions", () => {
    const custom = ctx({
      itemBindings: {
        copyItem: ["F2"],
        deleteItem: [],
        favoriteItem: defaultShortcutsFor("favoriteItem"),
        addTag: defaultShortcutsFor("addTag"),
        openDetail: defaultShortcutsFor("openDetail"),
        downloadItem: defaultShortcutsFor("downloadItem"),
        selectAll: defaultShortcutsFor("selectAll"),
      },
    });
    expect(resolveKeyAction(keyEvent({ key: "F2" }), custom)).toEqual({
      type: "copy-item",
      id: "a",
      prevent: true,
    });
    // Disabled delete: Ctrl+D no longer fires.
    expect(resolveKeyAction(keyEvent({ key: "d", ctrlKey: true }), custom).type).toBe("none");
    // Untouched defaults keep working alongside the custom chord.
    expect(resolveKeyAction(keyEvent({ key: "f", ctrlKey: true }), custom).type).toBe(
      "toggle-favorite",
    );
  });

  it("honors custom quick-copy and focus-search bindings", () => {
    const custom = ctx({
      quickCopyBindings: [["F1"], [], [], [], [], [], [], [], []],
      focusSearchBindings: ["Ctrl+P"],
    });
    expect(resolveKeyAction(keyEvent({ key: "F1" }), custom)).toEqual({
      type: "quick-copy",
      index: 0,
      prevent: true,
    });
    expect(resolveKeyAction(keyEvent({ key: "p", ctrlKey: true }), custom).type).toBe(
      "focus-search",
    );
    // Rebound away from the defaults: Ctrl+2 and `/` stay silent.
    expect(resolveKeyAction(keyEvent({ key: "2", ctrlKey: true }), custom).type).toBe("none");
    expect(resolveKeyAction(keyEvent({ key: "/" }), custom).type).toBe("none");
  });
});
