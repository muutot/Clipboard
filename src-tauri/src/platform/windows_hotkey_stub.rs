#![allow(dead_code)]

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::Emitter as _;

use super::hotkey_common::{action_index_for_hotkey_id, plan_registrations};
pub use super::hotkey_common::{
    assign_hotkey_ids, combined_hotkey_registrations, shortcut_bindings_to_double_modifiers,
    shortcut_bindings_to_windows_hotkeys, HotkeyRegistration, FIRST_HOTKEY_ID,
    FLOAT_HOTKEY_ID_BASE,
};
use crate::keyboard::{global_action_ids, Modifier, DEFAULT_DOUBLE_TAP_INTERVAL_MS};

/// OS hotkey action selected by the fired registration id. Mirrors
/// `windows_hotkey.rs`; the stub thread never fires, but the routing stays
/// identical so behavior differs only in OS registration, not in dispatch.
/// `Forward` carries later registry actions as `global-hotkey` events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleMain,
    ToggleFloat,
    Forward(usize),
}

pub fn action_for_hotkey_id(id: i32) -> HotkeyAction {
    match action_index_for_hotkey_id(id) {
        None | Some(0) => HotkeyAction::ToggleMain,
        Some(1) => HotkeyAction::ToggleFloat,
        Some(index) => HotkeyAction::Forward(index),
    }
}

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

static HOTKEY_STOP: AtomicBool = AtomicBool::new(false);

pub fn set_hotkey_sender(_tx: &mpsc::Sender<HotkeyAction>) {}

pub fn set_hotkey_hwnd(_hwnd: isize) {}

pub fn clear_hotkey_state() {}

pub fn stop_hotkey_thread() {
    HOTKEY_STOP.store(true, Ordering::SeqCst);
}

fn spawn_hotkey_thread_with_registrations(
    _registrations: Vec<HotkeyRegistration>,
    _double_modifiers: Vec<Modifier>,
    tx: mpsc::Sender<HotkeyAction>,
) -> thread::JoinHandle<()> {
    HOTKEY_STOP.store(false, Ordering::SeqCst);
    thread::spawn(move || {
        while !HOTKEY_STOP.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(200));
        }
        drop(tx);
    })
}

pub struct HotkeyManager {
    handle: Option<thread::JoinHandle<()>>,
    window: Option<tauri::WebviewWindow>,
    /// Chord bindings per global action in `global_action_ids()` order.
    /// Mirrors `windows_hotkey.rs`; a new registry row extends this vector
    /// without manager edits.
    global_chords: Vec<Vec<(u32, u32)>>,
    toggle_doubles: Vec<Modifier>,
    app: Option<tauri::AppHandle>,
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
            global_chords: Vec::new(),
            toggle_doubles: Vec::new(),
            app: None,
            quick_paste_target: Arc::new(QuickPasteTarget::default()),
        }
    }

    fn ensure_chord_slots(&mut self) {
        let want = global_action_ids().count();
        if self.global_chords.len() < want {
            self.global_chords.resize(want, Vec::new());
        }
    }

    /// Sets chord bindings for any registry action by id and rebuilds the
    /// shared loop. This is the only method new global actions need.
    pub fn set_action_chords(&mut self, action: &str, bindings: Vec<(u32, u32)>) {
        self.ensure_chord_slots();
        if let Some(index) = global_action_ids().position(|id| id == action) {
            self.global_chords[index] = bindings;
        }
        self.restart_combined_thread();
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
        self.ensure_chord_slots();
        if let Some(slot) = self.global_chords.first_mut() {
            *slot = bindings;
        }
        self.toggle_doubles = double_modifiers;
        self.window = Some(window.clone());
        self.restart_combined_thread();
    }

    pub fn set_float_hotkeys(&mut self, bindings: Vec<(u32, u32)>, app: tauri::AppHandle) {
        self.ensure_chord_slots();
        if let Some(slot) = self.global_chords.get_mut(1) {
            *slot = bindings;
        }
        self.app = Some(app);
        self.restart_combined_thread();
    }

    /// Starts the shared loop from a full registry-ordered chord plan in one
    /// rebuild. Mirrors `windows_hotkey.rs`; the stub thread never fires, but
    /// plan handling stays identical.
    pub fn start_with_plan(
        &mut self,
        chords: Vec<Vec<(u32, u32)>>,
        double_modifiers: Vec<Modifier>,
        window: tauri::WebviewWindow,
        app: tauri::AppHandle,
    ) {
        self.global_chords = chords;
        self.ensure_chord_slots();
        self.toggle_doubles = double_modifiers;
        self.window = Some(window);
        self.app = Some(app);
        self.restart_combined_thread();
    }

    /// Applies a full registry-ordered chord plan plus the app handle in one
    /// rebuild. Used by `refresh_hotkey_registrations` after any keyboard
    /// config change; without a window the loop stays stopped.
    pub fn apply_global_plan(
        &mut self,
        chords: Vec<Vec<(u32, u32)>>,
        double_modifiers: Vec<Modifier>,
        app: tauri::AppHandle,
    ) {
        self.global_chords = chords;
        self.ensure_chord_slots();
        self.toggle_doubles = double_modifiers;
        self.app = Some(app);
        self.restart_combined_thread();
    }

    fn restart_combined_thread(&mut self) {
        self.stop();
        let Some(window) = self.window.clone() else {
            return;
        };
        self.ensure_chord_slots();
        let slices: Vec<&[(u32, u32)]> = self.global_chords.iter().map(Vec::as_slice).collect();
        let plan = plan_registrations(&slices);
        if plan.is_empty() && self.toggle_doubles.is_empty() {
            return;
        }
        let registrations: Vec<HotkeyRegistration> = plan
            .iter()
            .map(|registration| {
                (
                    registration.id,
                    registration.modifiers,
                    registration.virtual_key,
                )
            })
            .collect();
        let (tx, rx) = mpsc::channel::<HotkeyAction>();
        let handle =
            spawn_hotkey_thread_with_registrations(registrations, self.toggle_doubles.clone(), tx);
        let _quick_paste_target = Arc::clone(&self.quick_paste_target);
        let app = self.app.clone();

        thread::spawn(move || {
            while let Ok(action) = rx.recv() {
                match action {
                    HotkeyAction::ToggleMain => {
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
                    HotkeyAction::ToggleFloat => {
                        if let Some(app) = app.as_ref() {
                            let _ = crate::commands::float::toggle_float_panel(app.clone());
                        }
                    }
                    HotkeyAction::Forward(index) => {
                        // Future global actions without a native handler are
                        // forwarded as events; listeners need no manager code.
                        let action_id = global_action_ids().nth(index).unwrap_or("unknown");
                        if let Some(app) = app.as_ref() {
                            let _ = app.emit("global-hotkey", action_id);
                        } else {
                            crate::log_event!(
                                "[hotkey] no app handle to forward global action {action_id}"
                            );
                        }
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
