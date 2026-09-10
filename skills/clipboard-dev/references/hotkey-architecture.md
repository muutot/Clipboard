# Hotkey Architecture

Single reference for every shortcut in the app: OS-global hotkeys (Windows
`RegisterHotKey` loop) and in-window shortcuts (main-route keydown table).
Both sides are driven by action registries, so a new shortcut is a few rows
of data plus its dispatch branch — never a new manager, loop, or settings
panel.

## Layer map

```text
keyboard-defaults.json            canonical defaults (single source; Rust embeds
                                  via include_str!, frontend reads via
                                  src/lib/keyboard-defaults.ts)
        │
        ├─ backend ──────────────────────────────┼─ frontend ───────────────
        │                                        │
conf/keyboard.json                conf/keyboard.json (same file)
KeyboardConfigStore               services/keyboard.ts (invoke wrappers)
        │                                        │
keyboard::GLOBAL_HOTKEY_ACTIONS   src/lib/keyboard-registry.ts::HOTKEY_ACTIONS
(keyboard/actions.rs)             (scope/category/icon/i18n metadata)
        │                                        │
resolve_global_hotkey_plan        resolveAllBindings / resolveGlobalBindings
(lib.rs: one plan per action,     (utils/shortcut-bindings.ts; absent →
 registry order)                  default, empty → disabled)
        │                                        │
HotkeyManager::apply_global_plan  keyboard-actions.ts::resolveKeyAction
/ start_with_plan (single         (pure decision table)
shared loop, per-action id        │
ranges via hotkey_common)         +page.svelte::executeKeyAction (handlers)
        │
native handler (toggle main /     future global actions without a native
float panel) or `global-hotkey`   handler: listen for the `global-hotkey`
event forward (anything else)     event with the action id payload
```

## Backend ownership

| Module                            | Responsibility                                                                                                                                                                                                                                                       |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `keyboard/actions.rs`             | **The** global-action registry: `GLOBAL_HOTKEY_ACTIONS` (`toggleWindow` with `allow_double_tap`, `toggleFloatPanel` without), `is_global_action`, `global_action_ids`, per-action `action_bindings` (skips invalid chords so one bad binding never breaks the plan). |
| `keyboard/config.rs`              | `conf/keyboard.json` schema, canonical normalization, cross-action conflict rejection, atomic save.                                                                                                                                                                  |
| `keyboard/matcher.rs`             | In-memory chord + double-tap matcher (used by tests and non-OS paths).                                                                                                                                                                                               |
| `keyboard/manager.rs`             | Config store + matcher composition; no OS code.                                                                                                                                                                                                                      |
| `platform/hotkey_common.rs`       | Shared pure logic for both OS backends: per-action id ranges (`action_id_base` / `action_index_for_hotkey_id`, stride 1000, legacy `1..` / `1000..` layout preserved), `plan_registrations`, Win32 key mapping, binding conversion.                                  |
| `platform/windows_hotkey.rs`      | Real `RegisterHotKey` message loop + low-level double-modifier hook.                                                                                                                                                                                                 |
| `platform/windows_hotkey_stub.rs` | Non-Windows placeholder: identical dispatch, no OS registration.                                                                                                                                                                                                     |
| `lib.rs`                          | `resolve_global_hotkey_plan` (registry → chords/doubles) + `refresh_hotkey_registrations` (single rebuild) + startup wiring with bundled-default toggle fallback.                                                                                                    |
| `commands/config/misc.rs`         | `get/configure/delete/reset_keyboard_config`; configure/delete refresh the OS loop only when `is_global_action(&action)`.                                                                                                                                            |

`HotkeyManager` stores chords per registry action (`global_chords` parallel to
`global_action_ids()` order) plus the toggle-only `toggle_doubles`. Its
dispatch loop handles `ToggleMain` (window show/hide + quick-paste target
remember) and `ToggleFloat` natively; any later registry action arrives as
`Forward(index)` and is emitted as a `global-hotkey` event carrying the
action id — listeners need no manager changes.

## Frontend ownership

