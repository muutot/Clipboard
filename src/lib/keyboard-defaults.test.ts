import { describe, expect, it } from "vitest";
import { defaultShortcutsFor } from "./keyboard-defaults";

describe("keyboard defaults (single source: keyboard-defaults.json)", () => {
  it("pins the toggle defaults", () => {
    expect(defaultShortcutsFor("toggleWindow")).toEqual(["Alt+C"]);
    expect(defaultShortcutsFor("toggleFloatPanel")).toEqual(["Alt+V"]);
  });

  it("pins the remaining canonical defaults", () => {
    expect(defaultShortcutsFor("copyItem")).toEqual(["Ctrl+C", "Enter"]);
    expect(defaultShortcutsFor("deleteItem")).toEqual(["Ctrl+D"]);
    expect(defaultShortcutsFor("favoriteItem")).toEqual(["Ctrl+F"]);
    expect(defaultShortcutsFor("addTag")).toEqual(["Ctrl+T"]);
    expect(defaultShortcutsFor("moveSelectionUp")).toEqual(["Arrowup"]);
    expect(defaultShortcutsFor("moveSelectionDown")).toEqual(["Arrowdown"]);
    expect(defaultShortcutsFor("switchFilterNext")).toEqual(["Tab"]);
    expect(defaultShortcutsFor("switchFilterPrev")).toEqual(["Shift+Tab"]);
    expect(defaultShortcutsFor("switchFilter1")).toEqual(["Alt+1"]);
    expect(defaultShortcutsFor("switchFilter7")).toEqual(["Alt+7"]);
    expect(defaultShortcutsFor("clearSelection")).toEqual(["Backspace"]);
    expect(defaultShortcutsFor("openDetail")).toEqual(["Space", "Ctrl+E"]);
    expect(defaultShortcutsFor("downloadItem")).toEqual(["Ctrl+S"]);
    expect(defaultShortcutsFor("selectAll")).toEqual(["Ctrl+A"]);
    expect(defaultShortcutsFor("quickPaste")).toEqual([]);
    expect(defaultShortcutsFor("quickCopy1")).toEqual(["Ctrl+1"]);
    expect(defaultShortcutsFor("quickCopy9")).toEqual(["Ctrl+9"]);
    expect(defaultShortcutsFor("hideWindow")).toEqual(["Escape"]);
    expect(defaultShortcutsFor("focusSearch")).toEqual(["/", "Ctrl+K"]);
  });

  it("returns copies and empty for unknown actions", () => {
    const first = defaultShortcutsFor("toggleWindow");
    first.push("X");
    expect(defaultShortcutsFor("toggleWindow")).toEqual(["Alt+C"]);
    expect(defaultShortcutsFor("noSuchAction")).toEqual([]);
  });
});
