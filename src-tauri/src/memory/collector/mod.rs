//! Best-effort memory diagnostics for the desktop process group.
//!
//! The diagnostics command is intentionally read-only.  It samples the
//! current process and descendants (when the platform exposes a process
//! table), then reports process-group totals and system memory.  A denied or
//! unavailable platform probe is represented by `null` rather than making
//! the clipboard pipeline fail.
//!
//! Shared collection logic and the `null`-on-failure fallbacks live here; the
//! per-OS probes are split into `linux.rs`, `macos.rs`, and `windows.rs`.

use std::{
    fs,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{config::ConfigStore, ocr, storage::StoragePaths};

use super::types::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Resident set size of the current process in bytes (Windows only).
/// Exposed for the performance metrics panel so it can sample memory via
/// `GetProcessMemoryInfo` instead of spawning a helper process per snapshot.
#[cfg(target_os = "windows")]
pub(crate) fn current_process_working_set_bytes() -> Option<u64> {
    windows::current_process_working_set_bytes()
}

/// Returns a read-only snapshot of process-group and system memory usage.
#[tauri::command]
pub fn get_memory_diagnostics(
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<MemoryDiagnostics, String> {
    Ok(collect_memory_diagnostics(&paths, &config))
}

/// Pure collector used by the command and unit tests.  All platform probes
/// are best effort and never panic when a process exits during enumeration.
pub fn collect_memory_diagnostics(
    paths: &StoragePaths,
    config: &Mutex<ConfigStore>,
) -> MemoryDiagnostics {
    let current_pid = std::process::id();
    let mut processes = collect_processes(current_pid);
    if !processes.iter().any(|process| process.pid == current_pid) {
        processes.push(fallback_current_process(current_pid));
    }

    processes.sort_by_key(|process| (if process.pid == current_pid { 0 } else { 1 }, process.pid));
    let current_process = processes
        .iter()
        .find(|process| process.pid == current_pid)
        .cloned()
        .unwrap_or_else(|| fallback_current_process(current_pid));
    let process_group = summarize_process_group(processes);

    MemoryDiagnostics {
        sampled_at_ms: unix_timestamp_ms(),
        current_process,
        process_group,
        system: collect_system_memory(),
        ocr: collect_ocr_memory(paths, config),
    }
}

pub(crate) fn summarize_process_group(mut processes: Vec<MemoryProcess>) -> MemoryProcessGroup {
    let working_set_bytes = processes
        .iter()
        .filter_map(|process| process.working_set_bytes)
        .fold(0u64, u64::saturating_add);
    let private_bytes = processes
        .iter()
        .filter_map(|process| process.private_bytes)
        .fold(0u64, u64::saturating_add);
    let virtual_bytes = processes
        .iter()
        .filter_map(|process| process.virtual_bytes)
        .fold(0u64, u64::saturating_add);

    // Keep the current process first even when a platform-specific collector
    // returns an unsorted process table.  This makes the UI stable between
    // refreshes without imposing a platform-specific tree order.
    processes.sort_by_key(|process| process.pid != std::process::id());

    MemoryProcessGroup {
        working_set_bytes,
        private_bytes,
        virtual_bytes,
        processes,
    }
}

fn fallback_current_process(pid: u32) -> MemoryProcess {
    MemoryProcess {
        pid,
        parent_pid: None,
        name: current_executable_name(),
        role: Some("main".to_owned()),
        working_set_bytes: None,
        private_bytes: None,
        private_working_set_bytes: None,
        virtual_bytes: None,
    }
}

fn current_executable_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "clipboard-desktop".to_owned())
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn collect_ocr_memory(paths: &StoragePaths, config: &Mutex<ConfigStore>) -> OcrMemoryDiagnostics {
    let (engine, configured_variant) = config
        .lock()
        .map(|config| {
            (
                config.ocr_engine().to_owned(),
                config.ppocr_model_variant().to_owned(),
            )
        })
        .unwrap_or_else(|_| ("unknown".to_owned(), "small".to_owned()));

    let model_directory = ocr::models::models_dir(&paths.storage);
    let (model_bytes, model_file_count) = directory_size(&model_directory);
    let installed_variants = ocr::models::installed_model_variants(&model_directory)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let model = ocr::models::model_spec(&configured_variant)
        .unwrap_or_else(ocr::models::default_model_spec);
    let loaded = engine.eq_ignore_ascii_case("ppocr")
        && ocr::models::model_is_installed(&model_directory, model);

    OcrMemoryDiagnostics {
        engine,
        model_variant: model.id.to_owned(),
        model_bytes,
        model_file_count,
        model_directory: model_directory.to_string_lossy().into_owned(),
        loaded,
        installed_variants,
    }
}

pub(crate) fn directory_size(root: &Path) -> (u64, u64) {
    let mut pending = vec![root.to_path_buf()];
    let mut total_bytes = 0u64;
    let mut file_count = 0u64;

    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                total_bytes = total_bytes.saturating_add(metadata.len());
                file_count = file_count.saturating_add(1);
            }
        }
    }

    (total_bytes, file_count)
}

#[cfg(target_os = "windows")]
pub(crate) fn collect_processes(current_pid: u32) -> Vec<MemoryProcess> {
    windows::collect_processes(current_pid)
}

#[cfg(target_os = "linux")]
pub(crate) fn collect_processes(current_pid: u32) -> Vec<MemoryProcess> {
    linux::collect_processes(current_pid)
}

#[cfg(target_os = "macos")]
pub(crate) fn collect_processes(current_pid: u32) -> Vec<MemoryProcess> {
    macos::collect_processes(current_pid)
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub(crate) fn collect_processes(current_pid: u32) -> Vec<MemoryProcess> {
    vec![fallback_current_process(current_pid)]
}

#[cfg(target_os = "windows")]
pub(crate) fn collect_system_memory() -> SystemMemory {
    windows::collect_system_memory()
}

#[cfg(target_os = "linux")]
pub(crate) fn collect_system_memory() -> SystemMemory {
    linux::collect_system_memory()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(crate) fn collect_system_memory() -> SystemMemory {
    SystemMemory {
        total_bytes: None,
        available_bytes: None,
    }
}
