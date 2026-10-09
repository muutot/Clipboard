//! Target identity and platform quick-paste backends.
use std::{
    fmt, thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub handle: isize,
    pub pid: u32,
}
impl fmt::UpperHex for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.handle, f)
    }
}
impl Target {
    fn external(self) -> Option<Self> {
        (self.handle > 0 && self.pid != 0 && self.pid != std::process::id()).then_some(self)
    }
}

fn wait_for_target(
    target: Target,
    mut current: impl FnMut() -> Option<Target>,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(400);
    loop {
        if current() == Some(target) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("previous application did not regain focus".into());
        }
        thread::sleep(Duration::from_millis(15));
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::platform::macos::objc;
    pub fn current() -> Option<Target> {
        let pool = unsafe { objc::objc_autoreleasePoolPush() };
        let app = objc::workspace_frontmost_app(objc::get_nsworkspace());
        let pid = unsafe {
            objc::msgSend_isize(app, objc::sel_registerName(c"processIdentifier".as_ptr()))
        } as u32;
        unsafe { objc::objc_autoreleasePoolPop(pool) };
        Target {
            handle: pid as isize,
            pid,
        }
        .external()
    }
    pub fn paste(target: Target) -> Result<(), String> {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
            fn CGEventCreateKeyboardEvent(
                source: *mut std::ffi::c_void,
                key: u16,
                down: bool,
            ) -> *mut std::ffi::c_void;
            fn CGEventSetFlags(event: *mut std::ffi::c_void, flags: u64);
            fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
            fn CGEventSourceFlagsState(state: i32) -> u64;
            fn CFRelease(value: *mut std::ffi::c_void);
        }
        if target.external().is_none() || !unsafe { AXIsProcessTrusted() } {
            return Err("Accessibility permission is required for quick paste".into());
        }
        let pool = unsafe { objc::objc_autoreleasePoolPush() };
        let activated = unsafe {
            let class = objc::objc_getClass(c"NSRunningApplication".as_ptr());
            let app = objc::msgSend_i32_Id(
                class,
                objc::sel_registerName(c"runningApplicationWithProcessIdentifier:".as_ptr()),
                target.pid as i32,
            );
            if app.is_null() {
                false
            } else {
                objc::msgSend_bool_usize(
                    app,
                    objc::sel_registerName(c"activateWithOptions:".as_ptr()),
                    2,
                ) != 0
            }
        };
        unsafe { objc::objc_autoreleasePoolPop(pool) };
        if !activated {
            return Err("previous application is unavailable".into());
        }
        wait_for_target(target, current)?;
        let deadline = Instant::now() + Duration::from_millis(400);
        while unsafe { CGEventSourceFlagsState(1) } & 0x1e0000 != 0 {
            if Instant::now() >= deadline {
                return Err("release modifier keys before pasting".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        if current() != Some(target) {
            return Err("paste target lost focus".into());
        }
        unsafe {
            let down = CGEventCreateKeyboardEvent(std::ptr::null_mut(), 9, true);
            let up = CGEventCreateKeyboardEvent(std::ptr::null_mut(), 9, false);
            if down.is_null() || up.is_null() {
                if !down.is_null() {
                    CFRelease(down);
                }
                if !up.is_null() {
                    CFRelease(up);
                }
                return Err("could not create paste key events".into());
            }
            CGEventSetFlags(down, 1 << 20);
            CGEventSetFlags(up, 1 << 20);
            CGEventPost(0, down);
            CGEventPost(0, up);
            CFRelease(down);
            CFRelease(up);
        }
        Ok(())
    }
}

#[cfg(any(test, target_os = "linux"))]
#[cfg_attr(all(test, not(target_os = "linux")), allow(dead_code))]
mod x11 {
    use super::*;
    use x11rb::{
        connection::Connection,
        protocol::{xproto::*, xtest::ConnectionExt as _},
        rust_connection::RustConnection,
    };
    fn property(conn: &RustConnection, window: u32, name: &[u8]) -> Option<u32> {
        let atom = conn.intern_atom(false, name).ok()?.reply().ok()?.atom;
        conn.get_property(false, window, atom, AtomEnum::ANY, 0, 1)
            .ok()?
            .reply()
            .ok()?
            .value32()?
            .next()
    }
    fn focused(conn: &RustConnection, root: u32) -> Option<Target> {
        let handle = property(conn, root, b"_NET_ACTIVE_WINDOW")?;
        let pid = property(conn, handle, b"_NET_WM_PID")?;
        Target {
            handle: handle as isize,
            pid,
        }
        .external()
    }
    pub fn current() -> Option<Target> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        focused(&conn, conn.setup().roots[screen].root)
    }
    pub fn paste(target: Target) -> Result<(), String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen].root;
        if target.external().is_none()
            || property(&conn, target.handle as u32, b"_NET_WM_PID") != Some(target.pid)
        {
            return Err("previous window is unavailable or its owner changed".into());
        }
        let atom = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom;
        let event = ClientMessageEvent::new(32, target.handle as u32, atom, [2, 0, 0, 0, 0]);
        conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(|e| e.to_string())?
        .check()
        .map_err(|e| e.to_string())?;
        wait_for_target(target, || focused(&conn, root))?;
        let deadline = Instant::now() + Duration::from_millis(400);
        loop {
            let keys = conn
                .query_keymap()
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            if keys.keys.iter().all(|value| *value == 0) {
                break;
            }
            if Instant::now() >= deadline {
                return Err("release keys before pasting".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        let first = conn.setup().min_keycode;
        let count = conn.setup().max_keycode - first + 1;
        let mapping = conn
            .get_keyboard_mapping(first, count)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let stride = mapping.keysyms_per_keycode as usize;
        if stride == 0 {
            return Err("empty keyboard mapping".into());
        }
        let keycode = |sym| {
            mapping
                .keysyms
                .chunks(stride)
                .position(|keys| keys.contains(&sym))
                .map(|index| first + index as u8)
                .ok_or_else(|| "paste key unavailable".to_string())
        };
        let ctrl = keycode(0xffe3)?;
        let v = keycode(b'v' as u32)?;
        if focused(&conn, root) != Some(target) {
            return Err("paste target lost focus".into());
        }
        let fake = |kind, code| {
            conn.xtest_fake_input(kind, code, 0, root, 0, 0, 0)
                .map_err(|e| e.to_string())?
                .check()
                .map_err(|e| e.to_string())
        };
        let result = fake(KEY_PRESS_EVENT, ctrl).and_then(|_| fake(KEY_PRESS_EVENT, v));
        let release_v = fake(KEY_RELEASE_EVENT, v);
        let release_ctrl = fake(KEY_RELEASE_EVENT, ctrl);
        result.and(release_v).and(release_ctrl)
    }
}

#[cfg(target_os = "macos")]
pub use mac::{current, paste};
#[cfg(target_os = "linux")]
pub fn current() -> Option<Target> {
    if super::Platform::detect().is_wayland() {
        super::wayland_paste::current()
    } else {
        x11::current()
    }
}
#[cfg(target_os = "linux")]
pub fn paste(target: Target) -> Result<(), String> {
    if super::Platform::detect().is_wayland() {
        super::wayland_paste::paste(target)
    } else {
        x11::paste(target)
    }
}
#[cfg(all(test, target_os = "windows"))]
pub fn current() -> Option<Target> {
    None
}
#[cfg(all(test, target_os = "windows"))]
pub fn paste(_: Target) -> Result<(), String> {
    Err("test-only portable compile adapter".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_self_and_matches_both_window_and_owner() {
        assert!(Target {
            handle: 1,
            pid: std::process::id()
        }
        .external()
        .is_none());
        let wanted = Target {
            handle: 5,
            pid: 123,
        };
        let mut reads = 0;
        wait_for_target(wanted, || {
            reads += 1;
            Some(if reads == 1 {
                Target {
                    handle: 5,
                    pid: 124,
                }
            } else {
                wanted
            })
        })
        .unwrap();
        assert_eq!(reads, 2);
    }
}
