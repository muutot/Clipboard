//! Shared pure helpers for the OS-global hotkey loop.
//!
//! `windows_hotkey.rs` (real `RegisterHotKey` loop) and
//! `windows_hotkey_stub.rs` (non-Windows placeholder) previously duplicated
//! all of this logic. Both now import from here, so a fix to id assignment,
//! key mapping, or binding conversion applies to both backends at once.
//!
//! Adding a new global action touches none of this file: per-action id
//! ranges are derived from the action's position in
//! `keyboard::global_action_ids()` (`action_id_base`), and `lib.rs` builds
//! one plan entry per registry action.

use std::collections::HashSet;

use crate::keyboard::Modifier;

/// One OS registration: hotkey id plus the Win32 modifier flags/virtual key.
pub type HotkeyRegistration = (i32, u32, u32);

/// First id of the first global action's range. Kept for compatibility with
/// the original toggle-only layout.
pub const FIRST_HOTKEY_ID: i32 = 1;
/// Base id of the second global action (`toggleFloatPanel`). New actions
/// continue the same stride via [`action_id_base`].
pub const FLOAT_HOTKEY_ID_BASE: i32 = 1000;
/// Width of one action's id range. Ranges never overlap by construction.
pub const HOTKEY_ID_STRIDE: i32 = 1000;

/// Id-range base for the Nth global action in registry order
/// (`toggleWindow` = 0, `toggleFloatPanel` = 1, ...). Action 0 starts at
/// `FIRST_HOTKEY_ID` (= 1); every later action starts at a multiple of the
/// stride, preserving the legacy toggle (`1..`) / float (`1000..`) layout.
pub fn action_id_base(action_index: usize) -> i32 {
    if action_index == 0 {
        FIRST_HOTKEY_ID
    } else {
        action_index as i32 * HOTKEY_ID_STRIDE
    }
}

/// Inverse of [`action_id_base`]: maps a fired `WM_HOTKEY` id back to its
/// action position. Returns `None` for ids below the first range.
pub fn action_index_for_hotkey_id(id: i32) -> Option<usize> {
    if id < FIRST_HOTKEY_ID {
        return None;
    }
    if id < FLOAT_HOTKEY_ID_BASE {
        return Some(0);
    }
    Some((id / HOTKEY_ID_STRIDE) as usize)
}

pub fn deduplicate_hotkeys(bindings: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut seen = HashSet::new();
    bindings
        .iter()
        .copied()
        .filter(|binding| seen.insert(*binding))
        .collect()
}

fn assign_hotkey_ids_with_base(bindings: &[(u32, u32)], base: i32) -> Vec<HotkeyRegistration> {
    deduplicate_hotkeys(bindings)
        .into_iter()
        .enumerate()
        .map(|(index, (modifiers, vk))| (base + index as i32, modifiers, vk))
        .collect()
}

pub fn assign_hotkey_ids(bindings: &[(u32, u32)]) -> Vec<HotkeyRegistration> {
    assign_hotkey_ids_with_base(bindings, FIRST_HOTKEY_ID)
}

/// Legacy two-action layout (toggle + float) on the shared message loop.
/// New code should use [`plan_registrations`], which covers every registry
/// action; this stays so existing callers and tests keep working.
pub fn combined_hotkey_registrations(
    toggle_bindings: &[(u32, u32)],
    float_bindings: &[(u32, u32)],
) -> Vec<HotkeyRegistration> {
    plan_registrations(&[toggle_bindings, float_bindings])
        .into_iter()
        .map(|registration| {
            (
                registration.id,
                registration.modifiers,
                registration.virtual_key,
            )
        })
        .collect()
}

/// One planned registration tagged with its registry-action position.
pub struct PlannedRegistration {
    pub id: i32,
    pub action_index: usize,
    pub modifiers: u32,
    pub virtual_key: u32,
}

/// Builds the single shared-loop plan for N global actions: the i-th slice
/// holds the Win32 chords of the i-th action in
/// `keyboard::global_action_ids()` order. Id ranges are disjoint by
/// construction ([`action_id_base`]), so routing needs no per-action code.
pub fn plan_registrations(plans: &[&[(u32, u32)]]) -> Vec<PlannedRegistration> {
    let mut registrations = Vec::new();
    for (action_index, bindings) in plans.iter().enumerate() {
        for (id, modifiers, vk) in
            assign_hotkey_ids_with_base(bindings, action_id_base(action_index))
        {
            registrations.push(PlannedRegistration {
                id,
                action_index,
                modifiers,
                virtual_key: vk,
            });
        }
    }
    registrations
}

