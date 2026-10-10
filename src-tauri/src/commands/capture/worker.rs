//! The shared clipboard ingestion loop and its failure backoff.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::commands::signal::{stop_signal_requested, wait_for_stop};

use super::files::{captured_file_metadata, store_captured_file_references};
use super::helpers::{
    apply_capture_auto_tags, foreground_app_name, load_emit_item, normalize_platform_image_to_png,
    should_skip_self_triggered_hash, should_skip_self_triggered_text,
};
use crate::content;
use crate::content::self_trigger::SelfTriggerGuard;
use crate::content::{FileStore, ThumbnailQueue, RESOURCE_METADATA_SCHEMA_VERSION};
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::platform;
use crate::platform::windows::ClipboardChange;
use crate::state::CaptureState;
use crate::storage::{ClipboardRepository, Database, OcrRepository};

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
/// the database is down.
fn note_capture_save_failure(
    consecutive_errors: &mut u32,
    stop_receiver: &mpsc::Receiver<()>,
    stop_flag: &AtomicBool,
) -> bool {
    *consecutive_errors += 1;
    if *consecutive_errors >= 10 {
        crate::log_warn!("[clipboard-worker] too many errors, pausing");
        if wait_for_stop(stop_receiver, stop_flag, Duration::from_secs(5)) {
            return true;
        }
        *consecutive_errors = 0;
    }
    false
}

