//! macOS/X11 global chords and sampled modifier taps.
use crate::keyboard::{Modifier, DEFAULT_DOUBLE_TAP_INTERVAL_MS};
use global_hotkey::hotkey::HotKey;

pub fn report_registration_failure(app: &tauri::AppHandle, action: &str, error: &str) {
    use tauri::Emitter;
    crate::log_warn!("[hotkey] {action}: {error}");
    if let Err(error) = app.emit(
        "hotkey-registration-failed",
        serde_json::json!({"action": action, "error": error}),
    ) {
        crate::log_warn!("[hotkey] failed to report registration failure: {error}");
    }
}

pub fn native_chord(modifiers: u32, key: u32) -> Result<HotKey, String> {
    let key = match key {
        0x30..=0x39 => format!("Digit{}", char::from_u32(key).unwrap()),
        0x41..=0x5a => format!("Key{}", char::from_u32(key).unwrap()),
        0x70..=0x87 => format!("F{}", key - 0x6f),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0d => "Enter".into(),
        0x1b => "Escape".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "ArrowLeft".into(),
        0x26 => "ArrowUp".into(),
        0x27 => "ArrowRight".into(),
        0x28 => "ArrowDown".into(),
        0x2d => "Insert".into(),
        0x2e => "Delete".into(),
        _ => return Err(format!("unsupported shortcut key {key}")),
    };
    let mut chord = String::new();
    for (bit, label) in [(1, "Alt+"), (2, "Control+"), (4, "Shift+"), (8, "Super+")] {
        if modifiers & bit != 0 {
            chord.push_str(label);
        }
    }
    chord.push_str(&key);
    chord
        .parse()
        .map_err(|error: global_hotkey::hotkey::HotKeyParseError| error.to_string())
}

pub struct TapTracker {
    registered: Vec<Modifier>,
    active: Option<Modifier>,
    interrupted: bool,
    last: Option<(Modifier, u64)>,
}
impl TapTracker {
    pub fn new(registered: Vec<Modifier>) -> Self {
        Self {
            registered,
            active: None,
            interrupted: true,
            last: None,
        }
    }
    /// `None` in a nonempty sample is a non-modifier key. Chords and overlapping
    /// modifiers cancel both the current press and the preceding tap.
    pub fn sample(&mut self, pressed: &[Option<Modifier>], time: u64) -> bool {
        if pressed.is_empty() {
            let active = self.active.take();
            let clean = !self.interrupted;
            self.interrupted = false;
            if clean {
                if let Some(modifier) = active {
                    let fire = self.registered.contains(&modifier)
                        && self.last.is_some_and(|(previous, at)| {
                            previous == modifier
                                && time >= at
                                && time - at <= DEFAULT_DOUBLE_TAP_INTERVAL_MS
                        });
                    self.last = if fire { None } else { Some((modifier, time)) };
                    return fire;
                }
            }
        } else if pressed.len() == 1 && pressed[0].is_some() && !self.interrupted {
            if self.active.is_some() && self.active != pressed[0] {
                self.interrupted = true;
                self.last = None;
            } else {
                self.active = pressed[0];
            }
        } else {
            self.interrupted = true;
            self.last = None;
        }
        false
    }
}