| Module                                            | Responsibility                                                                                                                                                                                                                                   |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `src/lib/keyboard-registry.ts`                    | **The** frontend registry: every action's id, scope (`global` must mirror the backend order 1:1), category, icon, i18n keys, and settings-search id. `GLOBAL_ACTION_IDS`, `isGlobalAction`, `actionsByCategory`.                                 |
| `src/lib/utils/shortcut-bindings.ts`              | `resolveAllBindings` (every registry action), `resolveGlobalBindings` (global only), plus the legacy per-group resolvers (filter position, navigation, single action). Rule everywhere: absent → canonical default, explicitly empty → disabled. |
| `src/lib/utils/keyboard.ts`                       | Target guards (`isEditableKeyboardTarget`, `isItemActionShortcut`, `isActivatableKeyboardTarget`) and `shortcutMatchesEvent` (exact modifier set, canonical format).                                                                             |
| `src/lib/utils/keyboard-actions.ts`               | Pure `resolveKeyAction` decision table (branch order is load-bearing; unit-tested).                                                                                                                                                              |
| `src/routes/+page.svelte`                         | `executeKeyAction` handler switch; loads `keyboardShortcuts` on mount and window focus.                                                                                                                                                          |
| `src/lib/components/KeyboardSettingsPanel.svelte` | Renders cards from `HOTKEY_ACTIONS` (no per-action markup); recording, add/remove binding, reset.                                                                                                                                                |

## Add a shortcut

Global (OS-wide, works unfocused):

1. Append the default binding to `keyboard-defaults.json`.
2. Append one row to `keyboard::GLOBAL_HOTKEY_ACTIONS` (`actions.rs`).
3. Append one row to `HOTKEY_ACTIONS` with `scope: "global"` **at the same
   position** (`keyboard-registry.ts`).
4. Add i18n `keyboard.*` label/desc keys (`en.ts`, `zh-CN.ts`, `types.ts`).
5. If it needs a native handler, add one match arm in both `HotkeyManager`
   dispatch loops; otherwise just listen for `global-hotkey` — no backend
   edits needed at all. Pin the new default in `keyboard/config.rs` tests
   and `keyboard-registry.test.ts` (parity is asserted automatically).

Window-only (main window focused):

1. Same defaults + registry steps with `scope: "window"`.
2. Same i18n step when user-facing.
3. One `KeyAction` variant + one `resolveKeyAction` branch +
   one `executeKeyAction` case. The settings card, fallback, and disable
   behavior come from the registry for free.

## High-availability constraints

- One shared message loop for all global actions; `stop()` before every
  rebuild tears down the previous message window so re-registration never
  leaks hotkeys. Startup and refresh both go through a single plan apply
  (`start_with_plan` / `apply_global_plan`), never one restart per action.
- Readiness handshake: the loop thread signals after publishing its hwnd;
  `stop()+join` before that point cannot block forever.
- Per-action fault isolation: invalid chords are skipped per action; a
  `RegisterHotKey` failure for one binding must not silently drop the rest
  (log the failing chord with its action id).
- Config semantics are load-bearing: absent action → canonical default,
  explicitly empty → disabled, conflict across actions → reject with
  `ShortcutConflict`. Never merge keyboard config into `conf.json`.
- Canonical shortcut format is the contract between layers (modifier order
  Ctrl/Alt/Shift/Meta, single-char upper-cased, multi-char first-upper
  rest-lower); both `ShortcutBinding::canonical` and
  `shortcutMatchesEvent` implement it — keep them in lock-step.
- Double-modifier taps stay toggle-only by design (`RegisterHotKey` cannot
  express them); `allow_double_tap` marks the single action that owns the
  hook. Other keys pressed mid-tap cancel the pending sequence.
- Non-Windows has no OS-global path: global bindings work only while the
  main window is focused there. Do not claim global behavior from the stub
  compiling. `quickPaste` ships unbound by default.
- Frontend and backend registries must stay in parity: `global_action_ids()`
  order == `GLOBAL_ACTION_IDS` order (id ranges derive from position).
  `keyboard-registry.test.ts` fails the build if defaults and registry drift.

## Verification

- `cargo test --lib keyboard` — config/parse/matcher/registry rules.
- `cargo test --lib hotkey` — id ranges, routing (`Forward` for new
  actions), conversion, double-tap tracker, quick-paste target.
- `npx vitest run src/lib/keyboard-registry.test.ts
src/lib/utils/shortcut-bindings.test.ts
src/lib/utils/keyboard-actions.test.ts` — registry/defaults parity,
  binding resolution, decision table.
- `npm run check` for registry/panel/route changes; `npm run lint:rust`
  and `cargo fmt` for backend changes.

## Change checklist

- Keep `keyboard-defaults.json` the single default source; never hardcode a
  default binding elsewhere.
- Update both registries, i18n, settings-search hints, and the tests above
  together with the dispatch branch.
- Update this reference when layers, the add-shortcut flow, id routing,
  event names, or availability constraints change.
