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
    pub fn pressed(&mut self) -> Vec<Option<Modifier>> {
        extern "C" {
            fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
        }
        (0..128)
            .filter(|key| unsafe { CGEventSourceKeyState(1, *key) })
            .map(|key| match key {
                59 | 62 => Some(Modifier::Control),
                56 | 60 => Some(Modifier::Shift),
                58 | 61 => Some(Modifier::Alt),
                54 | 55 => Some(Modifier::Meta),
                _ => None,
            })
            .collect()
    }
}

#[cfg(target_os = "linux")]
pub struct ModifierInput {
    display: *mut super::linux_x11::x11_ffi::Display,
    modifiers: Vec<(u32, Modifier)>,
}
#[cfg(target_os = "linux")]
impl ModifierInput {
    pub fn open() -> Option<Self> {
        use super::linux_x11::x11_ffi::*;
        let display = unsafe { XOpenDisplay(std::ptr::null()) };
        if display.is_null() {
            return None;
        }
        let modifiers = [
            (0xffe1, Modifier::Shift),
            (0xffe2, Modifier::Shift),
            (0xffe3, Modifier::Control),
            (0xffe4, Modifier::Control),
            (0xffe9, Modifier::Alt),
            (0xffea, Modifier::Alt),
            (0xffeb, Modifier::Meta),
            (0xffec, Modifier::Meta),
        ]
        .into_iter()
        .filter_map(|(sym, modifier)| {
            let code = unsafe { XKeysymToKeycode(display, sym) };
            (code != 0).then_some((code, modifier))
        })
        .collect();
        Some(Self { display, modifiers })
    }
    pub fn pressed(&mut self) -> Vec<Option<Modifier>> {
        let mut keys = [0i8; 32];
        unsafe {
            super::linux_x11::x11_ffi::XQueryKeymap(self.display, keys.as_mut_ptr());
        }
        (0u32..256)
            .filter(|key| keys[*key as usize / 8] as u8 & (1 << (*key % 8)) != 0)
            .map(|key| {
                self.modifiers
                    .iter()
                    .find_map(|(code, modifier)| (*code == key).then_some(*modifier))
            })
            .collect()
    }
}
#[cfg(target_os = "linux")]
impl Drop for ModifierInput {
    fn drop(&mut self) {
        unsafe {
            super::linux_x11::x11_ffi::XCloseDisplay(self.display);
        }
    }
}

// Compile the portable manager on Windows in tests; this supplies no native
// input and is not linked into the Windows application.
#[cfg(all(test, target_os = "windows"))]
pub struct ModifierInput;
#[cfg(all(test, target_os = "windows"))]
impl ModifierInput {
    pub fn open() -> Option<Self> {
        None
    }
    pub fn pressed(&mut self) -> Vec<Option<Modifier>> {
        Vec::new()
    }
}
