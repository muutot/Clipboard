use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::commands::lock::lock_state;
use crate::platform::ClipboardMonitor;
use crate::state::{CaptureState, CaptureWorker, SelfTriggerState};
use crate::storage::{Database, StoragePaths};
use serde::Serialize;

#[tauri::command]
/// Starts (or restarts) the background clipboard polling worker.
pub fn start_clipboard_monitoring(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    paths: tauri::State<'_, StoragePaths>,
    capture: tauri::State<'_, CaptureState>,
    self_trigger: tauri::State<'_, SelfTriggerState>,
    thumbnail_worker: tauri::State<'_, Mutex<crate::content::ThumbnailWorker>>,
    app_handle: tauri::AppHandle,
) -> Result<bool, String> {
    // Acquire the thumbnail queue BEFORE starting the monitor: if this lock
    // is poisoned we must fail without a running monitor that has no
    // consumer draining its change notifications.
    let thumbnail_queue =
        lock_state(&thumbnail_worker, "thumbnail worker lock is poisoned")?.queue();

    let mut guard = lock_state(&monitor, "clipboard monitor lock is poisoned")?;
    guard.start()?;

    let receiver = guard
        .take_receiver()
        .ok_or("clipboard monitor started but no receiver available".to_owned())?;

    drop(guard);

    let db_path = paths.database.clone();
    let storage_path = paths.storage.clone();
    let image_storage_path = paths.images.clone();
    let file_storage_path = paths.files.clone();
    let self_trigger_clone = self_trigger.0.clone();
    let capture_for_thread = capture.inner().clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_for_thread = Arc::clone(&stop_flag);
    let (stop_sender, stop_receiver) = mpsc::channel();

    let handle = thread::Builder::new()
        .name("clipboard-capture".to_owned())
        .spawn(move || {
            let database = match Database::open(&db_path) {
                Ok(db) => db,
                Err(e) => {
                    crate::log_error!("[clipboard-worker] failed to open database: {e}");
                    return;
                }
            };

            // Reuse the same ingestion loop as the startup path so a monitor
            // (re)started at runtime handles text, image, and file copies
            // identically — previously this command only ingested text/html.
            run_capture_loop(
                receiver,
                database,
                capture_for_thread,
                self_trigger_clone,
                stop_flag_for_thread,
                stop_receiver,
                storage_path,
                image_storage_path,
                file_storage_path,
                thumbnail_queue,
                app_handle,
            );
        })
        .map_err(|error| format!("failed to start clipboard worker: {error}"))?;

    capture.install_worker(CaptureWorker {
        stop_flag,
        stop_sender: Some(stop_sender),
        handle: Some(handle),
    });

    Ok(true)
}

#[tauri::command]
/// Stops the clipboard polling worker and joins its thread.
pub fn stop_clipboard_monitoring(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
) -> Result<bool, String> {
    // Always stop the capture worker, even when the monitor lock is
    // poisoned: an early `?` here used to leak the writer thread while
    // reporting the monitor as stopped.
    let monitor_result =
        lock_state(&monitor, "clipboard monitor lock is poisoned").and_then(|monitor| {
            let mut monitor = monitor;
            monitor.stop()
        });
    capture.stop_worker();
    monitor_result?;
    Ok(true)
}

#[tauri::command]
/// Reports whether the clipboard polling worker is alive for the UI.
pub fn get_clipboard_monitor_status(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
) -> Result<ClipboardMonitorStatus, String> {
    let monitor = lock_state(&monitor, "clipboard monitor lock is poisoned")?;
    Ok(ClipboardMonitorStatus {
        running: monitor.running && capture.worker_running(),
        ignored_applications: capture.ignored_apps(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardMonitorStatus {
    running: bool,
    ignored_applications: Vec<String>,
}

#[tauri::command]
/// Marks text written by this app as self-triggered for the suppression window.
pub fn mark_self_triggered(
    self_trigger: tauri::State<'_, SelfTriggerState>,
    text: String,
) -> Result<(), String> {
    self_trigger
        .0
        .lock()
        .map_err(|_| "self-trigger lock poisoned".to_owned())?
        .mark_clipboard_write(&text);
    Ok(())
}

#[tauri::command]
/// Marks image bytes written by this app as self-triggered.
pub fn mark_self_triggered_image(
    self_trigger: tauri::State<'_, SelfTriggerState>,
    resource_path: Option<String>,
    content_hash: Option<String>,
) -> Result<(), String> {
    let mut guard = self_trigger
        .0
        .lock()
        .map_err(|_| "self-trigger lock poisoned".to_owned())?;
    register_image_self_trigger(
        &mut guard,
        resource_path.as_deref(),
        content_hash.as_deref(),
    )
}

#[tauri::command]
/// Clears a text self-trigger marker after the clipboard write failed, so an
/// external copy of the same content inside the suppression window is still
/// captured instead of being silently swallowed.
pub fn unmark_self_triggered(
    self_trigger: tauri::State<'_, SelfTriggerState>,
    text: String,
) -> Result<(), String> {
    self_trigger
        .0
        .lock()
        .map_err(|_| "self-trigger lock poisoned".to_owned())?
        .unmark_clipboard_write(&text);
    Ok(())
}

#[tauri::command]
/// Clears image self-trigger markers after the clipboard write failed.
pub fn unmark_self_triggered_image(
    self_trigger: tauri::State<'_, SelfTriggerState>,
    resource_path: Option<String>,
    content_hash: Option<String>,
) -> Result<(), String> {
    let mut guard = self_trigger
        .0
        .lock()
        .map_err(|_| "self-trigger lock poisoned".to_owned())?;
    if let Some(path) = resource_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        if let Ok(data) = std::fs::read(path) {
            guard.unmark_media_write("image", &data);
        }
    }
    if let Some(content_hash) = content_hash
        .as_deref()
        .filter(|hash| !hash.trim().is_empty())
    {
        guard.unmark(content_hash);
    }
    Ok(())
}

/// Shared clipboard ingestion loop used by both the startup path in `lib.rs`
/// and the `start_clipboard_monitoring` Tauri command. Handles all clipboard
/// content kinds (text/html, image, files) so a monitor (re)started at runtime
/// behaves identically to the one launched at startup — previously the command
/// path only ingested text/html and silently dropped image/file copies.
///
/// The function owns the worker thread's lifecycle resources (database, stop
/// flag, stop receiver, app handle) and returns once the loop has terminated.
/// Callers are responsible for spawning the thread and installing the
/// resulting `CaptureWorker`.
/// Counts one failed `save_item` and applies the shared backoff: after 10
/// consecutive failures the capture loop pauses briefly so a persistently
/// broken database cannot spin the worker. Returns true when a stop signal
/// arrived during the pause and the loop should exit. Media captures use the
/// same breaker as text captures — image/file writes fail just as hard when
// ---------------------------------------------------------------------------
// Submodules
// ---------------------------------------------------------------------------
mod files;
mod helpers;
#[cfg(test)]
mod tests;
mod worker;

// ---------------------------------------------------------------------------
// Re-exports so items remain at `crate::commands::capture::*`
// ---------------------------------------------------------------------------

pub use files::*;
pub use helpers::*;
pub(crate) use worker::run_capture_loop;
