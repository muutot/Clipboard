use std::sync::Mutex;

use crate::state::CaptureState;
use tauri::Manager;

pub fn stop_runtime_services(app: &tauri::AppHandle) {
    if let Some(cleanup) = app.try_state::<Mutex<crate::CleanupWorker>>() {
        match cleanup.lock() {
            Ok(cleanup) => cleanup.stop(),
            Err(_) => crate::log_event!("[shutdown] history cleanup lock is poisoned"),
        }
    }

    if let Some(monitor) = app.try_state::<Mutex<crate::platform::ClipboardMonitor>>() {
        match monitor.lock() {
            Ok(mut monitor) => {
                if let Err(error) = monitor.stop() {
                    crate::log_event!("[shutdown] failed to stop clipboard monitor: {error}");
                }
            }
            Err(_) => crate::log_event!("[shutdown] clipboard monitor lock is poisoned"),
        }
    }

    if let Some(capture) = app.try_state::<CaptureState>() {
        capture.stop_worker();
    }

    if let Some(worker) = app.try_state::<crate::OcrWorkerManager>() {
        worker.stop();
    }

    if let Some(thumbnails) = app.try_state::<Mutex<crate::content::ThumbnailWorker>>() {
        match thumbnails.lock() {
            Ok(mut worker) => worker.stop(),
            Err(_) => crate::log_event!("[shutdown] thumbnail worker lock is poisoned"),
        }
    }

    if let Some(hotkey) = app.try_state::<Mutex<crate::platform::windows_hotkey::HotkeyManager>>() {
        match hotkey.lock() {
            Ok(mut hotkey) => hotkey.stop(),
            Err(_) => crate::log_event!("[shutdown] hotkey manager lock is poisoned"),
        }
    }

    if let Some(api) = app.try_state::<Mutex<crate::cli::LocalApiServer>>() {
        match api.lock() {
            Ok(mut api) => {
                if let Err(error) = api.stop() {
                    crate::log_event!("[shutdown] failed to stop local API: {error}");
                }
            }
            Err(_) => crate::log_event!("[shutdown] local API lock is poisoned"),
        }
    }

    if let Some(worker) = app.try_state::<Mutex<Option<crate::search::SearchSyncWorker>>>() {
        match worker.lock() {
            Ok(mut worker) => {
                if let Some(worker) = worker.as_mut() {
                    worker.stop();
                }
            }
            Err(_) => crate::log_event!("[shutdown] search-sync worker lock is poisoned"),
        }
    }
}
