#![allow(dead_code)]

use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::windows_clipboard;
use crate::keyboard::{Modifier, DEFAULT_DOUBLE_TAP_INTERVAL_MS};

type HotkeyRegistration = (i32, u32, u32);

#[derive(Default)]
struct QuickPasteTarget {
    window_handle: Mutex<Option<isize>>,
}

impl QuickPasteTarget {
    fn remember(&self, window_handle: isize) {
        if window_handle == 0 {
            return;
        }
        if let Ok(mut target) = self.window_handle.lock() {
            *target = Some(window_handle);
        }
    }

    fn take(&self) -> Option<isize> {
        self.window_handle
            .lock()
            .ok()
            .and_then(|mut target| target.take())
    }

    fn clear(&self) {
        if let Ok(mut target) = self.window_handle.lock() {
            *target = None;
        }
    }
}

fn modifier_from_virtual_key(virtual_key: u32) -> Option<Modifier> {
    match virtual_key {
        0x10 => Some(Modifier::Shift),
        0x11 => Some(Modifier::Control),
        0x12 => Some(Modifier::Alt),
        0x5B => Some(Modifier::Meta),
        _ => None,
    }
}

struct DoubleModifierTracker {
    registered: BTreeSet<Modifier>,
    active_press: Option<Modifier>,
    last_tap: Option<(Modifier, u64)>,
    press_interrupted: bool,
    double_tap_interval_ms: u64,
}

impl DoubleModifierTracker {
    fn new(modifiers: impl IntoIterator<Item = Modifier>) -> Self {
        Self {
            registered: modifiers.into_iter().collect(),
            active_press: None,
            last_tap: None,
            press_interrupted: false,
            double_tap_interval_ms: DEFAULT_DOUBLE_TAP_INTERVAL_MS,
        }
    }

    fn on_key_event(&mut self, virtual_key: u32, is_key_down: bool, timestamp_ms: u64) -> bool {
        let Some(modifier) = modifier_from_virtual_key(virtual_key) else {
            if is_key_down {
                self.press_interrupted = self.active_press.is_some();
                self.last_tap = None;
            }
            return false;
        };

        if is_key_down {
            if self.active_press == Some(modifier) {
                return false;
            }
            if self.active_press.is_some() {
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
                    && timestamp_ms >= previous_timestamp
                    && timestamp_ms - previous_timestamp <= self.double_tap_interval_ms
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

fn deduplicate_hotkeys(bindings: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut seen = HashSet::new();
    bindings
        .iter()
        .copied()
        .filter(|binding| seen.insert(*binding))
        .collect()
}

fn assign_hotkey_ids(bindings: &[(u32, u32)]) -> Vec<HotkeyRegistration> {
    deduplicate_hotkeys(bindings)
        .into_iter()
        .enumerate()
        .map(|(index, (modifiers, vk))| (1 + index as i32, modifiers, vk))
        .collect()
}

static HOTKEY_STOP: AtomicBool = AtomicBool::new(false);

pub fn set_hotkey_sender(_tx: &mpsc::Sender<()>) {}

pub fn set_hotkey_hwnd(_hwnd: isize) {}

pub fn clear_hotkey_state() {}

pub fn stop_hotkey_thread() {
    HOTKEY_STOP.store(true, Ordering::SeqCst);
}

fn spawn_hotkey_thread_with_registrations(
    _registrations: Vec<HotkeyRegistration>,
    _double_modifiers: Vec<Modifier>,
    tx: mpsc::Sender<()>,
) -> thread::JoinHandle<()> {
    HOTKEY_STOP.store(false, Ordering::SeqCst);
    thread::spawn(move || {
        while !HOTKEY_STOP.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(200));
        }
        drop(tx);
    })
}

pub fn spawn_hotkey_thread_with_hotkeys(
    bindings: Vec<(u32, u32)>,
    double_modifiers: Vec<Modifier>,
    tx: mpsc::Sender<()>,
) -> thread::JoinHandle<()> {
    spawn_hotkey_thread_with_registrations(assign_hotkey_ids(&bindings), double_modifiers, tx)
}

pub struct HotkeyManager {
    handle: Option<thread::JoinHandle<()>>,
    window: Option<tauri::WebviewWindow>,
    quick_paste_target: Arc<QuickPasteTarget>,
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self {
            handle: None,
            window: None,
            quick_paste_target: Arc::new(QuickPasteTarget::default()),
        }
    }

    pub fn start_with_window(&mut self, modifiers: u32, vk: u32, window: tauri::WebviewWindow) {
        self.start_with_bindings(vec![(modifiers, vk)], window);
    }

    pub fn start_with_bindings(&mut self, bindings: Vec<(u32, u32)>, window: tauri::WebviewWindow) {
        self.start_with_hotkeys(bindings, Vec::new(), window);
    }

    pub fn start_with_hotkeys(
        &mut self,
        bindings: Vec<(u32, u32)>,
        double_modifiers: Vec<Modifier>,
        window: tauri::WebviewWindow,
    ) {
        self.stop();
        self.window = Some(window.clone());
        let (tx, rx) = mpsc::channel::<()>();
        let handle = spawn_hotkey_thread_with_hotkeys(bindings, double_modifiers, tx);
        let _quick_paste_target = Arc::clone(&self.quick_paste_target);

        thread::spawn(move || {
            while let Ok(()) = rx.recv() {
                let is_visible = window.is_visible().unwrap_or(false);
                let is_focused = window.is_focused().unwrap_or(false);
                if is_visible && is_focused {
                    let _ = window.hide();
                } else {
                    if !is_visible {
                        let _ = window.show();
                    }
                    if !is_focused {
                        let _ = window.set_focus();
                    }
                }
            }
        });

        self.handle = Some(handle);
    }

    pub fn restart_with_hotkeys(
        &mut self,
        bindings: Vec<(u32, u32)>,
        double_modifiers: Vec<Modifier>,
    ) {
        if let Some(window) = self.window.clone() {
            self.start_with_hotkeys(bindings, double_modifiers, window);
        }
    }

    pub fn take_quick_paste_target(&self) -> Option<isize> {
        self.quick_paste_target.take()
    }

    /// Records the current foreground window as the quick-paste target so the
    /// main window can restore it when it is shown from the tray or another
    /// entry point (the toggle hotkey path records it inside its own loop).
    pub fn remember_foreground(&self) {
        if let Some(window_handle) = foreground_window_handle() {
            self.quick_paste_target.remember(window_handle);
        }
    }

    pub fn stop(&mut self) {
        stop_hotkey_thread();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.quick_paste_target.clear();
    }
}

fn foreground_window_handle() -> Option<isize> {
    None
}

pub fn restore_window_and_paste(_window_handle: isize) -> Result<(), String> {
    Err("quick paste is only implemented on Windows".to_owned())
}

fn windows_virtual_key(key: &str) -> Option<u32> {
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

pub fn shortcut_to_windows_hotkey(
    binding: &crate::keyboard::ShortcutBinding,
) -> Option<(u32, u32)> {
    match binding {
        crate::keyboard::ShortcutBinding::Chord { modifiers, key } => {
            let mut mod_flags: u32 = 0;
            for m in modifiers {
                match m {
                    crate::keyboard::Modifier::Alt => mod_flags |= windows_clipboard::MOD_ALT,
                    crate::keyboard::Modifier::Control => {
                        mod_flags |= windows_clipboard::MOD_CONTROL
                    }
                    crate::keyboard::Modifier::Shift => mod_flags |= windows_clipboard::MOD_SHIFT,
                    crate::keyboard::Modifier::Meta => mod_flags |= windows_clipboard::MOD_WIN,
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
