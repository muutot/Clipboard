//! Linux process-group and system-memory probes (`/proc` based).

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
