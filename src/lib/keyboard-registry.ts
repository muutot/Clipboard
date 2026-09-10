import { defaultShortcutsFor } from "$lib/keyboard-defaults";
import type { IconName } from "$lib/types/clipboard";

/**
 * Single frontend registry for every `conf/keyboard.json` action.
 *
 * This mirrors the backend's `keyboard::GLOBAL_HOTKEY_ACTIONS`
 * (`src-tauri/src/keyboard/actions.rs`): `scope: "global"` rows must stay in
 * 1:1 parity with that table (order included — it drives OS hotkey id
 * ranges), while `scope: "window"` rows are matched in-window by
 * `utils/keyboard-actions.ts`.
 *
 * Adding a shortcut takes ~4 lines:
 * 1. one default entry in `keyboard-defaults.json` (the canonical source);
 * 2. one row here (id, scope, category, icon, i18n keys);
 * 3. i18n `keyboard.*` label/desc keys when user-facing;
 * 4. the dispatch branch itself — a `KeyAction` variant + one
 *    `resolveKeyAction` branch + one `executeKeyAction` case for window
 *    actions, or nothing at all for global actions (the backend forwards
 *    them as `global-hotkey` events automatically).
 */
export type HotkeyScope = "global" | "window";
export type HotkeyCategory = "item" | "quick" | "system" | "switch";

export interface HotkeyActionDef {
  id: string;
  scope: HotkeyScope;
  category: HotkeyCategory;
  icon: IconName;
  labelKey?: string;
  descKey?: string;
  /** `data-settings-search-id` for the settings card; null = unlisted. */
  searchId?: string | null;
  /** Marks OS-global cards with the system badge in settings. */
  system?: boolean;
}

function quickCopyDef(n: number): HotkeyActionDef {
  return {
    id: `quickCopy${n}`,
    scope: "window",
    category: "quick",
    icon: "clipboard",
    descKey: "keyboard.quickCopyDesc",
    searchId: "keyboard.quick-copy",
  };
}

function switchFilterDef(n: number, labelKey: string, icon: IconName = "grid"): HotkeyActionDef {
  return {
    id: `switchFilter${n}`,
    scope: "window",
    category: "switch",
    icon,
    labelKey,
    descKey: labelKey,
    searchId: "keyboard.switch-filter",
  };
}

