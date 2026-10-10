//! Bare-modifier double-tap detection driven by the low-level keyboard hook.

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::keyboard::{Modifier, DEFAULT_DOUBLE_TAP_INTERVAL_MS};

use super::message_loop::HOTKEY_SENDER;
use super::HotkeyAction;

const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
pub(super) const WH_KEYBOARD_LL: i32 = 13;

fn modifier_from_virtual_key(virtual_key: u32) -> Option<Modifier> {
    match virtual_key {
        0x10 | 0xA0 | 0xA1 => Some(Modifier::Shift),
        0x11 | 0xA2 | 0xA3 => Some(Modifier::Control),
        0x12 | 0xA4 | 0xA5 => Some(Modifier::Alt),
        0x5B | 0x5C => Some(Modifier::Meta),
        _ => None,
    }
}

/// Detects bare modifier double taps from low-level key events.
/// Mirrors `ShortcutMatcher::record_modifier_tap`: a tap is one clean press
/// and release, and any other key in between cancels the pending sequence.
pub(super) struct DoubleModifierTracker {
    registered: BTreeSet<Modifier>,
    double_tap_interval_ms: u64,
    active_press: Option<Modifier>,
    press_interrupted: bool,
    // Timestamps are raw GetTickCount() ticks (u32 milliseconds, wraps every
    // ~49.7 days); wrapping subtraction keeps intervals across the wrap.
    last_tap: Option<(Modifier, u32)>,
}

impl DoubleModifierTracker {
    pub(super) fn new(registered: impl IntoIterator<Item = Modifier>) -> Self {
        Self {
            registered: registered.into_iter().collect(),
            double_tap_interval_ms: DEFAULT_DOUBLE_TAP_INTERVAL_MS,
            active_press: None,
            press_interrupted: false,
            last_tap: None,
        }
    }

    pub(super) fn on_key_event(
        &mut self,
        virtual_key: u32,
        is_key_down: bool,
        timestamp_ms: u32,
    ) -> bool {
        let Some(modifier) = modifier_from_virtual_key(virtual_key) else {
            if is_key_down {
                self.press_interrupted = self.active_press.is_some();
                self.last_tap = None;
            }
            return false;
        };

        if is_key_down {
            if self.active_press == Some(modifier) {
                // Key auto-repeat while the modifier stays held down.
                return false;
            }
            if self.active_press.is_some() {
                // Two different modifiers held together are not a bare tap.
                self.press_interrupted = true;
                self.last_tap = None;
                return false;
            }
            self.active_press = Some(modifier);
            self.press_interrupted = false;
            return false;
        }

        if self.active_press != Some(modifier) {
            return false;
        }
        self.active_press = None;
        if self.press_interrupted {
            self.press_interrupted = false;
            return false;
        }

        let is_double_tap = self
            .last_tap
            .is_some_and(|(previous_modifier, previous_timestamp)| {
                previous_modifier == modifier
                    && u64::from(timestamp_ms.wrapping_sub(previous_timestamp))
                        <= self.double_tap_interval_ms
            });
        if is_double_tap && self.registered.contains(&modifier) {
            self.last_tap = None;
            true
        } else {
            self.last_tap = Some((modifier, timestamp_ms));
            false
        }
    }
}

pub(super) unsafe extern "system" fn keyboard_hook_proc(
    code: i32,
    wparam: usize,
    lparam: isize,
) -> isize {
    extern "system" {
        fn CallNextHookEx(hook: isize, code: i32, wparam: usize, lparam: isize) -> isize;
    }

    #[repr(C)]
    struct KeyboardHookEvent {
        virtual_key: u32,
        scan_code: u32,
        flags: u32,
        time: u32,
        extra_info: usize,
    }

    if code >= 0 && lparam != 0 {
        let message = wparam as u32;
        let is_key_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let is_key_up = message == WM_KEYUP || message == WM_SYSKEYUP;
        if is_key_down || is_key_up {
            let event = &*(lparam as *const KeyboardHookEvent);
            let fired = DOUBLE_MODIFIER_TRACKER
                .lock()
                .ok()
                .and_then(|mut tracker| {
                    tracker.as_mut().map(|tracker| {
                        tracker.on_key_event(event.virtual_key, is_key_down, event.time)
                    })
                })
                .unwrap_or(false);
            if fired {
                // Double-tap modifiers always drive the main toggle; float
                // chords are registered as plain hotkeys only.
                if let Some(tx) = HOTKEY_SENDER.lock().ok().and_then(|g| g.clone()) {
                    let _ = tx.send(HotkeyAction::ToggleMain);
                }
            }
        }
    }

    CallNextHookEx(0, code, wparam, lparam)
}

static DOUBLE_MODIFIER_TRACKER: Mutex<Option<DoubleModifierTracker>> = Mutex::new(None);

pub(super) fn set_double_modifier_tracker(double_modifiers: &[Modifier]) {
    if let Ok(mut guard) = DOUBLE_MODIFIER_TRACKER.lock() {
        *guard = Some(DoubleModifierTracker::new(double_modifiers.iter().copied()));
    }
}

pub(super) fn clear_double_modifier_tracker() {
    if let Ok(mut guard) = DOUBLE_MODIFIER_TRACKER.lock() {
        *guard = None;
    }
}