pub fn windows_virtual_key(key: &str) -> Option<u32> {
    let normalized = key.to_ascii_uppercase();
    if let Some(function_key) = normalized
        .strip_prefix('F')
        .and_then(|number| number.parse::<u32>().ok())
        .filter(|number| (1..=24).contains(number))
    {
        return Some(0x6F + function_key);
    }

    match normalized.as_str() {
        "BACKSPACE" => Some(0x08),
        "TAB" => Some(0x09),
        "ENTER" | "RETURN" => Some(0x0D),
        "ESC" | "ESCAPE" => Some(0x1B),
        "SPACE" => Some(0x20),
        "PAGEUP" => Some(0x21),
        "PAGEDOWN" => Some(0x22),
        "END" => Some(0x23),
        "HOME" => Some(0x24),
        "LEFT" | "ARROWLEFT" => Some(0x25),
        "UP" | "ARROWUP" => Some(0x26),
        "RIGHT" | "ARROWRIGHT" => Some(0x27),
        "DOWN" | "ARROWDOWN" => Some(0x28),
        "INSERT" => Some(0x2D),
        "DELETE" | "DEL" => Some(0x2E),
        other if other.len() == 1 => {
            let byte = other.as_bytes()[0];
            (byte.is_ascii_alphanumeric()).then_some(byte as u32)
        }
        _ => None,
    }
}

/// Provider-neutral identity of a chord for conflict detection and support
/// checks: the canonical modifier set plus the shared platform virtual key.
/// Two spellings that register the same OS chord (for example `Ctrl+Esc` and
/// `Ctrl+Escape`) share one identity. Returns `None` when the key has no
/// global-hotkey mapping, so callers can reject it instead of silently
/// dropping it at registration time.
pub fn hotkey_registration_identity(binding: &crate::keyboard::ShortcutBinding) -> Option<String> {
    match binding {
        crate::keyboard::ShortcutBinding::Chord { modifiers, key } => {
            let virtual_key = windows_virtual_key(key)?;
            let labels: Vec<&str> = modifiers.iter().map(|modifier| modifier.label()).collect();
            Some(format!("{}+{}", labels.join("+"), virtual_key))
        }
        crate::keyboard::ShortcutBinding::DoubleModifier { modifier } => {
            Some(format!("{}+{}", modifier.label(), modifier.label()))
        }
    }
}

pub fn shortcut_to_windows_hotkey(
    binding: &crate::keyboard::ShortcutBinding,
) -> Option<(u32, u32)> {
    match binding {
        crate::keyboard::ShortcutBinding::Chord { modifiers, key } => {
            let mut mod_flags: u32 = 0;
            for m in modifiers {
                match m {
                    crate::keyboard::Modifier::Alt => {
                        mod_flags |= super::windows_clipboard::MOD_ALT
                    }
                    crate::keyboard::Modifier::Control => {
                        mod_flags |= super::windows_clipboard::MOD_CONTROL
                    }
                    crate::keyboard::Modifier::Shift => {
                        mod_flags |= super::windows_clipboard::MOD_SHIFT
                    }
                    crate::keyboard::Modifier::Meta => {
                        mod_flags |= super::windows_clipboard::MOD_WIN
                    }
                }
            }
            let vk = windows_virtual_key(key)?;
            Some((mod_flags, vk))
        }
        crate::keyboard::ShortcutBinding::DoubleModifier { .. } => None,
    }
}
pub fn shortcut_bindings_to_windows_hotkeys(
    bindings: &[crate::keyboard::ShortcutBinding],
) -> Vec<(u32, u32)> {
    let converted = bindings
        .iter()
        .filter_map(shortcut_to_windows_hotkey)
        .collect::<Vec<_>>();
    deduplicate_hotkeys(&converted)
}

