//! A batch owns one take-once target; later calls cannot retarget it.
use crate::commands::lock::lock_state;
use crate::platform::windows::hotkey::HotkeyManager;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
type Target = crate::platform::windows::hotkey::QuickPasteHandle;
#[cfg(not(target_os = "windows"))]
type Target = crate::platform::quick_paste::Target;

struct Batch {
    token: String,
    target: Target,
    created: Instant,
    touched: Instant,
    started: bool,
    busy: bool,
}
impl Batch {
    fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.touched) > Duration::from_secs(30)
            || now.duration_since(self.created) > Duration::from_secs(300)
    }
    fn validate(&self, token: &str, now: Instant, focused: bool) -> Result<(), String> {
        if self.token != token || self.expired(now) {
            return Err("paste batch expired or cancelled".into());
        }
        if self.busy {
            return Err("paste batch already has an in-flight item".into());
        }
        if !focused {
            return Err("paste batch target lost focus".into());
        }
        Ok(())
    }
}
static BATCH: Mutex<Option<Batch>> = Mutex::new(None);

#[cfg(target_os = "windows")]
fn focused(target: Target, first: bool) -> bool {
    extern "system" {
        fn GetForegroundWindow() -> isize;
        fn GetWindowThreadProcessId(window: isize, pid: *mut u32) -> u32;
    }
    let mut pid = 0;
    unsafe {
        let window = GetForegroundWindow();
        GetWindowThreadProcessId(window, &mut pid);
        (window == target.window_handle && pid == target.process_id && pid != 0)
            || (first && pid == std::process::id())
    }
}
#[cfg(not(target_os = "windows"))]
fn focused(target: Target, first: bool) -> bool {
    crate::platform::quick_paste::current() == Some(target)
        || (first && crate::platform::quick_paste::current().is_none())
}

#[tauri::command]
pub fn begin_paste_batch(
    hotkey_manager: tauri::State<'_, Mutex<HotkeyManager>>,
) -> Result<Option<String>, String> {
    let mut slot = BATCH
        .lock()
        .map_err(|_| "paste batch lock is poisoned".to_owned())?;
    let now = Instant::now();
    if slot.as_ref().is_some_and(|batch| !batch.expired(now)) {
        return Err("another paste batch is active".into());
    }
    *slot = None;
    let Some(target) =
        lock_state(&hotkey_manager, "hotkey manager lock is poisoned")?.take_quick_paste_target()
    else {
        return Ok(None);
    };
    #[cfg(target_os = "windows")]
    if target.window_handle == 0
        || target.process_id == 0
        || target.process_id == std::process::id()
    {
        return Ok(None);
    }
    #[cfg(not(target_os = "windows"))]
    if target.handle == 0 || target.pid == 0 || target.pid == std::process::id() {
        return Ok(None);
    }
    let token = uuid::Uuid::new_v4().to_string();
    *slot = Some(Batch {
        token: token.clone(),
        target,
        created: now,
        touched: now,
        started: false,
        busy: false,
    });
    Ok(Some(token))
}

#[tauri::command]
pub fn validate_paste_batch(token: String) -> Result<(), String> {
    let slot = BATCH
        .lock()
        .map_err(|_| "paste batch lock is poisoned".to_owned())?;
    let batch = slot.as_ref().ok_or("paste batch is no longer active")?;
    batch.validate(
        &token,
        Instant::now(),
        focused(batch.target, !batch.started),
    )
}

#[tauri::command]
pub fn end_paste_batch(token: String) -> Result<(), String> {
    let mut slot = BATCH
        .lock()
        .map_err(|_| "paste batch lock is poisoned".to_owned())?;
    if slot.as_ref().is_some_and(|batch| batch.token == token) {
        *slot = None;
    }
    Ok(())
}

pub(super) fn acquire(token: &str) -> Result<Target, String> {
    let mut slot = BATCH
        .lock()
        .map_err(|_| "paste batch lock is poisoned".to_owned())?;
    let batch = slot.as_mut().ok_or("paste batch is no longer active")?;
    batch.validate(token, Instant::now(), focused(batch.target, !batch.started))?;
    batch.busy = true;
    batch.touched = Instant::now();
    Ok(batch.target)
}

pub(super) fn active(token: &str) -> bool {
    BATCH.lock().ok().is_some_and(|slot| {
        slot.as_ref().is_some_and(|batch| {
            batch.token == token
                && !batch.expired(Instant::now())
                && focused(batch.target, !batch.started)
        })
    })
}
pub(super) fn finish(token: &str, succeeded: bool) {
    if let Ok(mut slot) = BATCH.lock() {
        if let Some(batch) = slot.as_mut().filter(|batch| batch.token == token) {
            if succeeded {
                batch.started = true;
                batch.busy = false;
                batch.touched = Instant::now();
            } else {
                *slot = None;
            }
        }
    }
}
pub(super) fn refuse_single_during_batch() -> Result<(), String> {
    let slot = BATCH
        .lock()
        .map_err(|_| "paste batch lock is poisoned".to_owned())?;
    if slot
        .as_ref()
        .is_some_and(|batch| !batch.expired(Instant::now()))
    {
        return Err("a paste batch is active".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_lease_requires_token_focus_and_excludes_overlaps() {
        #[cfg(target_os = "windows")]
        let target = Target {
            window_handle: 123,
            process_id: 456,
        };
        #[cfg(not(target_os = "windows"))]
        let target = Target {
            handle: 123,
            pid: 456,
        };
        let now = Instant::now();
        let mut batch = Batch {
            token: "batch".into(),
            target,
            created: now,
            touched: now,
            started: false,
            busy: false,
        };
        assert!(batch.validate("batch", now, false).is_err());
        assert!(batch.validate("batch", now, true).is_ok());
        assert!(batch.validate("wrong", now, true).is_err());
        batch.busy = true;
        assert!(batch.validate("batch", now, true).is_err());
        batch.busy = false;
        batch.started = true;
        assert!(batch.validate("batch", now, false).is_err());
        assert!(batch.validate("batch", now, true).is_ok());
        assert!(batch
            .validate("batch", now + Duration::from_secs(31), true)
            .is_err());
        batch.touched = now + Duration::from_secs(301);
        assert!(batch
            .validate("batch", now + Duration::from_secs(301), true)
            .is_err());
    }
}
