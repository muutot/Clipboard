import { describe, expect, it } from "vitest";
import defaultsDocument from "../../keyboard-defaults.json";
import {
  GLOBAL_ACTION_IDS,
  HOTKEY_ACTIONS,
  actionsByCategory,
  isGlobalAction,
} from "./keyboard-registry";

const DEFAULT_IDS = new Set(
  Object.keys((defaultsDocument as { shortcuts: Record<string, string[]> }).shortcuts),
);

describe("keyboard-registry", () => {
  it("covers every canonical default and vice versa", () => {
    const registryIds = new Set(HOTKEY_ACTIONS.map((action) => action.id));
    for (const id of registryIds) {
      expect(
        DEFAULT_IDS.has(id),
        `registry action ${id} needs a keyboard-defaults.json entry`,
      ).toBe(true);
    }
    for (const id of DEFAULT_IDS) {
      expect(registryIds.has(id), `default action ${id} needs a registry row`).toBe(true);
    }
  });

  it("has no duplicate ids and valid scopes/categories", () => {
    const ids = HOTKEY_ACTIONS.map((action) => action.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const action of HOTKEY_ACTIONS) {
      expect(["global", "window"]).toContain(action.scope);
      expect(["item", "quick", "system", "switch"]).toContain(action.category);
    }
  });

  it("keeps the backend-parity global list in registry order", () => {
    // Must match `GLOBAL_HOTKEY_ACTIONS` in src-tauri/src/keyboard/actions.rs.
    expect(GLOBAL_ACTION_IDS).toEqual(["toggleWindow", "toggleFloatPanel"]);
    expect(isGlobalAction("toggleWindow")).toBe(true);
    expect(isGlobalAction("toggleFloatPanel")).toBe(true);
    expect(isGlobalAction("quickPaste")).toBe(false);
  });

  it("resolves categories used by the settings panel", () => {
    expect(actionsByCategory("quick").map((action) => action.id)).toEqual([
      "quickCopy1",
      "quickCopy2",
      "quickCopy3",
      "quickCopy4",
      "quickCopy5",
      "quickCopy6",
      "quickCopy7",
      "quickCopy8",
      "quickCopy9",
    ]);
    expect(actionsByCategory("system").map((action) => action.id)).toEqual([
      "toggleWindow",
      "toggleFloatPanel",
      "hideWindow",
      "focusSearch",
    ]);
  });
});