pub fn shortcut_bindings_to_double_modifiers(
    bindings: &[crate::keyboard::ShortcutBinding],
) -> Vec<Modifier> {
    use std::collections::BTreeSet;
    let mut seen = BTreeSet::new();
    bindings
        .iter()
        .filter_map(|binding| match binding {
            crate::keyboard::ShortcutBinding::DoubleModifier { modifier } => {
                seen.insert(*modifier).then_some(*modifier)
            }
            crate::keyboard::ShortcutBinding::Chord { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::keyboard::ShortcutBinding;

    use super::{
        action_id_base, action_index_for_hotkey_id, assign_hotkey_ids,
        combined_hotkey_registrations, hotkey_registration_identity, plan_registrations,
        shortcut_bindings_to_double_modifiers, shortcut_bindings_to_windows_hotkeys,
        FIRST_HOTKEY_ID, FLOAT_HOTKEY_ID_BASE,
    };

    #[test]
    fn action_id_ranges_are_disjoint_and_round_trip() {
        assert_eq!(action_id_base(0), FIRST_HOTKEY_ID);
        assert_eq!(action_id_base(1), FLOAT_HOTKEY_ID_BASE);
        assert_eq!(action_id_base(2), 2000);
        assert_eq!(action_index_for_hotkey_id(FIRST_HOTKEY_ID), Some(0));
        assert_eq!(action_index_for_hotkey_id(999), Some(0));
        assert_eq!(action_index_for_hotkey_id(FLOAT_HOTKEY_ID_BASE), Some(1));
        assert_eq!(
            action_index_for_hotkey_id(FLOAT_HOTKEY_ID_BASE + 7),
            Some(1)
        );
        assert_eq!(action_index_for_hotkey_id(2000), Some(2));
        assert_eq!(action_index_for_hotkey_id(0), None);
        assert_eq!(action_index_for_hotkey_id(-3), None);
    }

    #[test]
    fn plan_registrations_keep_disjoint_id_ranges_per_action() {
        let toggle = [(1, b'V' as u32)];
        let float = [(4, b'F' as u32), (2, 0x20)];
        let plan = plan_registrations(&[&toggle, &float]);

        let ids: Vec<_> = plan.iter().map(|r| (r.id, r.action_index)).collect();
        assert_eq!(
            ids,
            vec![
                (FIRST_HOTKEY_ID, 0),
                (FLOAT_HOTKEY_ID_BASE, 1),
                (FLOAT_HOTKEY_ID_BASE + 1, 1),
            ]
        );
    }

    #[test]
    fn third_action_gets_its_own_range_without_code_changes() {
        let plan = plan_registrations(&[&[], &[], &[(1, b'X' as u32)]]);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].action_index, 2);
        assert_eq!(action_index_for_hotkey_id(plan[0].id), Some(2));
    }

    #[test]
    fn converts_supported_windows_keys() {
        let bindings = [
            ShortcutBinding::from_str("Alt+V").unwrap(),
            ShortcutBinding::from_str("Ctrl+Enter").unwrap(),
            ShortcutBinding::from_str("Shift+F5").unwrap(),
            ShortcutBinding::from_str("Meta+1").unwrap(),
        ];

        assert_eq!(
            shortcut_bindings_to_windows_hotkeys(&bindings),
            vec![(1, b'V' as u32), (2, 0x0D), (4, 0x74), (8, b'1' as u32)]
        );
    }

    #[test]
    fn batches_bindings_without_duplicate_registration_ids() {
        let bindings = [(1, b'V' as u32), (1, b'V' as u32), (2, 0x20)];

        assert_eq!(
            assign_hotkey_ids(&bindings),
            vec![
                (FIRST_HOTKEY_ID, 1, b'V' as u32),
                (FIRST_HOTKEY_ID + 1, 2, 0x20)
            ]
        );
    }

    #[test]
    fn legacy_combined_layout_matches_the_generic_plan() {
        let toggle = [(1, b'V' as u32)];
        let float = [(4, b'F' as u32)];
        assert_eq!(
            combined_hotkey_registrations(&toggle, &float),
            vec![
                (FIRST_HOTKEY_ID, 1, b'V' as u32),
                (FLOAT_HOTKEY_ID_BASE, 4, b'F' as u32),
            ]
        );
        assert!(combined_hotkey_registrations(&[], &[]).is_empty());
    }

    #[test]
    fn registration_identity_unifies_key_aliases_and_rejects_unknown_keys() {
        let esc = ShortcutBinding::from_str("Ctrl+Esc").unwrap();
        let escape = ShortcutBinding::from_str("Ctrl+Escape").unwrap();
        assert_eq!(
            hotkey_registration_identity(&esc),
            hotkey_registration_identity(&escape)
        );
        assert_eq!(
            hotkey_registration_identity(&ShortcutBinding::from_str("Ctrl+,").unwrap()),
            None
        );
    }

    #[test]
    fn double_modifier_bindings_are_not_registered_as_native_chords() {
        use super::shortcut_to_windows_hotkey;
        let binding = ShortcutBinding::from_str("Shift+Shift").unwrap();
        assert_eq!(shortcut_to_windows_hotkey(&binding), None);
    }

    #[test]
    fn collects_double_modifier_bindings_for_the_keyboard_hook() {
        use crate::keyboard::Modifier;
        let bindings = [
            ShortcutBinding::from_str("Shift+Shift").unwrap(),
            ShortcutBinding::from_str("Alt+V").unwrap(),
            ShortcutBinding::from_str("Ctrl+Ctrl").unwrap(),
            ShortcutBinding::from_str("Shift+Shift").unwrap(),
        ];

        assert_eq!(
            shortcut_bindings_to_double_modifiers(&bindings),
            vec![Modifier::Shift, Modifier::Control]
        );
    }
}
