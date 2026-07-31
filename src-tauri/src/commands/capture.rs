use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::signal::{stop_signal_requested, wait_for_stop};
use crate::content;
use crate::content::RESOURCE_METADATA_SCHEMA_VERSION;
use crate::content::{
    created_at_ms, extension_for_path, mime_type_for_path, modified_at_ms, FileStore,
};
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::platform;
use crate::platform::ClipboardMonitor;
use crate::state::{CaptureState, CaptureWorker, SelfTriggerState};
use crate::storage::{ClipboardRepository, Database, StoragePaths};
use serde::Serialize;

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
    let self_trigger_clone = self_trigger.0.clone();
    let capture_for_thread = capture.inner().clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_for_thread = Arc::clone(&stop_flag);
    let (stop_sender, stop_receiver) = mpsc::channel();

    let handle = thread::Builder::new()
        .name("clipboard-capture".to_owned())
        .spawn(move || {
            let database = match Database::open(&db_path) {
                Ok(db) => Arc::new(db),
                Err(e) => {
                    eprintln!("[clipboard-worker] failed to open database: {e}");
                    return;
                }
            };

            let self_trigger_guard = self_trigger_clone;
            let mut consecutive_errors = 0u32;

            loop {
                if stop_flag_for_thread.load(Ordering::SeqCst)
                    || stop_signal_requested(&stop_receiver)
                {
                    break;
                }
                match receiver.recv_timeout(Duration::from_millis(500)) {
                    Ok(_change) => {
                        let _ingestion_guard = capture_for_thread
                            .ingestion_guard
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        if stop_flag_for_thread.load(Ordering::SeqCst)
                            || stop_signal_requested(&stop_receiver)
                        {
                            break;
                        }

                        let app_info = platform::get_foreground_app();
                        let source_app = foreground_app_name(&app_info);
                        if capture_for_thread.should_skip(source_app.as_deref(), None) {
                            continue;
                        }

                        let text = match platform::read_clipboard_text() {
                            Some(t) => t,
                            None => continue,
                        };
                        let html = platform::read_clipboard_html()
                            .filter(|html| !html.trim().is_empty() && html.len() <= 500_000);

                        if text.is_empty() || text.len() > 500_000 {
                            continue;
                        }

                        if capture_for_thread.should_skip(source_app.as_deref(), Some(&text)) {
                            continue;
                        }

                        let markers = content::detect_markers(&text);
                        let kind = if markers.is_link || markers.has_url {
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
                            &mut self_trigger_guard.lock().unwrap(),
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
                            id: content_hash.clone(),
                            kind,
                            title,
                            text_content: Some(text),
                            html_content: html,
                            resource_path: None,
                            preview_path: None,
                            content_hash,
                            source_app: source_app.clone(),
                            icon_path: None,
                            size_bytes,
                            created_at_ms: now_ms,
                            last_used_at_ms: None,
                            is_favorite: false,
                            metadata_json: None,
                        };

                        if stop_flag_for_thread.load(Ordering::SeqCst)
                            || stop_signal_requested(&stop_receiver)
                        {
                            break;
                        }

                        match database.save_item(&item) {
                            Ok(_) => {
                                consecutive_errors = 0;
                            }
                            Err(e) => {
                                eprintln!("[clipboard-worker] failed to save item: {e}");
                                consecutive_errors += 1;
                                if consecutive_errors >= 10 {
                                    eprintln!(
                                        "[clipboard-worker] too many consecutive errors, pausing"
                                    );
                                    if wait_for_stop(
                                        &stop_receiver,
                                        &stop_flag_for_thread,
                                        Duration::from_secs(5),
                                    ) {
                                        break;
                                    }
                                    consecutive_errors = 0;
                                }
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        eprintln!("[clipboard-worker] monitor channel disconnected, stopping");
                        break;
                    }
                }
            }
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
