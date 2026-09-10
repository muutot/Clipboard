import { describe, expect, it } from "vitest";
import { defaultShortcutsFor } from "../keyboard-defaults";
import {
  resolveActionBindings,
  resolveAllBindings,
  resolveFilterShortcutBindings,
  resolveGlobalBindings,
  resolveNavigationBindings,
} from "./shortcut-bindings";

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
      moveSelectionUp: ["Arrowup"],
      moveSelectionDown: ["Arrowdown"],
      switchFilterNext: ["Arrowright", "Tab"],
      switchFilterPrev: ["Arrowleft", "Shift+Tab"],
    });
  });

  it("honors overrides and per-action disables", () => {
    const bindings = resolveNavigationBindings({
      moveSelectionUp: ["K"],
      switchFilterNext: [],
    });
    expect(bindings.moveSelectionUp).toEqual(["K"]);
    expect(bindings.moveSelectionDown).toEqual(["Arrowdown"]);
    expect(bindings.switchFilterNext).toEqual([]);
    expect(bindings.switchFilterPrev).toEqual(["Arrowleft", "Shift+Tab"]);
  });
});

describe("resolveActionBindings", () => {
  it("falls back, honors overrides, and treats empty as disabled", () => {
    const fallback = defaultShortcutsFor("toggleFloatPanel");
    expect(resolveActionBindings({}, "toggleFloatPanel", fallback)).toEqual(["Alt+V"]);
    expect(
      resolveActionBindings({ toggleFloatPanel: ["Ctrl+F"] }, "toggleFloatPanel", fallback),
    ).toEqual(["Ctrl+F"]);
    expect(resolveActionBindings({ toggleFloatPanel: [] }, "toggleFloatPanel", fallback)).toEqual(
      [],
    );
  });
});

describe("resolveAllBindings", () => {
  it("covers every canonical default without configuration", () => {
    const bindings = resolveAllBindings({});
    expect(bindings.toggleWindow).toEqual(["Alt+C"]);
    expect(bindings.toggleFloatPanel).toEqual(["Alt+V"]);
    expect(bindings.copyItem).toEqual(["Ctrl+C", "Enter"]);
    expect(bindings.quickCopy9).toEqual(["Ctrl+9"]);
    expect(bindings.quickPaste).toEqual([]);
  });

  it("honors overrides and per-action disables", () => {
    const bindings = resolveAllBindings({ copyItem: ["F2"], focusSearch: [] });
    expect(bindings.copyItem).toEqual(["F2"]);
    expect(bindings.focusSearch).toEqual([]);
    expect(bindings.toggleWindow).toEqual(["Alt+C"]);
  });
});

describe("resolveGlobalBindings", () => {
  it("returns only the OS-global actions in registry order", () => {
    expect(Object.keys(resolveGlobalBindings({}))).toEqual(["toggleWindow", "toggleFloatPanel"]);
    expect(resolveGlobalBindings({ toggleWindow: [] }).toggleWindow).toEqual([]);
  });
});
