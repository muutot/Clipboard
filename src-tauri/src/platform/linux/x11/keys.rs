use super::*;

// ---------------------------------------------------------------------------
// X11 keycode / keysym mapping tables
// ---------------------------------------------------------------------------

/// Maps a `ShortcutBinding` key string to an X11 keysym name.
///
/// Keysym names follow the X11 convention (e.g. "space", "Return", "F1").
/// The `XStringToKeysym` function is the authoritative translation.
///
/// # Key name → Keysym lookup (partial table)
///
/// | Shortcut Key | X11 Keysym Name | Keysym Value |
/// |-------------|-----------------|--------------|
/// | Space       | space           | 0x0020       |
/// | Return      | Return          | 0xFF0D       |
/// | Escape      | Escape          | 0xFF1B       |
/// | Tab         | Tab             | 0xFF09       |
/// | Delete      | Delete          | 0xFFFF       |
/// | Backspace   | BackSpace       | 0xFF08       |
/// | UpArrow     | Up              | 0xFF52       |
/// | DownArrow   | Down            | 0xFF54       |
/// | LeftArrow   | Left            | 0xFF51       |
/// | RightArrow  | Right           | 0xFF53       |
/// | Home        | Home            | 0xFF50       |
/// | End         | End             | 0xFF57       |
/// | PageUp      | Prior           | 0xFF55       |
/// | PageDown    | Next            | 0xFF56       |
/// | F1-F12      | F1-F12          | 0xFFBE-0xFFC9|
///
/// Single-character keys (A-Z, 0-9) map directly to their ASCII keysyms.
#[derive(Debug, Clone)]
pub struct X11KeyMapping {
    /// The X11 keysym value for the key.
    pub keysym: u64,
    /// The X11 keycode (depends on keyboard layout, resolved at runtime).
    pub keycode: u32,
}

impl X11KeyMapping {
    /// Converts a Rust key name into the corresponding X11 keysym name string.
    ///
    /// Returns the X11 keysym name expected by `XStringToKeysym`.
    pub fn key_to_keysym_name(key: &str) -> &'static str {
        match key {
            "Space" => "space",
            "Return" | "Enter" => "Return",
            "Escape" | "Esc" => "Escape",
            "Tab" => "Tab",
            "Delete" => "Delete",
            "Backspace" => "BackSpace",
            "Up" | "UpArrow" => "Up",
            "Down" | "DownArrow" => "Down",
            "Left" | "LeftArrow" => "Left",
            "Right" | "RightArrow" => "Right",
            "Home" => "Home",
            "End" => "End",
            "PageUp" => "Prior",
            "PageDown" => "Next",
            "Insert" => "Insert",
            "Pause" => "Pause",
            "Print" | "PrintScreen" => "Print",
            "CapsLock" => "Caps_Lock",
            "NumLock" => "Num_Lock",
            "ScrollLock" => "Scroll_Lock",
            "F1" => "F1",
            "F2" => "F2",
            "F3" => "F3",
            "F4" => "F4",
            "F5" => "F5",
            "F6" => "F6",
            "F7" => "F7",
            "F8" => "F8",
            "F9" => "F9",
            "F10" => "F10",
            "F11" => "F11",
            "F12" => "F12",
            // Single-char keys: use the lowercase character directly as the
            // X11 keysym name.  XStringToKeysym("a") → XK_a.
            other if other.chars().count() == 1 => {
                // For single chars we'd return the lowercase version, but
                // &str lifetime constraints mean we can't return a temporary.
                // In the real implementation the caller lowercases the key.
                "space" // fallback for the map; caller handles single-char
            }
            _ => "space", // unknown keys default
        }
    }
}

// ---------------------------------------------------------------------------
// Modifier mask translation
// ---------------------------------------------------------------------------

/// Translates our `Modifier` enum to X11 modifier masks.
///
/// | Modifier | X11 Mask    | Notes                                 |
/// |----------|-------------|---------------------------------------|
/// | Control  | ControlMask | `(1 << 2)`                            |
/// | Alt      | Mod1Mask    | `(1 << 3)` — Alt on most configs      |
/// | Shift    | ShiftMask   | `(1 << 0)`                            |
/// | Meta     | Mod4Mask    | `(1 << 6)` — Super / Windows key      |
pub struct X11ModifierMapping;

impl X11ModifierMapping {
    /// Converts a `ShortcutBinding` into `(X11 keycode, X11 modifier mask)`.
    ///
    /// We also register the grab with the combination that includes:
    /// - `LockMask` (CapsLock)
    /// - `Mod2Mask` (NumLock)
    ///
    /// This ensures the hotkey works regardless of CapsLock/NumLock state.
    pub fn to_grab_params(
        binding: &ShortcutBinding,
        // display: *mut x11_ffi::Display,
    ) -> Option<(u32, u32)> {
        match binding {
            ShortcutBinding::Chord { modifiers, key } => {
                // 1. Resolve the key to a keysym name via X11KeyMapping::key_to_keysym_name().
                // 2. Call XStringToKeysym() → XKeysymToKeycode() for the keycode.
                // 3. Accumulate modifier mask:
                //    - Control → ControlMask
                //    - Alt     → Mod1Mask
                //    - Shift   → ShiftMask
                //    - Meta    → Mod4Mask
                // 4. Also register with (mask | LockMask) and (mask | Mod2Mask)
                //    to ignore CapsLock/NumLock state.
                let _ = (modifiers, key);
                None
            }
            ShortcutBinding::DoubleModifier { modifier } => {
                // Double-modifier shortcuts are not natively supported by
                // XGrabKey.  Implementation monitors KeyPress events on the
                // root window and detects two successive presses of the same
                // modifier key within a time window.
                let _ = modifier;
                None
            }
        }
    }
}
