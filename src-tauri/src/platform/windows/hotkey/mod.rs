//! Native hotkey engine, split by concern.
//!
//! | File              | Owns                                                                       |
//! | ----------------- | -------------------------------------------------------------------------- |
//! | `mod.rs`          | Shared vocabulary (`HotkeyAction` routing, registration failures) + wiring |
//! | `message_loop.rs` | `RegisterHotKey` message window/class, the loop thread, stop handshake     |
//! | `double_tap.rs`   | Bare-modifier double-tap detection via the low-level keyboard hook         |
//! | `paste_target.rs` | Quick-paste foreground target tracking and Win32 `Ctrl+V` synthesis        |
//! | `manager.rs`      | `HotkeyManager`: per-action chord plan, loop rebuild, dispatch thread      |
//! | `tests.rs`        | Unit tests for routing, the tracker, the target, and the input layout      |

use crate::platform::hotkey_common::action_index_for_hotkey_id;
pub use crate::platform::hotkey_common::{
    assign_hotkey_ids, combined_hotkey_registrations, shortcut_bindings_to_double_modifiers,
    shortcut_bindings_to_windows_hotkeys, HotkeyRegistration, FIRST_HOTKEY_ID,
    FLOAT_HOTKEY_ID_BASE,
};

mod double_tap;
mod manager;
mod message_loop;
mod paste_target;

#[cfg(test)]
mod tests;

pub use manager::HotkeyManager;
pub use message_loop::{
    clear_hotkey_state, set_hotkey_hwnd, set_hotkey_sender, stop_hotkey_thread,
};
pub use paste_target::restore_window_and_paste;

/// OS hotkey action selected by the fired registration id.
///
/// `Forward` carries a registry-action position for actions without a native
/// handler; the dispatch loop emits them as `global-hotkey` events, so a new
/// global shortcut needs no manager changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleMain,
    ToggleFloat,
    Forward(usize),
}

/// Maps a fired `WM_HOTKEY` id onto its action. Pure so the routing is
/// unit-testable without a message loop. Unknown ids fail toward the main
/// toggle, never toward float.
pub fn action_for_hotkey_id(id: i32) -> HotkeyAction {
    match action_index_for_hotkey_id(id) {
        None | Some(0) => HotkeyAction::ToggleMain,
        Some(1) => HotkeyAction::ToggleFloat,
        Some(index) => HotkeyAction::Forward(index),
    }
}

/// Chord registration failure forwarded to the frontend so the keyboard
/// settings panel can surface a conflict (another app already owns the
/// shortcut) instead of silently showing the binding as active.
#[derive(serde::Serialize, Clone)]
pub struct HotkeyRegistrationFailure<'a> {
    pub action: &'a str,
    pub error: String,
}
