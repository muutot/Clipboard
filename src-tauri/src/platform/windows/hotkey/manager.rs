//! `HotkeyManager`: the per-action chord plan, loop rebuild, and dispatch thread.

use std::sync::{mpsc, Arc};
use std::thread;

use tauri::{Emitter as _, Manager as _};

use crate::keyboard::{global_action_ids, Modifier};
use crate::platform::hotkey_common::plan_registrations;

use super::message_loop::{
    spawn_hotkey_thread_with_registrations, stop_hotkey_thread, wait_for_hotkey_hwnd_or_exit,
};
use super::paste_target::{foreground_window_handle, QuickPasteTarget};
use super::{HotkeyAction, HotkeyRegistration};

pub struct HotkeyManager {
    handle: Option<thread::JoinHandle<()>>,
    window: Option<tauri::WebviewWindow>,
    /// Chord bindings per global action in `global_action_ids()` order
    /// (index 0 = `toggleWindow`, 1 = `toggleFloatPanel`). A new registry row
    /// automatically extends this vector, so new global shortcuts need no
    /// manager edits — only the plan builder in `lib.rs` reads the registry.
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

    /// Registers float-panel chord bindings on the same shared message loop.
    /// The app handle opens the backend-owned float window without involving
    /// any frontend state; it also forwards future `Forward` actions as
    /// `global-hotkey` events.
    pub fn set_float_hotkeys(&mut self, bindings: Vec<(u32, u32)>, app: tauri::AppHandle) {
        self.ensure_chord_slots();
        if let Some(slot) = self.global_chords.get_mut(1) {
            *slot = bindings;
        }
        self.app = Some(app);
        self.restart_combined_thread();
    }

    /// Starts the shared loop from a full registry-ordered chord plan in one
    /// rebuild. Used once at startup so every global action registers
    /// together instead of restarting the loop per action.
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

    /// (Re)builds the single hotkey thread from every registry action's
    /// stored chords. Stopping first tears down the previous message window,
    /// so re-registration never leaks hotkeys.
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
        let handle = spawn_hotkey_thread_with_registrations(
            registrations,
            self.toggle_doubles.clone(),
            tx,
            self.app.clone(),
            Arc::clone(&self.quick_paste_target),
        );
        let quick_paste_target = Arc::clone(&self.quick_paste_target);
        let app = self.app.clone();

        thread::spawn(move || {
            while let Ok(action) = rx.recv() {
                match action {
                    HotkeyAction::ToggleMain => {
                        let is_visible = window.is_visible().unwrap_or(false);
                        let is_focused = window.is_focused().unwrap_or(false);
                        if !is_focused {
                            if let Some(window_handle) = foreground_window_handle() {
                                quick_paste_target.remember(window_handle);
                            }
                        }
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
                            // Mirror ToggleMain: remember the foreground window
                            // as the quick-paste target so "copy and paste"
                            // from the float panel has a window to restore.
                            // Skip when one of our own windows is focused
                            // (the user is toggling the panel away, and its
                            // handle must not become the paste target).
                            let own_focused = window.is_focused().unwrap_or(false)
                                || app
                                    .get_webview_window(crate::commands::float::FLOAT_WINDOW_LABEL)
                                    .and_then(|panel| panel.is_focused().ok())
                                    .unwrap_or(false);
                            if !own_focused {
                                if let Some(window_handle) = foreground_window_handle() {
                                    quick_paste_target.remember(window_handle);
                                }
                            }
                            if let Err(error) =
                                crate::commands::float::toggle_float_panel(app.clone())
                            {
                                crate::log_error!(
                                    "[hotkey] failed to toggle the float panel: {error}"
                                );
                            }
                        }
                    }
                    HotkeyAction::Forward(index) => {
                        // Future global actions without a native handler are
                        // forwarded as events; listeners need no manager code.
                        let action_id = global_action_ids().nth(index).unwrap_or("unknown");
                        if let Some(app) = app.as_ref() {
                            if let Err(error) = app.emit("global-hotkey", action_id) {
                                crate::log_error!(
                                    "[hotkey] failed to emit global-hotkey {action_id}: {error}"
                                );
                            }
                        } else {
                            crate::log_warn!(
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
        let handle = self.handle.take();
        // The spawn-side readiness wait may time out before the loop thread
        // publishes its hwnd (pathologically slow window creation). Then
        // `stop_hotkey_thread` has no hwnd to post WM_QUIT to and the join
        // below would block forever on a thread stuck in GetMessageW. Wait
        // bounded for the hwnd to appear — or the thread to exit on its own
        // error path — before asking the stop to run; if even that expires,
        // leak the thread so shutdown still proceeds.
        let joinable = handle
            .as_ref()
            .map(wait_for_hotkey_hwnd_or_exit)
            .unwrap_or(true);
        stop_hotkey_thread();
        if let Some(handle) = handle {
            if joinable {
                if let Err(panic) = handle.join() {
                    crate::log_error!(
                        "[hotkey] message loop thread terminated with a panic: {panic:?}"
                    );
                }
            } else {
                crate::log_warn!(
                    "[hotkey] message loop thread never became stoppable; leaking it so shutdown can proceed"
                );
            }
        }
        self.quick_paste_target.clear();
    }
}
