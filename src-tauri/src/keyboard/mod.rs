mod actions;
mod binding;
mod config;
mod manager;
mod matcher;

pub use actions::{
    action_bindings, global_action_ids, is_global_action, ActionScope, HotkeyActionDef,
    GLOBAL_HOTKEY_ACTIONS,
};

pub use binding::{Modifier, ShortcutBinding, ShortcutParseError};
pub use config::{KeyboardConfig, KeyboardConfigStore};
pub use manager::KeyboardManager;
pub use matcher::{ShortcutMatcher, DEFAULT_DOUBLE_TAP_INTERVAL_MS};
