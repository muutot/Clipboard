export function isEditableKeyboardTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;

  return (
    target.matches("input, textarea, select") ||
    target.isContentEditable ||
    target.closest("[contenteditable='true']") !== null
  );
}

/**
 * Item-action shortcuts (Ctrl/⌘ letter) that should still operate on the
 * selected entry even when focus is in an editable target such as the search
 * box. Ctrl+A is deliberately excluded so the search box keeps its native
 * "select all text" behavior.
 *
 * `extraBindings` carries the user's configured item-action chords
 * (conf/keyboard.json): a custom binding such as F2 must also punch through
 * editables, otherwise rebinding an action silently disables it while typing.
 */
export function isItemActionShortcut(
  event: KeyboardEvent,
  extraBindings: readonly string[] = [],
): boolean {
  if (extraBindings.some((binding) => shortcutMatchesEvent(binding, event))) return true;
  if (!(event.ctrlKey || event.metaKey) || event.shiftKey) return false;
  return ["c", "d", "f", "e", "t", "s"].includes(event.key.toLowerCase());
}

/**
 * Native activatable controls fire their click action from the keydown
 * default behavior. When focus sits on one of them, Enter/Space must
 * activate that control — not the list selection below. Note: history
 * cards are divs with role="option" and deliberately keep the hijacked
 * Enter/Space activation, so role="option" is intentionally absent here;
 * the search-suggestion options are real <button> elements and are covered.
 */
export function isActivatableKeyboardTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return !!target.closest(
    "button, a, link, select, summary, label, [role='tab'], [role='menuitem'], [role='button'], [role='link'], [role='checkbox']",
  );
}

function normalizeKeyLabel(key: string): string {
  if (key === " ") return "Space";
  if ([...key].length === 1) return key.toUpperCase();
  return key.charAt(0).toUpperCase() + key.slice(1).toLowerCase();
}

/**
 * Matches a canonical shortcut string (as persisted in conf/keyboard.json,
 * e.g. "Alt+1", "Ctrl+Shift+V", "Arrowright") against a keydown event.
 * Modifier sets must match exactly; a double-modifier binding never matches
 * a single keydown chord.
 */
export function shortcutMatchesEvent(canonical: string, event: KeyboardEvent): boolean {
  const parts = canonical.split("+").filter(Boolean);
  const expectedModifiers = new Set<string>();
  const keys: string[] = [];

  for (const part of parts) {
    const lower = part.toLowerCase();
    if (lower === "ctrl" || lower === "control") {
      expectedModifiers.add("Ctrl");
    } else if (lower === "alt" || lower === "option") {
      expectedModifiers.add("Alt");
    } else if (lower === "shift") {
      expectedModifiers.add("Shift");
    } else if (lower === "meta" || lower === "cmd" || lower === "command") {
      expectedModifiers.add("Meta");
    } else {
      keys.push(part);
    }
  }

  if (keys.length !== 1) return false;

  const actualModifiers = new Set<string>();
  if (event.ctrlKey) actualModifiers.add("Ctrl");
  if (event.altKey) actualModifiers.add("Alt");
  if (event.shiftKey) actualModifiers.add("Shift");
  if (event.metaKey) actualModifiers.add("Meta");

  if (expectedModifiers.size !== actualModifiers.size) return false;
  for (const modifier of expectedModifiers) {
    if (!actualModifiers.has(modifier)) return false;
  }

  return normalizeKeyLabel(event.key) === normalizeKeyLabel(keys[0]);
}
