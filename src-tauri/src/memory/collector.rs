//! Best-effort memory diagnostics for the desktop process group.
//!
//! The diagnostics command is intentionally read-only.  It samples the
//! current process and descendants (when the platform exposes a process
//! table), then reports process-group totals and system memory.  A denied or
//! unavailable platform probe is represented by `null` rather than making
//! the clipboard pipeline fail.

use std::{
    fs,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{config::ConfigStore, ocr, storage::StoragePaths};

use super::types::*;

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

#[cfg(target_os = "linux")]
mod linux {
    use super::{fallback_current_process, MemoryProcess, SystemMemory};
    use std::{collections::HashSet, fs};

    #[derive(Debug, Clone)]
    struct ProcessEntry {
        pid: u32,
        parent_pid: Option<u32>,
        name: String,
    }

    pub(super) fn collect_processes(current_pid: u32) -> Vec<MemoryProcess> {
        let entries = enumerate_processes();
        let selected = descendant_pids(current_pid, &entries);
        let mut processes = entries
            .into_iter()
            .filter(|entry| selected.contains(&entry.pid))
            .map(|entry| {
                let metrics = read_process_metrics(entry.pid);
                MemoryProcess {
                    pid: entry.pid,
                    parent_pid: entry.parent_pid,
                    name: entry.name.clone(),
                    role: Some(if entry.pid == current_pid {
                        "main".to_owned()
                    } else if is_webview_process(&entry.name) {
                        "webview".to_owned()
                    } else {
                        "child".to_owned()
                    }),
                    working_set_bytes: metrics.0,
                    private_bytes: metrics.1,
                    private_working_set_bytes: metrics.2,
                    virtual_bytes: metrics.3,
                }
            })
            .collect::<Vec<_>>();

        if !processes.iter().any(|process| process.pid == current_pid) {
            processes.push(fallback_current_process(current_pid));
        }
        processes
    }

    pub(super) fn collect_system_memory() -> SystemMemory {
        let Ok(contents) = fs::read_to_string("/proc/meminfo") else {
            return SystemMemory {
                total_bytes: None,
                available_bytes: None,
            };
        };
        let mut total_bytes = None;
        let mut available_bytes = None;
        for line in contents.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let Some(bytes) = parse_proc_value(value) else {
                continue;
            };
            match key.trim() {
                "MemTotal" => total_bytes = Some(bytes),
                "MemAvailable" => available_bytes = Some(bytes),
                _ => {}
            }
        }
        SystemMemory {
            total_bytes,
            available_bytes,
        }
    }

    fn enumerate_processes() -> Vec<ProcessEntry> {
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
                let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
                let parent_pid = parse_parent_pid(&stat);
                let name = fs::read_to_string(entry.path().join("comm"))
                    .ok()
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "unknown".to_owned());
                Some(ProcessEntry {
                    pid,
                    parent_pid,
                    name,
                })
            })
            .collect()
    }

    fn descendant_pids(current_pid: u32, entries: &[ProcessEntry]) -> HashSet<u32> {
        let mut selected = HashSet::from([current_pid]);
        let mut changed = true;
        while changed {
            changed = false;
            for entry in entries {
                if entry
                    .parent_pid
                    .is_some_and(|parent_pid| selected.contains(&parent_pid))
                    && selected.insert(entry.pid)
                {
                    changed = true;
                }
            }
        }
        selected
    }

    fn parse_parent_pid(stat: &str) -> Option<u32> {
        let close = stat.rfind(')')?;
        let fields = stat
            .get(close + 1..)?
            .split_whitespace()
            .collect::<Vec<_>>();
        fields.get(1)?.parse().ok()
    }

    fn read_process_metrics(pid: u32) -> (Option<u64>, Option<u64>, Option<u64>, Option<u64>) {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).ok();
        let working_set = status
            .as_deref()
            .and_then(|status| find_proc_value(status, "VmRSS"));
        let virtual_bytes = status
            .as_deref()
            .and_then(|status| find_proc_value(status, "VmSize"));
        let private_working_set = fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
            .ok()
            .and_then(|smaps| {
                let mut found = false;
                let total = smaps
                    .lines()
                    .filter(|line| line.starts_with("Private_"))
                    .filter_map(|line| {
                        let value = line
                            .split_once(':')
                            .and_then(|(_, value)| parse_proc_value(value))?;
                        found = true;
                        Some(value)
                    })
                    .fold(0u64, u64::saturating_add);
                found.then_some(total)
            });
        (working_set, None, private_working_set, virtual_bytes)
    }

    fn find_proc_value(contents: &str, key: &str) -> Option<u64> {
        contents.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key)
                .then(|| parse_proc_value(value))
                .flatten()
        })
    }

    fn parse_proc_value(value: &str) -> Option<u64> {
        let mut parts = value.split_whitespace();
        let number = parts.next()?.parse::<u64>().ok()?;
        let multiplier = match parts.next().unwrap_or("B").to_ascii_lowercase().as_str() {
            "kb" => 1024,
            "mb" => 1024 * 1024,
            "gb" => 1024 * 1024 * 1024,
            _ => 1,
        };
        Some(number.saturating_mul(multiplier))
    }

    fn is_webview_process(name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        name.contains("webview") || name.contains("msedge")
    }

    #[cfg(test)]
    mod tests {
        use super::{parse_parent_pid, parse_proc_value};

        #[test]
        fn parses_linux_process_stat_parent_with_spaces_in_name() {
            assert_eq!(parse_parent_pid("42 (worker process) S 7 8 9"), Some(7));
        }

        #[test]
        fn parses_proc_units_as_bytes() {
            assert_eq!(parse_proc_value("12 kB"), Some(12 * 1024));
            assert_eq!(parse_proc_value("3 MB"), Some(3 * 1024 * 1024));
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{fallback_current_process, MemoryProcess};
    use std::process::Command;

    pub(super) fn collect_processes(current_pid: u32) -> Vec<MemoryProcess> {
        let mut process = fallback_current_process(current_pid);
        if let Ok(output) = Command::new("ps")
            .args(["-o", "rss=,vsz=", "-p", &current_pid.to_string()])
            .output()
        {
            if output.status.success() {
                let values = String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .filter_map(|value| value.parse::<u64>().ok())
                    .collect::<Vec<_>>();
                process.working_set_bytes = values
                    .first()
                    .copied()
                    .map(|value| value.saturating_mul(1024));
                process.virtual_bytes = values
                    .get(1)
                    .copied()
                    .map(|value| value.saturating_mul(1024));
            }
        }
        vec![process]
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{fallback_current_process, MemoryProcess, SystemMemory};
    use std::{
        collections::HashSet,
        ffi::c_void,
        mem::{size_of, zeroed},
    };

    /// Resident set size of the current process via
    /// `GetProcessMemoryInfo`, shared with the performance metrics panel so
    /// it does not need to spawn a helper process per sample.
    pub(super) fn current_process_working_set_bytes() -> Option<u64> {
        current_process_memory().0
    }

    type Handle = isize;
    const INVALID_HANDLE_VALUE: Handle = -1;
    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    impl Default for ProcessEntry32W {
        fn default() -> Self {
            unsafe { zeroed() }
        }
    }

    #[repr(C)]
    struct ProcessMemoryCountersEx {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }

    #[repr(C)]
    struct ProcessMemoryCountersEx2 {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
        private_working_set_size: usize,
        shared_commit_usage: u64,
    }

    #[repr(C)]
    struct MemoryStatusEx {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page_file: u64,
        ull_avail_page_file: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetCurrentProcess"]
        fn get_current_process() -> Handle;
        #[link_name = "OpenProcess"]
        fn open_process(access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        #[link_name = "CloseHandle"]
        fn close_handle(handle: Handle) -> i32;
        #[link_name = "CreateToolhelp32Snapshot"]
        fn create_toolhelp32_snapshot(flags: u32, process_id: u32) -> Handle;
        #[link_name = "Process32FirstW"]
        fn process32_first_w(snapshot: Handle, entry: *mut ProcessEntry32W) -> i32;
        #[link_name = "Process32NextW"]
        fn process32_next_w(snapshot: Handle, entry: *mut ProcessEntry32W) -> i32;
        #[link_name = "GlobalMemoryStatusEx"]
        fn global_memory_status_ex(status: *mut MemoryStatusEx) -> i32;
    }

    #[link(name = "psapi")]
    unsafe extern "system" {
        #[link_name = "GetProcessMemoryInfo"]
        fn get_process_memory_info(process: Handle, counters: *mut c_void, size: u32) -> i32;
    }

    #[derive(Debug, Clone)]
    struct ProcessEntry {
        pid: u32,
        parent_pid: Option<u32>,
        name: String,
    }

    pub(super) fn collect_processes(current_pid: u32) -> Vec<MemoryProcess> {
        let entries = enumerate_processes();
        let selected = descendant_pids(current_pid, &entries);
        let mut processes = entries
            .into_iter()
            .filter(|entry| selected.contains(&entry.pid))
            .map(|entry| {
                let (working_set_bytes, private_bytes, private_working_set_bytes, virtual_bytes) =
                    process_memory(entry.pid);
                let role = if entry.pid == current_pid {
                    Some("main".to_owned())
                } else if is_webview_process(&entry.name) {
                    Some("webview".to_owned())
                } else {
                    Some("child".to_owned())
                };
                MemoryProcess {
                    pid: entry.pid,
                    parent_pid: entry.parent_pid,
                    name: entry.name,
                    role,
                    working_set_bytes,
                    private_bytes,
                    private_working_set_bytes,
                    virtual_bytes,
                }
            })
            .collect::<Vec<_>>();

        if !processes.iter().any(|process| process.pid == current_pid) {
            let mut process = fallback_current_process(current_pid);
            let (working_set_bytes, private_bytes, private_working_set_bytes, virtual_bytes) =
                current_process_memory();
            process.working_set_bytes = working_set_bytes;
            process.private_bytes = private_bytes;
            process.private_working_set_bytes = private_working_set_bytes;
            process.virtual_bytes = virtual_bytes;
            processes.push(process);
        }
        processes
    }

    pub(super) fn collect_system_memory() -> SystemMemory {
        let mut status = MemoryStatusEx {
            dw_length: size_of::<MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        let ok = unsafe { global_memory_status_ex(&mut status) != 0 };
        if ok {
            SystemMemory {
                total_bytes: Some(status.ull_total_phys),
                available_bytes: Some(status.ull_avail_phys),
            }
        } else {
            SystemMemory {
                total_bytes: None,
                available_bytes: None,
            }
        }
    }

    fn enumerate_processes() -> Vec<ProcessEntry> {
        let snapshot = unsafe { create_toolhelp32_snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == 0 || snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }

        let mut entry = ProcessEntry32W {
            dw_size: size_of::<ProcessEntry32W>() as u32,
            ..Default::default()
        };
        let mut result = Vec::new();
        let first_ok = unsafe { process32_first_w(snapshot, &mut entry) != 0 };
        if first_ok {
            loop {
                result.push(ProcessEntry {
                    pid: entry.th32_process_id,
                    parent_pid: (entry.th32_parent_process_id != 0)
                        .then_some(entry.th32_parent_process_id),
                    name: utf16_name(&entry.sz_exe_file),
                });
                if unsafe { process32_next_w(snapshot, &mut entry) == 0 } {
                    break;
                }
            }
        }
        unsafe {
            close_handle(snapshot);
        }
        result
    }

    fn descendant_pids(current_pid: u32, entries: &[ProcessEntry]) -> HashSet<u32> {
        let mut selected = HashSet::from([current_pid]);
        let mut changed = true;
        while changed {
            changed = false;
            for entry in entries {
                if entry
                    .parent_pid
                    .is_some_and(|parent_pid| selected.contains(&parent_pid))
                    && selected.insert(entry.pid)
                {
                    changed = true;
                }
            }
        }
        selected
    }

    fn process_memory(pid: u32) -> (Option<u64>, Option<u64>, Option<u64>, Option<u64>) {
        let process = unsafe { open_process(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process == 0 || process == INVALID_HANDLE_VALUE {
            return (None, None, None, None);
        }
        let result = query_process_memory(process);
        unsafe {
            close_handle(process);
        }
        result
    }

    fn current_process_memory() -> (Option<u64>, Option<u64>, Option<u64>, Option<u64>) {
        let process = unsafe { get_current_process() };
        query_process_memory(process)
    }

    fn query_process_memory(
        process: Handle,
    ) -> (Option<u64>, Option<u64>, Option<u64>, Option<u64>) {
        let mut counters_ex2 = ProcessMemoryCountersEx2 {
            cb: size_of::<ProcessMemoryCountersEx2>() as u32,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool_usage: 0,
            quota_paged_pool_usage: 0,
            quota_peak_non_paged_pool_usage: 0,
            quota_non_paged_pool_usage: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
            private_usage: 0,
            private_working_set_size: 0,
            shared_commit_usage: 0,
        };
        let ex2_ok = unsafe {
            get_process_memory_info(
                process,
                (&mut counters_ex2 as *mut ProcessMemoryCountersEx2).cast::<c_void>(),
                size_of::<ProcessMemoryCountersEx2>() as u32,
            ) != 0
        };
        if ex2_ok {
            return (
                Some(counters_ex2.working_set_size as u64),
                Some(counters_ex2.private_usage as u64),
                Some(counters_ex2.private_working_set_size as u64),
                Some(counters_ex2.pagefile_usage as u64),
            );
        }

        let mut counters = ProcessMemoryCountersEx {
            cb: size_of::<ProcessMemoryCountersEx>() as u32,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool_usage: 0,
            quota_paged_pool_usage: 0,
            quota_peak_non_paged_pool_usage: 0,
            quota_non_paged_pool_usage: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
            private_usage: 0,
        };
        let ok = unsafe {
            get_process_memory_info(
                process,
                (&mut counters as *mut ProcessMemoryCountersEx).cast::<c_void>(),
                size_of::<ProcessMemoryCountersEx>() as u32,
            ) != 0
        };
        if !ok {
            return (None, None, None, None);
        }
        (
            Some(counters.working_set_size as u64),
            Some(counters.private_usage as u64),
            None,
            Some(counters.pagefile_usage as u64),
        )
    }

    fn utf16_name(value: &[u16]) -> String {
        let end = value
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(value.len());
        String::from_utf16_lossy(&value[..end])
    }

    fn is_webview_process(name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        name.contains("webview") || name.contains("msedge")
    }
}
