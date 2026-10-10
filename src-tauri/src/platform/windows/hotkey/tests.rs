//! Unit tests for hotkey routing, the tracker, the paste target, and the input layout.

use std::sync::Arc;

use crate::keyboard::Modifier;
use crate::platform::hotkey_common::action_id_base;

use super::double_tap::DoubleModifierTracker;
use super::paste_target::{
    clear_foreground_paste_target, foreground_hook_proc, set_foreground_paste_target,
    QuickPasteTarget, EVENT_SYSTEM_FOREGROUND, OBJID_WINDOW,
};
use super::{action_for_hotkey_id, HotkeyAction, FIRST_HOTKEY_ID, FLOAT_HOTKEY_ID_BASE};

const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_V: u32 = b'V' as u32;

#[cfg(target_os = "windows")]
#[test]
fn input_struct_matches_win32_layout() {
    use super::paste_target::{HardwareInput, Input, KeyboardInput, MouseInput};

    // sizeof(INPUT) on 64-bit Windows is 40 bytes, on 32-bit 28 bytes.
    let (expected_input, mouse, keyboard) = if cfg!(target_pointer_width = "64") {
        (40, 32, 24)
    } else {
        (28, 24, 16)
    };
    assert_eq!(size_of::<Input>(), expected_input);
    assert_eq!(size_of::<MouseInput>(), mouse);
    assert_eq!(size_of::<KeyboardInput>(), keyboard);
    assert_eq!(size_of::<HardwareInput>(), 8);
}

#[test]
fn quick_paste_target_is_consumed_once() {
    let target = QuickPasteTarget::default();
    target.remember(42);

    assert_eq!(target.take(), Some(42));
    assert_eq!(target.take(), None);
}

#[test]
fn quick_paste_target_ignores_invalid_window_handle() {
    let target = QuickPasteTarget::default();
    target.remember(0);

    assert_eq!(target.take(), None);
}

#[test]
fn foreground_hook_records_window_objects_only() {
    let shared = Arc::new(QuickPasteTarget::default());
    set_foreground_paste_target(&shared);
    unsafe {
        foreground_hook_proc(0, EVENT_SYSTEM_FOREGROUND, 77, OBJID_WINDOW, 0, 0, 0);
    }
    assert_eq!(shared.take(), Some(77));
    unsafe {
        // Non-window objects and null handles must not become targets.
        foreground_hook_proc(0, EVENT_SYSTEM_FOREGROUND, 78, 1, 0, 0, 0);
        foreground_hook_proc(0, EVENT_SYSTEM_FOREGROUND, 0, OBJID_WINDOW, 0, 0, 0);
    }
    assert_eq!(shared.take(), None);
    clear_foreground_paste_target();
}

#[test]
fn hotkey_ids_route_to_their_action() {
    assert_eq!(
        action_for_hotkey_id(FIRST_HOTKEY_ID),
        HotkeyAction::ToggleMain
    );
    assert_eq!(
        action_for_hotkey_id(FLOAT_HOTKEY_ID_BASE),
        HotkeyAction::ToggleFloat
    );
    assert_eq!(
        action_for_hotkey_id(FLOAT_HOTKEY_ID_BASE + 7),
        HotkeyAction::ToggleFloat
    );
    // A third registry action forwards without manager changes.
    assert_eq!(
        action_for_hotkey_id(action_id_base(2)),
        HotkeyAction::Forward(2)
    );
    // Unknown ids fail toward the main toggle, never toward float.
    assert_eq!(action_for_hotkey_id(0), HotkeyAction::ToggleMain);
    assert_eq!(action_for_hotkey_id(-3), HotkeyAction::ToggleMain);
}

#[test]
fn tracker_fires_on_a_clean_double_tap() {
    let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

    assert!(!tracker.on_key_event(VK_SHIFT, true, 1_000));
    assert!(!tracker.on_key_event(VK_SHIFT, false, 1_050));
    assert!(!tracker.on_key_event(VK_SHIFT, true, 1_200));
    assert!(tracker.on_key_event(VK_SHIFT, false, 1_250));
}

