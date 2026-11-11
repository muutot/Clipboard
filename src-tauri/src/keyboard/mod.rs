mod actions;
mod binding;
mod config;
mod manager;
mod matcher;

pub use actions::{
    ActionScope, GLOBAL_HOTKEY_ACTIONS, HotkeyActionDef, action_bindings, global_action_ids,
    is_global_action,
};

pub use binding::{Modifier, ShortcutBinding, ShortcutParseError};
pub use config::{KeyboardConfig, KeyboardConfigStore};
pub use manager::KeyboardManager;
pub use matcher::{DEFAULT_DOUBLE_TAP_INTERVAL_MS, ShortcutMatcher};
