use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::signal::{stop_signal_requested, wait_for_stop};
use crate::content;
use crate::content::self_trigger::SelfTriggerGuard;
use crate::content::RESOURCE_METADATA_SCHEMA_VERSION;
use crate::content::{
    created_at_ms, extension_for_path, mime_type_for_path, modified_at_ms, FileStore,
    ThumbnailQueue,
};
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::platform;
use crate::platform::windows_clipboard::ClipboardChange;
use crate::platform::ClipboardMonitor;
use crate::state::{CaptureState, CaptureWorker, SelfTriggerState};
use crate::storage::{ClipboardRepository, Database, OcrRepository, StoragePaths};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub fn foreground_app_name(app: &platform::ForegroundApp) -> Option<String> {
    if !app.exe_path.trim().is_empty() {
        let leaf = app
            .exe_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&app.exe_path);
        Path::new(leaf)
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
    } else if !app.name.trim().is_empty() {
        Some(app.name.trim().to_owned())
    } else {
        None
    }
}

pub fn should_skip_self_triggered_hash(
    guard: &mut content::self_trigger::SelfTriggerGuard,
    content_hash: &str,
) -> bool {
    guard.is_self_triggered(content_hash)
}

pub fn should_skip_self_triggered_text(
    guard: &mut content::self_trigger::SelfTriggerGuard,
    kind: ClipboardKind,
    text: &str,
) -> bool {
    let kind_name = match kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image | ClipboardKind::File => return false,
    };
    guard.is_text_write_self_triggered(kind_name, text)
}

pub fn should_skip_self_triggered_media(
    guard: &mut content::self_trigger::SelfTriggerGuard,
    kind: &str,
    data: &[u8],
) -> bool {
    guard.is_media_write_self_triggered(kind, data)
}

