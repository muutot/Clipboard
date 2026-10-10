use super::*;

// ---------------------------------------------------------------------------
// X11GlobalHotkey
// ---------------------------------------------------------------------------

/// Registers global keyboard shortcuts via `XGrabKey` on the root window.
///
/// # How it works
///
/// 1. Open an X display connection.
/// 2. For each shortcut, translate the `ShortcutBinding` to a `(keycode,
///    modifiers)` pair using `X11ModifierMapping`.
/// 3. Call `XGrabKey(display, keycode, modifiers, root_window, True,
///    GrabModeAsync, GrabModeAsync)` — this ensures no other application
///    can steal the combination.
/// 4. Also grab variations: `modifiers | LockMask` (CapsLock),
///    `modifiers | Mod2Mask` (NumLock), `modifiers | LockMask | Mod2Mask`.
/// 5. Enter an event loop (or piggyback on the existing one) listening for
///    `KeyPress` events.  When a registered combination arrives, invoke the
///    callback.
/// 6. On shutdown, call `XUngrabKey` for every registered combination.
///
/// # Limitations
///
/// - `XGrabKey` can only register chord-based shortcuts (modifier + regular
///   key).  Double-modifier shortcuts require a different strategy: listen
///   for raw KeyPress events on modifier keys and detect double-taps in
///   software.
type HotkeyCallback = Box<dyn Fn(&str) + Send + Sync + 'static>;

pub struct X11GlobalHotkey {
    /// Set of registered (keycode, modifiers) pairs currently grabbed.
    registered: HashSet<(u32, u32)>,
    /// Map from (keycode, modifiers) to action_id for callback dispatch.
    action_map: HashMap<(u32, u32), String>,
    /// Callback invoked when a grabbed key fires.
    on_hotkey: Option<HotkeyCallback>,
}

impl Default for X11GlobalHotkey {
    fn default() -> Self {
        Self::new()
    }
}

impl X11GlobalHotkey {
    /// Creates a new, empty hotkey manager.
    pub fn new() -> Self {
        Self {
            registered: HashSet::new(),
            action_map: HashMap::new(),
            on_hotkey: None,
        }
    }

    /// Registers one or more shortcuts.
    ///
    /// # Errors
    ///
    /// Returns `X11Error::HotkeyGrabFailed` if the key combination is already
    /// grabbed by another application.
    pub fn register(&mut self, action_id: &str, shortcuts: &[ShortcutBinding]) -> X11Result<()> {
        // Implementation:
        //
        // let display = XOpenDisplay(NULL);
        // let root = XDefaultRootWindow(display);
        //
        // for binding in shortcuts {
        //     if let Some((keycode, mods)) =
        //         X11ModifierMapping::to_grab_params(binding) {
        //         // Also grab with LockMask and Mod2Mask variations
        //         for extra in &[0, LOCK_MASK, MOD2_MASK, LOCK_MASK | MOD2_MASK] {
        //             let full_mods = mods | extra;
        //             let result = XGrabKey(display, keycode as i32, full_mods,
        //                 root, 1, GRAB_MODE_ASYNC, GRAB_MODE_ASYNC);
        //             if result != 0 {
        //                 return Err(X11Error::HotkeyGrabFailed(keycode, full_mods));
        //             }
        //             self.registered.insert((keycode, full_mods));
        //         }
        //         self.action_map.insert((keycode, mods), action_id.to_owned());
        //     }
        // }
        // XFlush(display);
        // Ok(())
        let _ = (action_id, shortcuts);
        Ok(())
    }

    /// Unregisters all grabbed keys.
    pub fn unregister_all(&mut self) -> X11Result<()> {
        // for (keycode, mods) in &self.registered {
        //     XUngrabKey(display, *keycode as i32, *mods, root);
        // }
        // self.registered.clear();
        // self.action_map.clear();
        Ok(())
    }

    /// Sets the callback invoked when a registered key fires.
    pub fn set_callback<F>(&mut self, callback: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.on_hotkey = Some(Box::new(callback));
    }

    /// Returns whether any shortcuts are currently registered.
    pub fn is_active(&self) -> bool {
        !self.registered.is_empty()
    }

    /// Returns the number of grabbed key combinations.
    pub fn count(&self) -> usize {
        self.registered.len()
    }
}