#[cfg(any(test, not(target_os = "windows")))]
mod runtime {
    use super::*;
    use crate::platform::hotkey_common::HotkeyRegistration;
    #[cfg(not(target_os = "windows"))]
    use crate::platform::windows::hotkey::{action_for_hotkey_id, HotkeyAction};
    #[cfg(all(test, target_os = "windows"))]
    use crate::platform::windows::portable_hotkey_compile_test::{
        action_for_hotkey_id, HotkeyAction,
    };
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use std::{
        cell::RefCell,
        collections::HashMap,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc, Mutex,
        },
        thread,
        time::{Duration, Instant},
    };

    struct MainState {
        generation: Arc<AtomicBool>,
        manager: GlobalHotKeyManager,
    }
    thread_local! { static NATIVE: RefCell<Option<MainState>> = const { RefCell::new(None) }; }

    pub fn start(
        registrations: Vec<HotkeyRegistration>,
        doubles: Vec<Modifier>,
        tx: mpsc::Sender<HotkeyAction>,
        app: tauri::AppHandle,
        stop: Arc<AtomicBool>,
    ) -> thread::JoinHandle<()> {
        let mapping = Arc::new(Mutex::new(HashMap::new()));
        let mapping_for_main = Arc::clone(&mapping);
        let generation = Arc::clone(&stop);
        let app_for_main = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            if generation.load(Ordering::SeqCst) {
                return;
            }
            NATIVE.with(|state| {
                // Manager construction, registration, and destruction all stay
                // on the UI thread, as required by the Carbon event handler.
                state.borrow_mut().take();
                while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
                let manager = match GlobalHotKeyManager::new() {
                    Ok(manager) => manager,
                    Err(error) => {
                        crate::log_error!("[hotkey] native manager: {error}");
                        return;
                    }
                };
                for (id, modifiers, key) in registrations {
                    let result = native_chord(modifiers, key).and_then(|chord| {
                        manager.register(chord).map_err(|e| e.to_string())?;
                        mapping_for_main
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(chord.id(), id);
                        Ok(())
                    });
                    if let Err(error) = result {
                        let index = crate::platform::hotkey_common::action_index_for_hotkey_id(id)
                            .unwrap_or(0);
                        let action = crate::keyboard::global_action_ids()
                            .nth(index)
                            .unwrap_or("unknown");
                        report_registration_failure(&app_for_main, action, &error);
                    }
                }
                *state.borrow_mut() = Some(MainState {
                    generation,
                    manager,
                });
            });
        }) {
            crate::log_error!("[hotkey] failed to schedule registrations: {error}");
        }

        thread::spawn(move || {
            let mut input = if doubles.is_empty() {
                None
            } else {
                crate::platform::modifier_input::ModifierInput::open()
            };
            if !doubles.is_empty() && input.is_none() {
                report_registration_failure(
                    &app,
                    "toggleWindow",
                    "Modifier monitoring unavailable; macOS requires Input Monitoring permission",
                );
            }
            let mut taps = TapTracker::new(doubles);
            let started = Instant::now();
            while !stop.load(Ordering::SeqCst) {
                while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                    if event.state == HotKeyState::Pressed {
                        if let Some(id) = mapping
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get(&event.id)
                            .copied()
                        {
                            let _ = tx.send(action_for_hotkey_id(id));
                        }
                    }
                }
                if let Some(device) = input.as_mut() {
                    if let Some(pressed) = device.pressed() {
                        if taps.sample(&pressed, started.elapsed().as_millis() as u64) {
                            let _ = tx.send(HotkeyAction::ToggleMain);
                        }
                    } else {
                        report_registration_failure(
                            &app,
                            "toggleWindow",
                            "Modifier input disconnected; double-tap monitoring stopped",
                        );
                        input = None;
                    }
                }
                thread::sleep(Duration::from_millis(10));
            }
            // Never wait for the main thread while HotkeyManager::stop joins us.
            if let Err(error) = app.run_on_main_thread(move || {
                NATIVE.with(|state| {
                    if state
                        .borrow()
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(&current.generation, &stop))
                    {
                        if let Some(current) = state.borrow_mut().take() {
                            drop(current.manager);
                        }
                    }
                })
            }) {
                crate::log_warn!("[hotkey] failed to schedule native cleanup: {error}");
            }
        })
    }
}
#[cfg(any(test, not(target_os = "windows")))]
pub use runtime::start;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_supported_keys_and_modifiers_without_dropping_flags() {
        let chord = native_chord(15, 0x41).unwrap();
        assert_eq!(
            chord.mods,
            global_hotkey::hotkey::Modifiers::ALT
                | global_hotkey::hotkey::Modifiers::CONTROL
                | global_hotkey::hotkey::Modifiers::SHIFT
                | global_hotkey::hotkey::Modifiers::SUPER
        );
        for key in [
            0x08, 0x09, 0x0d, 0x1b, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2d,
            0x2e, 0x30, 0x39, 0x41, 0x5a, 0x70, 0x87,
        ] {
            assert!(native_chord(0, key).is_ok());
        }
        assert!(native_chord(0, 0).is_err());
    }
    #[test]
    fn taps_require_two_clean_releases_and_cancel_on_chords() {
        let mut tracker = TapTracker::new(vec![Modifier::Control]);
        let ctrl = [Some(Modifier::Control)];
        tracker.sample(&[], 0);
        tracker.sample(&ctrl, 10);
        assert!(!tracker.sample(&[], 20));
        tracker.sample(&ctrl, 30);
        assert!(tracker.sample(&[], 40));
        tracker.sample(&ctrl, 50);
        tracker.sample(&[Some(Modifier::Control), None], 60);
        tracker.sample(&[], 70);
        tracker.sample(&ctrl, 80);
        assert!(!tracker.sample(&[], 90));
        tracker.sample(&ctrl, 1000);
        assert!(!tracker.sample(&[], 1010));
    }
}
