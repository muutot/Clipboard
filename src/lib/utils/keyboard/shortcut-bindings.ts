// Shortcut binding resolution extracted from the main route. Maps the
// persisted conf/keyboard.json actions onto runtime handlers: an absent
// action falls back to its canonical default (single source:
// `keyboard-defaults.json` at the repo root), and an action explicitly
// configured to empty disables that binding (the settings panel exposes
// the same keys).

import { defaultShortcutsFor } from "$lib/keyboard-defaults";
import { HOTKEY_ACTIONS } from "$lib/keyboard-registry";

export type NavigationAction =
  "moveSelectionUp" | "moveSelectionDown" | "switchFilterNext" | "switchFilterPrev";

const NAVIGATION_DEFAULTS: Record<NavigationAction, string[]> = {
  moveSelectionUp: defaultShortcutsFor("moveSelectionUp"),
  moveSelectionDown: defaultShortcutsFor("moveSelectionDown"),
  switchFilterNext: defaultShortcutsFor("switchFilterNext"),
  switchFilterPrev: defaultShortcutsFor("switchFilterPrev"),
};

function readBinding(
  shortcuts: Record<string, string[]>,
  action: string,
  fallback: string[],
): string[] {
  const hasAction = Object.prototype.hasOwnProperty.call(shortcuts, action);
  return hasAction ? (shortcuts[action] ?? []) : fallback;
}

/** `switchFilter<N>` bindings keyed by filter id, one entry per filter position. */
export function resolveFilterShortcutBindings(
  shortcuts: Record<string, string[]>,
  filterIds: readonly string[],
): Record<string, string[]> {
  const map: Record<string, string[]> = {};
  filterIds.forEach((filterId, index) => {
    map[filterId] = readBinding(shortcuts, `switchFilter${index + 1}`, [`Alt+${index + 1}`]);
  });
  return map;
}

export function resolveNavigationBindings(
  shortcuts: Record<string, string[]>,
): Record<NavigationAction, string[]> {
  const result = {} as Record<NavigationAction, string[]>;
  for (const action of Object.keys(NAVIGATION_DEFAULTS) as NavigationAction[]) {
    result[action] = readBinding(shortcuts, action, NAVIGATION_DEFAULTS[action]);
  }
  return result;
}

/** Single-action binding with fallback default (absent) or disabled (empty). */
export function resolveActionBindings(
  shortcuts: Record<string, string[]>,
  action: string,
  fallback: string[],
): string[] {
  return readBinding(shortcuts, action, fallback);
}

/**
 * Bindings for every registry action: absent falls back to the canonical
 * default, explicitly empty disables. New actions resolve here with no
 * further edits — the table is driven by `HOTKEY_ACTIONS`.
 */
export function resolveAllBindings(shortcuts: Record<string, string[]>): Record<string, string[]> {
  const map: Record<string, string[]> = {};
  for (const action of HOTKEY_ACTIONS) {
    map[action.id] = readBinding(shortcuts, action.id, defaultShortcutsFor(action.id));
  }
  return map;
}

/** Bindings for OS-global actions only, in registry order. */
export function resolveGlobalBindings(
  shortcuts: Record<string, string[]>,
): Record<string, string[]> {
  const map: Record<string, string[]> = {};
  for (const action of HOTKEY_ACTIONS.filter((entry) => entry.scope === "global")) {
    map[action.id] = readBinding(shortcuts, action.id, defaultShortcutsFor(action.id));
  }
  return map;
}
