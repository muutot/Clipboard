//! Key-state sampling for bare modifier double taps. Polling is deliberate:
//! this worker can always stop/join without a permanent keyboard hook thread.
//! Taps shorter than the 10 ms sampling interval are not observable.
use crate::keyboard::Modifier;

#[cfg(target_os = "macos")]
pub struct ModifierInput;
#[cfg(target_os = "macos")]
impl ModifierInput {
    pub fn open() -> Option<Self> {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn CGPreflightListenEventAccess() -> bool;
        }
        unsafe { CGPreflightListenEventAccess().then_some(Self) }
    }
    pub fn pressed(&mut self) -> Option<Vec<Option<Modifier>>> {
        extern "C" {
            fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
        }
        Some(
            (0..128)
                .filter(|key| unsafe { CGEventSourceKeyState(1, *key) })
                .map(|key| match key {
                    59 | 62 => Some(Modifier::Control),
                    56 | 60 => Some(Modifier::Shift),
                    58 | 61 => Some(Modifier::Alt),
                    54 | 55 => Some(Modifier::Meta),
                    _ => None,
                })
                .collect(),
        )
    }
}

#[cfg(any(test, target_os = "linux"))]
pub mod x11_input {
    use super::Modifier;
    use x11rb::{
        connection::Connection, protocol::xproto::ConnectionExt, rust_connection::RustConnection,
    };
    pub struct ModifierInput {
        conn: RustConnection,
        modifiers: Vec<(u8, Modifier)>,
    }
    impl ModifierInput {
        pub fn open() -> Option<Self> {
            let (conn, _) = x11rb::connect(None).ok()?;
            let first = conn.setup().min_keycode;
            let mapping = conn
                .get_keyboard_mapping(first, conn.setup().max_keycode - first + 1)
                .ok()?
                .reply()
                .ok()?;
            let stride = mapping.keysyms_per_keycode as usize;
            if stride == 0 {
                return None;
            }
            let mut modifiers = Vec::new();
            for (index, keys) in mapping.keysyms.chunks(stride).enumerate() {
                for key in keys {
                    let modifier = match key {
                        0xffe1 | 0xffe2 => Modifier::Shift,
                        0xffe3 | 0xffe4 => Modifier::Control,
                        0xffe9 | 0xffea => Modifier::Alt,
                        0xffeb | 0xffec => Modifier::Meta,
                        _ => continue,
                    };
                    modifiers.push((first + index as u8, modifier));
                    break;
                }
            }
            Some(Self { conn, modifiers })
        }
        pub fn pressed(&mut self) -> Option<Vec<Option<Modifier>>> {
            let keys = self.conn.query_keymap().ok()?.reply().ok()?.keys;
            Some(
                (0u16..256)
                    .filter(|key| keys[*key as usize / 8] & (1 << (*key % 8)) != 0)
                    .map(|key| {
                        self.modifiers
                            .iter()
                            .find_map(|(code, modifier)| (*code as u16 == key).then_some(*modifier))
                    })
                    .collect(),
            )
        }
    }
}
#[cfg(target_os = "linux")]
pub use x11_input::ModifierInput;

// Compile the portable manager on Windows in tests; this supplies no native
// input and is not linked into the Windows application.
#[cfg(all(test, target_os = "windows"))]
pub struct ModifierInput;
#[cfg(all(test, target_os = "windows"))]
impl ModifierInput {
    pub fn open() -> Option<Self> {
        None
    }
    pub fn pressed(&mut self) -> Option<Vec<Option<Modifier>>> {
        None
    }
}
