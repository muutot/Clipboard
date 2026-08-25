// Shortcut binding resolution extracted from the main route. Maps the
// persisted conf/keyboard.json actions onto runtime handlers: an absent
// action falls back to its default, and an action explicitly configured to
// empty disables that binding (the settings panel exposes the same keys).

export type NavigationAction =
  "moveSelectionUp" | "moveSelectionDown" | "switchFilterNext" | "switchFilterPrev";

const NAVIGATION_DEFAULTS: Record<NavigationAction, string[]> = {
  moveSelectionUp: ["ArrowUp"],
  moveSelectionDown: ["ArrowDown"],
  switchFilterNext: ["ArrowRight", "Tab"],
  switchFilterPrev: ["ArrowLeft", "Shift+Tab"],
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
