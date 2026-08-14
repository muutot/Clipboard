export function isEditableKeyboardTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;

  return (
    target.matches("input, textarea, select") ||
    target.isContentEditable ||
    target.closest("[contenteditable='true']") !== null
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