export const HOTKEY_ACTIONS: HotkeyActionDef[] = [
  {
    id: "copyItem",
    scope: "window",
    category: "item",
    icon: "copy",
    labelKey: "keyboard.copyItem",
    descKey: "keyboard.copyItemDesc",
    searchId: "keyboard.copy-item",
  },
  {
    id: "deleteItem",
    scope: "window",
    category: "item",
    icon: "trash",
    labelKey: "keyboard.deleteItem",
    descKey: "keyboard.deleteItemDesc",
    searchId: "keyboard.delete-item",
  },
  {
    id: "favoriteItem",
    scope: "window",
    category: "item",
    icon: "star",
    labelKey: "keyboard.favoriteItem",
    descKey: "keyboard.favoriteItemDesc",
    searchId: "keyboard.favorite-item",
  },
  {
    id: "addTag",
    scope: "window",
    category: "item",
    icon: "tag",
    labelKey: "keyboard.addTag",
    descKey: "keyboard.addTagDesc",
  },
  {
    id: "moveSelectionUp",
    scope: "window",
    category: "switch",
    icon: "arrow-up",
    labelKey: "keyboard.moveSelectionUp",
    descKey: "keyboard.moveSelectionDesc",
  },
  {
    id: "moveSelectionDown",
    scope: "window",
    category: "switch",
    icon: "arrow-down",
    labelKey: "keyboard.moveSelectionDown",
    descKey: "keyboard.moveSelectionDesc",
  },
  {
    id: "switchFilterNext",
    scope: "window",
    category: "switch",
    icon: "arrow-right",
    labelKey: "keyboard.switchFilterNext",
    descKey: "keyboard.switchFilterDesc",
  },
  {
    id: "switchFilterPrev",
    scope: "window",
    category: "switch",
    icon: "arrow-left",
    labelKey: "keyboard.switchFilterPrev",
    descKey: "keyboard.switchFilterDesc",
  },
  switchFilterDef(1, "keyboard.switchFilterAll"),
  switchFilterDef(2, "keyboard.switchFilterText"),
  switchFilterDef(3, "keyboard.switchFilterLink"),
  switchFilterDef(4, "keyboard.switchFilterImage"),
  switchFilterDef(5, "keyboard.switchFilterFile"),
  switchFilterDef(6, "keyboard.switchFilterFavorite"),
  switchFilterDef(7, "keyboard.switchFilterDeleted"),
  {
    id: "clearSelection",
    scope: "window",
    category: "item",
    icon: "x",
    labelKey: "keyboard.clearSelection",
    descKey: "keyboard.clearSelectionDesc",
  },
  {
    id: "openDetail",
    scope: "window",
    category: "item",
    icon: "eye",
    labelKey: "keyboard.openDetail",
    descKey: "keyboard.viewDetailDesc",
    searchId: "keyboard.open-detail",
  },
  {
    id: "downloadItem",
    scope: "window",
    category: "item",
    icon: "download",
    labelKey: "keyboard.downloadItem",
    descKey: "keyboard.saveItemDesc",
  },
  {
    id: "selectAll",
    scope: "window",
    category: "item",
    icon: "check",
    labelKey: "keyboard.selectAll",
    descKey: "keyboard.selectAllDesc",
    searchId: "keyboard.select-all",
  },
  {
    id: "quickPaste",
    scope: "window",
    category: "item",
    icon: "clipboard",
    labelKey: "keyboard.quickPaste",
    descKey: "keyboard.pasteToWindowDesc",
    searchId: "keyboard.quick-paste",
  },
  quickCopyDef(1),
  quickCopyDef(2),
  quickCopyDef(3),
  quickCopyDef(4),
  quickCopyDef(5),
  quickCopyDef(6),
  quickCopyDef(7),
  quickCopyDef(8),
  quickCopyDef(9),
  {
    id: "toggleWindow",
    scope: "global",
    category: "system",
    icon: "eye",
    labelKey: "keyboard.toggleWindow",
    descKey: "keyboard.toggleWindowDesc",
    searchId: "keyboard.toggle-window",
    system: true,
  },
  {
    id: "toggleFloatPanel",
    scope: "global",
    category: "system",
    icon: "layers",
    labelKey: "keyboard.toggleFloatPanel",
    descKey: "keyboard.toggleFloatPanelDesc",
    searchId: "keyboard.toggle-float-panel",
    system: true,
  },
  {
    id: "hideWindow",
    scope: "window",
    category: "system",
    icon: "eye",
    labelKey: "keyboard.hideWindow",
    descKey: "keyboard.hideWindowDesc",
  },
  {
    id: "focusSearch",
    scope: "window",
    category: "system",
    icon: "search",
    labelKey: "keyboard.focusSearch",
    descKey: "keyboard.focusSearchDesc",
  },
];

/** Global action ids in registry order — must match the backend order. */
export const GLOBAL_ACTION_IDS: string[] = HOTKEY_ACTIONS.filter(
  (action) => action.scope === "global",
).map((action) => action.id);

export function isGlobalAction(action: string): boolean {
  return GLOBAL_ACTION_IDS.includes(action);
}

export function actionsByCategory(category: HotkeyCategory): HotkeyActionDef[] {
  return HOTKEY_ACTIONS.filter((action) => action.category === category);
}

/** Canonical defaults for one registry action (copy; never mutates shared state). */
export function registryDefaultsFor(action: string): string[] {
  return defaultShortcutsFor(action);
}
