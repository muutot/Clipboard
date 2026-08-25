import { describe, expect, it } from "vitest";
import { resolveFilterShortcutBindings, resolveNavigationBindings } from "./shortcut-bindings";

describe("resolveFilterShortcutBindings", () => {
  it("falls back to Alt+position for absent actions", () => {
    const bindings = resolveFilterShortcutBindings({}, ["all", "text", "link"]);
    expect(bindings).toEqual({
      all: ["Alt+1"],
      text: ["Alt+2"],
      link: ["Alt+3"],
    });
  });

  it("uses configured bindings when the action key exists", () => {
    const shortcuts = { switchFilter1: ["Ctrl+K"], switchFilter3: ["F9"] };
    const bindings = resolveFilterShortcutBindings(shortcuts, ["all", "text", "link", "image"]);
    expect(bindings.all).toEqual(["Ctrl+K"]);
    expect(bindings.text).toEqual(["Alt+2"]); // untouched action keeps default
    expect(bindings.link).toEqual(["F9"]);
    expect(bindings.image).toEqual(["Alt+4"]);
  });

  it("disables a binding when configured to an empty array", () => {
    const shortcuts = { switchFilter2: [] };
    const bindings = resolveFilterShortcutBindings(shortcuts, ["all", "text"]);
    expect(bindings.all).toEqual(["Alt+1"]);
    expect(bindings.text).toEqual([]);
  });

  it("does not let a missing value key bypass the empty-disable rule", () => {
    // `switchFilter1` present but null-ish: treated as disabled, not default.
    const shortcuts = { switchFilter1: undefined as unknown as string[] };
    const bindings = resolveFilterShortcutBindings(shortcuts, ["all"]);
    expect(bindings.all).toEqual([]);
  });
});

describe("resolveNavigationBindings", () => {
  it("returns the canonical defaults without configuration", () => {
    expect(resolveNavigationBindings({})).toEqual({
      moveSelectionUp: ["ArrowUp"],
      moveSelectionDown: ["ArrowDown"],
      switchFilterNext: ["ArrowRight", "Tab"],
      switchFilterPrev: ["ArrowLeft", "Shift+Tab"],
    });
  });

  it("honors overrides and per-action disables", () => {
    const bindings = resolveNavigationBindings({
      moveSelectionUp: ["K"],
      switchFilterNext: [],
    });
    expect(bindings.moveSelectionUp).toEqual(["K"]);
    expect(bindings.moveSelectionDown).toEqual(["ArrowDown"]);
    expect(bindings.switchFilterNext).toEqual([]);
    expect(bindings.switchFilterPrev).toEqual(["ArrowLeft", "Shift+Tab"]);
  });
});
