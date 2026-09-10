import defaultsDocument from "../../keyboard-defaults.json";

interface KeyboardDefaultsDocument {
  shortcuts: Record<string, string[]>;
}

const SHORTCUTS: Record<string, string[]> = (defaultsDocument as KeyboardDefaultsDocument)
  .shortcuts;

/**
 * Canonical default bindings for one `conf/keyboard.json` action.
 *
 * Single source of truth is `keyboard-defaults.json` at the repository
 * root, shared with the backend (embedded in Rust via `include_str!` in
 * `src-tauri/src/keyboard/config.rs`). Returns a copy so callers can
 * never mutate the shared table.
 */
export function defaultShortcutsFor(action: string): string[] {
  return [...(SHORTCUTS[action] ?? [])];
}
