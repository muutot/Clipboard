use std::str::FromStr;

use super::{KeyboardConfig, ShortcutBinding};

/// Dispatch scope of a `conf/keyboard.json` action.
///
/// - `Global`: chord bindings are registered with the OS (`RegisterHotKey`
///   on Windows) and dispatched by `HotkeyManager` even when the app is not
///   focused. Adding a new global action only needs one row in
///   [`GLOBAL_HOTKEY_ACTIONS`]; the plan builder, refresh path, and the
///   manager's dispatch loop pick it up without further edits.
/// - `Window`: bindings are matched in-window by the frontend keydown table
///   (`utils/keyboard-actions.ts`). They never touch the OS hotkey loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionScope {
    Global,
    Window,
}

/// One dispatchable hotkey action.
///
/// `allow_double_tap` marks actions whose double-modifier bindings are routed
/// to the low-level keyboard hook. Only `toggleWindow` sets it today:
/// `RegisterHotKey` cannot express a bare double-modifier tap, so the hook
/// stays toggle-only by design and every other global action uses chords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyActionDef {
    pub id: &'static str,
    pub scope: ActionScope,
    pub allow_double_tap: bool,
}

/// Canonical global-action registry. **This is the only place that lists
/// OS-global actions**: `lib.rs` builds the registration plan from it,
/// `configure/delete/reset_keyboard_config` refresh from it, and the frontend
/// mirrors it in `src/lib/keyboard-registry.ts`.
/// New global shortcut = one row here (+ `keyboard-defaults.json` default).
pub const GLOBAL_HOTKEY_ACTIONS: &[HotkeyActionDef] = &[
    HotkeyActionDef {
        id: "toggleWindow",
        scope: ActionScope::Global,
        allow_double_tap: true,
    },
    HotkeyActionDef {
        id: "toggleFloatPanel",
        scope: ActionScope::Global,
        allow_double_tap: false,
    },
];

/// Whether `action` is OS-global (drives `refresh_hotkey_registrations`).
pub fn is_global_action(action: &str) -> bool {
    GLOBAL_HOTKEY_ACTIONS
        .iter()
        .any(|def| def.id == action && def.scope == ActionScope::Global)
}

/// Ids of every global action, in registry order (stable id-range order).
pub fn global_action_ids() -> impl Iterator<Item = &'static str> {
    GLOBAL_HOTKEY_ACTIONS
        .iter()
        .filter(|def| def.scope == ActionScope::Global)
        .map(|def| def.id)
}

/// Parses the configured bindings of one action, skipping invalid entries
/// so a single bad chord can never break the whole registration plan.
pub fn action_bindings(config: &KeyboardConfig, action: &str) -> Vec<ShortcutBinding> {
    config
        .shortcuts
        .get(action)
        .map(|shortcuts| {
            shortcuts
                .iter()
                .filter_map(|shortcut| ShortcutBinding::from_str(shortcut).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{action_bindings, global_action_ids, is_global_action, ActionScope};
    use crate::keyboard::{KeyboardConfig, GLOBAL_HOTKEY_ACTIONS};

    #[test]
    fn registry_lists_the_two_global_actions_in_stable_order() {
        let ids: Vec<_> = global_action_ids().collect();
        assert_eq!(ids, vec!["toggleWindow", "toggleFloatPanel"]);
        assert!(GLOBAL_HOTKEY_ACTIONS
            .iter()
            .all(|def| def.scope == ActionScope::Global));
    }

    #[test]
    fn only_registry_actions_count_as_global() {
        assert!(is_global_action("toggleWindow"));
        assert!(is_global_action("toggleFloatPanel"));
        assert!(!is_global_action("quickPaste"));
        assert!(!is_global_action("noSuchAction"));
    }

    #[test]
    fn invalid_chords_are_skipped_without_breaking_the_plan() {
        let config = KeyboardConfig::from_shortcuts(BTreeMap::from([(
            "toggleWindow".to_owned(),
            vec!["Alt+C".to_owned(), "Ctrl+Alt".to_owned()],
        )]));
        let bindings = action_bindings(&config, "toggleWindow");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].canonical(), "Alt+C");
        assert!(action_bindings(&config, "missingAction").is_empty());
    }
}