#[test]
fn tracker_ignores_taps_outside_the_double_tap_interval() {
    let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

    tracker.on_key_event(VK_SHIFT, true, 1_000);
    tracker.on_key_event(VK_SHIFT, false, 1_050);
    tracker.on_key_event(VK_SHIFT, true, 1_500);
    assert!(!tracker.on_key_event(VK_SHIFT, false, 1_550));
    tracker.on_key_event(VK_SHIFT, true, 1_700);
    assert!(tracker.on_key_event(VK_SHIFT, false, 1_750));
}

#[test]
fn tracker_fires_when_the_tick_counter_wraps() {
    let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

    assert!(!tracker.on_key_event(VK_SHIFT, true, u32::MAX - 250));
    assert!(!tracker.on_key_event(VK_SHIFT, false, u32::MAX - 200));
    assert!(!tracker.on_key_event(VK_SHIFT, true, u32::MAX - 50));
    assert!(tracker.on_key_event(VK_SHIFT, false, 0));
}

#[test]
fn tracker_ignores_taps_outside_the_interval_across_the_wrap() {
    let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

    tracker.on_key_event(VK_SHIFT, true, u32::MAX - 250);
    tracker.on_key_event(VK_SHIFT, false, u32::MAX - 200);
    // One second after the wrapped first tap: outside the 300 ms window.
    tracker.on_key_event(VK_SHIFT, true, 800);
    assert!(!tracker.on_key_event(VK_SHIFT, false, 850));
}

#[test]
fn tracker_treats_chords_and_other_keys_as_interruptions() {
    let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

    // Shift+V is a chord, not a bare tap.
    tracker.on_key_event(VK_SHIFT, true, 1_000);
    tracker.on_key_event(VK_V, true, 1_020);
    tracker.on_key_event(VK_V, false, 1_040);
    assert!(!tracker.on_key_event(VK_SHIFT, false, 1_060));

    // A clean double tap afterwards still works.
    tracker.on_key_event(VK_SHIFT, true, 1_200);
    tracker.on_key_event(VK_SHIFT, false, 1_220);
    tracker.on_key_event(VK_SHIFT, true, 1_320);
    assert!(tracker.on_key_event(VK_SHIFT, false, 1_340));
}

#[test]
fn tracker_resets_when_a_different_modifier_is_tapped_in_between() {
    let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

    tracker.on_key_event(VK_SHIFT, true, 1_000);
    tracker.on_key_event(VK_SHIFT, false, 1_020);
    tracker.on_key_event(VK_CONTROL, true, 1_060);
    tracker.on_key_event(VK_CONTROL, false, 1_080);
    tracker.on_key_event(VK_SHIFT, true, 1_120);
    assert!(!tracker.on_key_event(VK_SHIFT, false, 1_140));
}

#[test]
fn tracker_only_fires_for_registered_modifiers() {
    let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

    tracker.on_key_event(VK_CONTROL, true, 1_000);
    tracker.on_key_event(VK_CONTROL, false, 1_020);
    tracker.on_key_event(VK_CONTROL, true, 1_100);
    assert!(!tracker.on_key_event(VK_CONTROL, false, 1_120));
}

#[test]
fn tracker_ignores_key_auto_repeat_while_a_modifier_is_held() {
    let mut tracker = DoubleModifierTracker::new([Modifier::Shift]);

    tracker.on_key_event(VK_SHIFT, true, 1_000);
    tracker.on_key_event(VK_SHIFT, true, 1_050);
    tracker.on_key_event(VK_SHIFT, false, 1_100);
    tracker.on_key_event(VK_SHIFT, true, 1_200);
    assert!(tracker.on_key_event(VK_SHIFT, false, 1_250));
}