/// The function owns the worker thread's lifecycle resources (database, stop
/// flag, stop receiver, app handle) and returns once the loop has terminated.
/// Callers are responsible for spawning the thread and installing the
/// resulting `CaptureWorker`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_capture_loop(
    receiver: mpsc::Receiver<ClipboardChange>,
    database: Database,
    capture_state: CaptureState,
    self_trigger_guard: Arc<Mutex<SelfTriggerGuard>>,
    stop_flag: Arc<AtomicBool>,
    stop_receiver: mpsc::Receiver<()>,
    storage_path: PathBuf,
    image_storage_path: PathBuf,
    file_storage_path: PathBuf,
    thumbnail_queue: ThumbnailQueue,
    app_handle: AppHandle,
) {
    let mut consecutive_errors = 0u32;

    loop {
        if stop_flag.load(Ordering::SeqCst) || stop_signal_requested(&stop_receiver) {
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(_change) => {
                let _ingestion_guard = capture_state
                    .ingestion_guard
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if stop_flag.load(Ordering::SeqCst) || stop_signal_requested(&stop_receiver) {
                    break;
                }

                #[cfg(any(target_os = "linux", target_os = "macos"))]
                let _helper_budget =
                    platform::bounded_command::CaptureBudget::enter(stop_flag.clone());
                let app_info = platform::platform().get_foreground_app();
                let source_app = foreground_app_name(&app_info);
                if capture_state.should_skip(source_app.as_deref(), None) {
                    continue;
                }

                let _resource_publication = database.begin_resource_write();

                // Extract and cache app icon
                let icon_dir = storage_path.join("icons");
                let icon_path = if let Some(source_name) = source_app.as_deref() {
                    platform::platform().extract_app_icon(
                        &icon_dir,
                        source_name,
                        &app_info.exe_path,
                    )
                } else {
                    None
                };

                let platform = platform::platform();
                let max_text_capture_bytes = capture_state.max_text_capture_bytes() as usize;
                let Some(snapshot) = platform::clipboard_snapshot::read_consistent_snapshot(
                    platform,
                    max_text_capture_bytes,
                ) else {
                    continue;
                };
                let platform::clipboard_snapshot::ClipboardSnapshot {
                    text,
                    html,
                    rtf,
                    image: image_data,
                    files: file_paths,
                } = snapshot;

                if capture_state.should_skip(source_app.as_deref(), text.as_deref()) {
                    continue;
                }

                // The foreground window may have changed while the clipboard
                // formats were being read (or before this event was handled).
                // The clipboard carries no source attribution, so a single
                // sample cannot prove where the content came from. Re-sample
                // and skip when either endpoint is sensitive/paused: this
                // conservatively closes the copy-in-sensitive-app then
                // alt-tab-before-handling bypass without dropping captures
                // that moved between two ordinary windows.
                let app_info_after = platform::platform().get_foreground_app();
                let source_app_after = foreground_app_name(&app_info_after);
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                if platform::bounded_command::capture_aborted() {
                    continue;
                }
                if source_app_after != source_app
                    && capture_state.should_skip(source_app_after.as_deref(), text.as_deref())
                {
                    continue;
                }

                if let Some((img, img_width, img_height)) = image_data {
                    if stop_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    // Linux/macOS adapters return raw RGBA pixels; re-encode
                    // them here so the persisted `{hash}.png` is decodable.
                    let img = normalize_platform_image_to_png(img, img_width, img_height);
                    if img.is_empty() {
                        // Never persist a dangling resource (see the write
                        // path below): normalization produced nothing usable.
                        continue;
                    }
                    // One pass computes the raw + normalized hashes and checks
                    // them against recent self-writes; the raw hash doubles as
                    // the stored identity, avoiding a second SHA-256 of the
                    // same (potentially tens-of-MB) buffer below.
                    let (self_triggered, img_hashes) = {
                        let mut guard = self_trigger_guard
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let hashes = guard.media_write_hashes("image", &img);
                        let triggered = hashes
                            .iter()
                            .any(|content_hash| guard.is_self_triggered(content_hash));
                        (triggered, hashes)
                    };
                    if self_triggered {
                        continue;
                    }
                    let img_hash = img_hashes[0].clone();
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;

                    let image_dir = image_storage_path.clone();
                    if let Err(e) = std::fs::create_dir_all(&image_dir) {
                        // Without the directory the write below reports a
                        // misleading error, so name the root cause here.
                        crate::log_error!(
                            "[clipboard-worker] failed to create image directory {}: {}",
                            image_dir.display(),
                            e
                        );
                        // A record pointing at an unwritten file renders
                        // broken forever and fails every OCR/thumbnail job,
                        // so drop this capture instead of saving it.
                        continue;
                    }
                    let img_path = image_dir.join(format!("{}.png", img_hash));
                    // Content-addressed bytes go through the atomic writer:
                    // a bare `fs::write` interrupted by a crash leaves a
                    // truncated `{hash}.png` that later captures would never
                    // repair, poisoning every OCR/thumbnail/copy job for it.
                    if let Err(e) = FileStore::save_bytes_atomically(&img_path, &img) {
                        crate::log_error!(
                            "[clipboard-worker] failed to write image {}: {}",
                            img_path.display(),
                            e
                        );
                        // Same as above: never persist a dangling resource.
                        continue;
                    }
                    crate::log_event!("[clipboard-worker] saved image: {}", img_path.display());

                    let image_path = img_path.to_string_lossy().to_string();
                    let metadata = serde_json::json!({
                        "schemaVersion": RESOURCE_METADATA_SCHEMA_VERSION,
                        "width": img_width,
                        "height": img_height,
                        "mimeType": "image/png",
                        "extension": "png",
                        "sizeBytes": img.len(),
                        "resourcePath": image_path,
                        "previewPath": image_path,
                        "storagePath": image_path,
                        "contentHash": img_hash,
                    });

                    let item = ClipboardItem {
                        id: format!("img_{}", img_hash),
                        kind: ClipboardKind::Image,
                        title: img_path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        text_content: None,
                        html_content: None,
                        rtf_content: None,
                        resource_path: Some(image_path.clone()),
                        preview_path: Some(image_path),
                        content_hash: img_hash,
                        source_app: source_app.clone(),
                        icon_path: icon_path.clone(),
                        size_bytes: img.len() as u64,
                        created_at_ms: now_ms,
                        last_used_at_ms: Some(now_ms),
                        is_favorite: false,
                        metadata_json: Some(metadata.to_string()),
                    };

                    if stop_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    match database.save_item(&item) {
                        Ok(saved_id) => {
                            consecutive_errors = 0;
                            if let Err(e) = database.enqueue_ocr(&saved_id) {
                                // A missed enqueue means the screenshot never
                                // becomes searchable text; at least log it.
                                crate::log_error!(
                                    "[clipboard-worker] failed to enqueue OCR for {saved_id}: {e}"
                                );
                            }
                            thumbnail_queue.enqueue(saved_id.clone(), img_path.clone());
                            apply_capture_auto_tags(&database, &capture_state, &saved_id, &item);
                            let emit_item = load_emit_item(&database, &saved_id, &item);
                            if let Err(error) = app_handle.emit("clipboard-item-added", &emit_item)
                            {
                                crate::log_error!(
                                    "[clipboard-worker] failed to emit item-added: {error}"
                                );
                            }
                            crate::platform::refresh_tray_recent_menu(&app_handle);
                            continue;
                        }
                        Err(e) => {
                            crate::log_error!("[clipboard-worker] failed to save image: {e}");
                            if note_capture_save_failure(
                                &mut consecutive_errors,
                                &stop_receiver,
                                &stop_flag,
                            ) {
                                break;
                            }
                        }
                    }
                    continue;
                }

                if !file_paths.is_empty() {
                    if stop_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;

                    if file_paths.len() == 1 {
                        let file_path = &file_paths[0];
                        let file_hash = content::hash::compute_file_capture_hash(
                            std::slice::from_ref(file_path),
                        );
                        if should_skip_self_triggered_hash(
                            &mut self_trigger_guard
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                            &file_hash,
                        ) {
                            continue;
                        }
                        let stored_files = store_captured_file_references(
                            std::slice::from_ref(file_path),
                            &file_storage_path,
                            capture_state.max_file_copy_size_bytes(),
                        );
                        let stored_file = &stored_files[0];

                        let item = ClipboardItem {
                            id: format!("file_{}", file_hash),
                            kind: ClipboardKind::File,
                            title: stored_file.original_name.clone(),
                            text_content: None,
                            html_content: None,
                            rtf_content: None,
                            resource_path: Some(stored_file.storage_path.clone()),
                            preview_path: None,
                            content_hash: file_hash,
                            source_app: source_app.clone(),
                            icon_path: icon_path.clone(),
                            size_bytes: stored_file.size_bytes,
                            created_at_ms: now_ms,
                            last_used_at_ms: Some(now_ms),
                            is_favorite: false,
                            metadata_json: Some(captured_file_metadata(&stored_files)),
                        };

                        if stop_flag.load(Ordering::SeqCst) {
                            break;
                        }
                        match database.save_item(&item) {
                            Ok(saved_id) => {
                                consecutive_errors = 0;
                                apply_capture_auto_tags(
                                    &database,
                                    &capture_state,
                                    &saved_id,
                                    &item,
                                );
                                let emit_item = load_emit_item(&database, &saved_id, &item);
                                if let Err(error) =
                                    app_handle.emit("clipboard-item-added", &emit_item)
                                {
                                    crate::log_error!(
                                        "[clipboard-worker] failed to emit item-added: {error}"
                                    );
                                }
                                crate::platform::refresh_tray_recent_menu(&app_handle);
                            }
                            Err(e) => {
                                crate::log_error!("[clipboard-worker] failed to save file: {e}");
                                if note_capture_save_failure(
                                    &mut consecutive_errors,
                                    &stop_receiver,
                                    &stop_flag,
                                ) {
                                    break;
                                }
                            }
                        }
                    } else {
                        let group_hash = content::hash::compute_file_capture_hash(&file_paths);
                        if should_skip_self_triggered_hash(
                            &mut self_trigger_guard
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                            &group_hash,
                        ) {
                            continue;
                        }

                        let stored_files = store_captured_file_references(
                            &file_paths,
                            &file_storage_path,
                            capture_state.max_file_copy_size_bytes(),
                        );
                        let total_size = stored_files.iter().map(|file| file.size_bytes).sum();
                        let stored_paths = stored_files
                            .iter()
                            .map(|file| file.storage_path.clone())
                            .collect::<Vec<_>>();
                        let paths_json = serde_json::to_string(&stored_paths).unwrap_or_default();

                        let item = ClipboardItem {
                            id: format!("files_{}", group_hash),
                            kind: ClipboardKind::File,
                            title: stored_files[0].original_name.clone(),
                            text_content: Some(paths_json),
                            html_content: None,
                            rtf_content: None,
                            resource_path: Some(stored_files[0].storage_path.clone()),
                            preview_path: None,
                            content_hash: group_hash,
                            source_app: source_app.clone(),
                            icon_path: icon_path.clone(),
                            size_bytes: total_size,
                            created_at_ms: now_ms,
                            last_used_at_ms: Some(now_ms),
                            is_favorite: false,
                            metadata_json: Some(captured_file_metadata(&stored_files)),
                        };

                        if stop_flag.load(Ordering::SeqCst) {
                            break;
                        }
                        match database.save_item(&item) {
                            Ok(saved_id) => {
                                consecutive_errors = 0;
                                apply_capture_auto_tags(
                                    &database,
                                    &capture_state,
                                    &saved_id,
                                    &item,
                                );
                                let emit_item = load_emit_item(&database, &saved_id, &item);
                                if let Err(error) =
                                    app_handle.emit("clipboard-item-added", &emit_item)
                                {
                                    crate::log_error!(
                                        "[clipboard-worker] failed to emit item-added: {error}"
                                    );
                                }
                                crate::platform::refresh_tray_recent_menu(&app_handle);
                            }
                            Err(e) => {
                                crate::log_error!(
                                    "[clipboard-worker] failed to save file batch: {e}"
                                );
                                if note_capture_save_failure(
                                    &mut consecutive_errors,
                                    &stop_receiver,
                                    &stop_flag,
                                ) {
                                    break;
                                }
                            }
                        }
                    }
                    continue;
                }

                let text = match text {
                    Some(t) => t,
                    None => continue,
                };

                if text.is_empty() || text.len() > max_text_capture_bytes {
                    continue;
                }

                let markers = content::detect_markers(&text);
                let kind = if markers.is_link {
                    ClipboardKind::Link
                } else {
                    ClipboardKind::Text
                };

                let content_hash = content::hash::compute_content_hash(
                    if kind == ClipboardKind::Link {
                        "link"
                    } else {
                        "text"
                    },
                    &text,
                    None,
                );

                if should_skip_self_triggered_text(
                    &mut self_trigger_guard
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()),
                    kind,
                    &text,
                ) {
                    continue;
                }

                let title = text.chars().take(200).collect::<String>();
                let size_bytes = text.len() as u64;
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;

                let item = ClipboardItem {
                    id: format!("{}_{}", content_hash, now_ms),
                    kind,
                    title: title.clone(),
                    text_content: Some(text.clone()),
                    html_content: html,
                    rtf_content: rtf,
                    resource_path: None,
                    preview_path: None,
                    content_hash: content_hash.clone(),
                    source_app: source_app.clone(),
                    icon_path: icon_path.clone(),
                    size_bytes,
                    created_at_ms: now_ms,
                    last_used_at_ms: Some(now_ms),
                    is_favorite: false,
                    metadata_json: None,
                };

                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                match database.save_item(&item) {
                    Ok(saved_id) => {
                        consecutive_errors = 0;
                        apply_capture_auto_tags(&database, &capture_state, &saved_id, &item);
                        let emit_item = load_emit_item(&database, &saved_id, &item);
                        if let Err(error) = app_handle.emit("clipboard-item-added", &emit_item) {
                            crate::log_error!(
                                "[clipboard-worker] failed to emit item-added: {error}"
                            );
                        }
                        crate::platform::refresh_tray_recent_menu(&app_handle);
                    }
                    Err(e) => {
                        crate::log_error!("[clipboard-worker] failed to save item: {e}");
                        consecutive_errors += 1;
                        if consecutive_errors >= 10 {
                            crate::log_warn!("[clipboard-worker] too many errors, pausing");
                            if wait_for_stop(&stop_receiver, &stop_flag, Duration::from_secs(5)) {
                                break;
                            }
                            consecutive_errors = 0;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                crate::log_event!("[clipboard-worker] monitor disconnected, stopping");
                break;
            }
        }
    }
}
