use std::sync::Mutex;

use crate::state::CaptureState;
use tauri::Manager;

/// Lock a managed `Mutex<T>` while keeping the stop path alive: a poisoned
/// lock means the last holder panicked, not that the worker stopped, so the
/// guard is recovered with `into_inner()` and the stop still runs. Skipping
/// the stop would leave background writers (SQLite/S3) running through
/// process teardown.
fn lock_or_recover<'a, T>(lock: &'a Mutex<T>, label: &str) -> std::sync::MutexGuard<'a, T> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            crate::log_event!("[shutdown] {label} lock is poisoned; recovering the guard");
            poisoned.into_inner()
        }
    }
}

pub fn stop_runtime_services(app: &tauri::AppHandle) {
    // Stop the auto-sync worker first: it is a background writer that owns an
    // AppHandle and writes both SQLite and S3, so anything it commits after a
    // storage snapshot would be lost, and it must not resolve managed state
    // after teardown. Its stop waits for the in-flight run up to a bounded
    // timeout and then leaks the (flag-set) thread rather than hang exit on a
    // slow S3 run.
    if let Some(worker) = app.try_state::<Mutex<crate::commands::sync::AutoSyncWorker>>() {
        lock_or_recover(&worker, "auto-sync worker").stop();
    }

    if let Some(cleanup) = app.try_state::<Mutex<crate::CleanupWorker>>() {
        lock_or_recover(&cleanup, "history cleanup").stop();
    }

    if let Some(monitor) = app.try_state::<Mutex<crate::platform::ClipboardMonitor>>() {
        if let Err(error) = lock_or_recover(&monitor, "clipboard monitor").stop() {
            crate::log_event!("[shutdown] failed to stop clipboard monitor: {error}");
        }
    }

    if let Some(capture) = app.try_state::<CaptureState>() {
        capture.stop_worker();
    }

    if let Some(worker) = app.try_state::<crate::OcrWorkerManager>() {
        worker.stop();
    }

    if let Some(thumbnails) = app.try_state::<Mutex<crate::content::ThumbnailWorker>>() {
        lock_or_recover(&thumbnails, "thumbnail worker").stop();
    }

    if let Some(hotkey) = app.try_state::<Mutex<crate::platform::windows_hotkey::HotkeyManager>>() {
        lock_or_recover(&hotkey, "hotkey manager").stop();
    }

    if let Some(api) = app.try_state::<Mutex<crate::cli::LocalApiServer>>() {
        if let Err(error) = lock_or_recover(&api, "local API").stop() {
            crate::log_event!("[shutdown] failed to stop local API: {error}");
        }
    }

    if let Some(worker) = app.try_state::<Mutex<Option<crate::search::SearchSyncWorker>>>() {
        if let Some(worker) = lock_or_recover(&worker, "search-sync worker").as_mut() {
            worker.stop();
        }
    }
}