pub fn register_image_self_trigger(
    guard: &mut content::self_trigger::SelfTriggerGuard,
    resource_path: Option<&str>,
    fallback_hash: Option<&str>,
) -> Result<(), String> {
    let mut registered = false;

    if let Some(path) = resource_path.filter(|path| !path.trim().is_empty()) {
        match std::fs::read(path) {
            Ok(data) => {
                guard.mark_media_write("image", &data);
                registered = true;
            }
            Err(error) if fallback_hash.is_none() => {
                return Err(format!("failed to read image for self-trigger: {error}"));
            }
            Err(_) => {}
        }
    }

    if let Some(content_hash) = fallback_hash.filter(|hash| !hash.trim().is_empty()) {
        guard.mark_as_self_triggered(content_hash);
        registered = true;
    }

    if registered {
        Ok(())
    } else {
        Err("image self-trigger has no readable resource or content hash".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFileReference {
    pub(crate) original_path: String,
    pub(crate) storage_path: String,
    pub(crate) original_name: String,
    pub(crate) size_bytes: u64,
    content_hash: Option<String>,
    pub(crate) extension: Option<String>,
    pub(crate) mime_type: String,
    created_at_ms: Option<i64>,
    modified_at_ms: Option<i64>,
    pub(crate) copied: bool,
}

pub fn store_captured_file_references(
    file_paths: &[String],
    file_storage_dir: &Path,
    max_copy_size_bytes: u64,
) -> Vec<CapturedFileReference> {
    file_paths
        .iter()
        .map(|file_path| {
            let source_path = Path::new(file_path);
            match FileStore::save_file(source_path, file_storage_dir, max_copy_size_bytes) {
                Ok(info) => CapturedFileReference {
                    original_path: file_path.clone(),
                    copied: Path::new(&info.storage_path) != source_path,
                    storage_path: info.storage_path,
                    original_name: info.original_name,
                    size_bytes: info.size_bytes,
                    content_hash: Some(info.content_hash),
                    extension: info.extension,
                    mime_type: info.mime_type,
                    created_at_ms: info.created_at_ms,
                    modified_at_ms: info.modified_at_ms,
                },
                Err(error) => {
                    eprintln!(
                        "[clipboard-worker] failed to store file {}: {error}",
                        source_path.display()
                    );
                    let metadata = std::fs::metadata(source_path).ok();
                    CapturedFileReference {
                        original_path: file_path.clone(),
                        storage_path: file_path.clone(),
                        original_name: source_path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| file_path.clone()),
                        size_bytes: metadata.as_ref().map_or(0, std::fs::Metadata::len),
                        content_hash: None,
                        extension: extension_for_path(source_path),
                        mime_type: mime_type_for_path(source_path),
                        created_at_ms: metadata.as_ref().and_then(created_at_ms),
                        modified_at_ms: metadata.as_ref().and_then(modified_at_ms),
                        copied: false,
                    }
                }
            }
        })
        .collect()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileMetadata {
    schema_version: u8,
    size_bytes: u64,
    resource_path: Option<String>,
    original_path: Option<String>,
    files: Vec<FileEntry>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileEntry {
    name: String,
    extension: Option<String>,
    mime_type: String,
    size_bytes: u64,
    storage_path: String,
    original_path: String,
    content_hash: Option<String>,
    copied: bool,
    created_at_ms: Option<i64>,
    modified_at_ms: Option<i64>,
}

pub fn captured_file_metadata(files: &[CapturedFileReference]) -> String {
    let first = files.first();
    serde_json::to_string(&FileMetadata {
        schema_version: RESOURCE_METADATA_SCHEMA_VERSION,
        size_bytes: files.iter().map(|file| file.size_bytes).sum::<u64>(),
        resource_path: first.map(|file| file.storage_path.clone()),
        original_path: if files.len() == 1 {
            first.map(|file| file.original_path.clone())
        } else {
            None
        },
        files: files
            .iter()
            .map(|file| FileEntry {
                name: file.original_name.clone(),
                extension: file.extension.clone(),
                mime_type: file.mime_type.clone(),
                size_bytes: file.size_bytes,
                storage_path: file.storage_path.clone(),
                original_path: file.original_path.clone(),
                content_hash: file.content_hash.clone(),
                copied: file.copied,
                created_at_ms: file.created_at_ms,
                modified_at_ms: file.modified_at_ms,
            })
            .collect(),
    })
    .unwrap_or_default()
}

#[tauri::command]
pub fn start_clipboard_monitoring(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    paths: tauri::State<'_, StoragePaths>,
    capture: tauri::State<'_, CaptureState>,
    self_trigger: tauri::State<'_, SelfTriggerState>,
    thumbnail_worker: tauri::State<'_, Mutex<crate::content::ThumbnailWorker>>,
    app_handle: tauri::AppHandle,
) -> Result<bool, String> {
    let mut guard = monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?;
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
    let thumbnail_queue = thumbnail_worker
        .lock()
        .map_err(|_| "thumbnail worker lock is poisoned".to_owned())?
        .queue();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_for_thread = Arc::clone(&stop_flag);
    let (stop_sender, stop_receiver) = mpsc::channel();

    let handle = thread::Builder::new()
        .name("clipboard-capture".to_owned())
        .spawn(move || {
            let database = match Database::open(&db_path) {
                Ok(db) => db,
                Err(e) => {
                    eprintln!("[clipboard-worker] failed to open database: {e}");
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
pub fn stop_clipboard_monitoring(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
) -> Result<bool, String> {
    monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?
        .stop()?;
    capture.stop_worker();
    Ok(true)
}

#[tauri::command]
pub fn get_clipboard_monitor_status(
    monitor: tauri::State<'_, Mutex<ClipboardMonitor>>,
    capture: tauri::State<'_, CaptureState>,
) -> Result<ClipboardMonitorStatus, String> {
    let monitor = monitor
        .lock()
        .map_err(|_| "clipboard monitor lock is poisoned".to_owned())?;
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

                let app_info = platform::platform().get_foreground_app();
                let source_app = foreground_app_name(&app_info);
                if capture_state.should_skip(source_app.as_deref(), None) {
                    continue;
                }

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

                let text = platform::platform().read_clipboard_text();
                let max_text_capture_bytes = capture_state.max_text_capture_bytes() as usize;
                let html = platform::platform()
                    .read_clipboard_html()
                    .filter(|html| !html.trim().is_empty() && html.len() <= max_text_capture_bytes);
                let rtf = platform::platform()
                    .read_clipboard_rtf()
                    .filter(|rtf| !rtf.trim().is_empty() && rtf.len() <= max_text_capture_bytes);
                let image_data = platform::platform().read_clipboard_image();
                let file_paths = platform::platform().read_clipboard_file_paths();

                if capture_state.should_skip(source_app.as_deref(), text.as_deref()) {
                    continue;
                }

                if let Some((img, img_width, img_height)) = image_data {
                    if stop_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    if should_skip_self_triggered_media(
                        &mut self_trigger_guard
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()),
                        "image",
                        &img,
                    ) {
                        continue;
                    }
                    let img_hash = content::hash::compute_media_hash("image", &img);
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;

                    let image_dir = image_storage_path.clone();
                    std::fs::create_dir_all(&image_dir).ok();
                    let img_path = image_dir.join(format!("{}.png", img_hash));
                    match std::fs::write(&img_path, &img) {
                        Ok(_) => {
                            eprintln!("[clipboard-worker] saved image: {}", img_path.display())
                        }
                        Err(e) => eprintln!(
                            "[clipboard-worker] failed to write image {}: {}",
                            img_path.display(),
                            e
                        ),
                    }

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
                        last_used_at_ms: None,
                        is_favorite: false,
                        metadata_json: Some(metadata.to_string()),
                    };

                    if stop_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    match database.save_item(&item) {
                        Ok(saved_id) => {
                            consecutive_errors = 0;
                            let _ = database.enqueue_ocr(&saved_id);
                            thumbnail_queue.enqueue(saved_id.clone(), img_path.clone());
                            let mut emit_item = item.clone();
                            emit_item.id = saved_id;
                            let _ = app_handle.emit("clipboard-item-added", &emit_item);
                            continue;
                        }
                        Err(e) => {
                            eprintln!("[clipboard-worker] failed to save image: {e}");
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
                            last_used_at_ms: None,
                            is_favorite: false,
                            metadata_json: Some(captured_file_metadata(&stored_files)),
                        };

                        if stop_flag.load(Ordering::SeqCst) {
                            break;
                        }
                        match database.save_item(&item) {
                            Ok(saved_id) => {
                                consecutive_errors = 0;
                                let mut emit_item = item.clone();
                                emit_item.id = saved_id;
                                let _ = app_handle.emit("clipboard-item-added", &emit_item);
                            }
                            Err(e) => {
                                eprintln!("[clipboard-worker] failed to save file: {e}");
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
                            last_used_at_ms: None,
                            is_favorite: false,
                            metadata_json: Some(captured_file_metadata(&stored_files)),
                        };

                        if stop_flag.load(Ordering::SeqCst) {
                            break;
                        }
                        match database.save_item(&item) {
                            Ok(saved_id) => {
                                consecutive_errors = 0;
                                let mut emit_item = item.clone();
                                emit_item.id = saved_id;
                                let _ = app_handle.emit("clipboard-item-added", &emit_item);
                            }
                            Err(e) => {
                                eprintln!("[clipboard-worker] failed to save file batch: {e}");
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
                    last_used_at_ms: None,
                    is_favorite: false,
                    metadata_json: None,
                };

                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                match database.save_item(&item) {
                    Ok(saved_id) => {
                        consecutive_errors = 0;
                        let mut emit_item = item.clone();
                        emit_item.id = saved_id;
                        let _ = app_handle.emit("clipboard-item-added", &emit_item);
                    }
                    Err(e) => {
                        eprintln!("[clipboard-worker] failed to save item: {e}");
                        consecutive_errors += 1;
                        if consecutive_errors >= 10 {
                            eprintln!("[clipboard-worker] too many errors, pausing");
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
                eprintln!("[clipboard-worker] monitor disconnected, stopping");
                break;
            }
        }
    }
}
