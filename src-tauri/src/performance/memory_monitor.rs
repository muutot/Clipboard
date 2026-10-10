//! Best-effort process RSS sampling and the memory metrics the tracker reports.

use std::sync::Mutex;
use std::time::Instant;

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "macos")]
use std::process::Command;

use serde::Serialize;

pub struct MemoryMonitor {
    peak_bytes: Mutex<u64>,
    snapshot_count: Mutex<u64>,
    started_at: Instant,
}

impl MemoryMonitor {
    pub fn new() -> Self {
        Self {
            peak_bytes: Mutex::new(0),
            snapshot_count: Mutex::new(0),
            started_at: Instant::now(),
        }
    }

    pub fn current_usage_bytes(&self) -> u64 {
        current_process_memory_bytes()
    }

    pub fn peak_usage_bytes(&self) -> u64 {
        self.peak_bytes.lock().map(|p| *p).unwrap_or(0)
    }

    pub fn record_snapshot(&self) {
        let current = self.current_usage_bytes();
        if let Ok(mut peak) = self.peak_bytes.lock() {
            if current > *peak {
                *peak = current;
            }
        }
        if let Ok(mut count) = self.snapshot_count.lock() {
            *count += 1;
        }
    }

    pub fn snapshot(&self) -> MemoryMetrics {
        let current_bytes = self.current_usage_bytes();
        let peak_bytes = self
            .peak_bytes
            .lock()
            .map(|mut peak| {
                *peak = (*peak).max(current_bytes);
                *peak
            })
            .unwrap_or(current_bytes);
        let snapshot_count = self.snapshot_count.lock().map(|c| *c).unwrap_or(0);
        MemoryMetrics {
            current_bytes,
            peak_bytes,
            snapshot_count,
            uptime_seconds: self.started_at.elapsed().as_secs(),
        }
    }
}

/// Returns the resident set size of this process. Platform APIs are kept
/// local to this module so metrics remain best-effort and never affect the
/// clipboard pipeline when a platform denies process introspection.
fn current_process_memory_bytes() -> u64 {
    #[cfg(target_os = "linux")]
    {
        return fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    let value = line.strip_prefix("VmRSS:")?.trim();
                    let kilobytes = value.split_whitespace().next()?.parse::<u64>().ok()?;
                    Some(kilobytes.saturating_mul(1024))
                })
            })
            .unwrap_or(0);
    }

    #[cfg(target_os = "windows")]
    {
        // Sample the working set via GetProcessMemoryInfo (shared with the
        // memory diagnostics collector). Spawning tasklist.exe here cost
        // 50-200ms per snapshot and its CSV output mis-parses locales that
        // use '.' as the thousands separator.
        return crate::memory::current_process_working_set_bytes().unwrap_or(0);
    }

    #[cfg(target_os = "macos")]
    {
        // Prefer the in-process task_info query over forking ps(1) per
        // snapshot; fall back to ps when the kernel denies the query so
        // metrics stay best-effort.
        return macos_current_process_rss_bytes()
            .or_else(ps_current_process_rss_bytes)
            .unwrap_or(0);
    }

    #[allow(unreachable_code)]
    0
}

/// Resident set size of the current process via `task_info` with the 64-bit
/// `MACH_TASK_BASIC_INFO` flavor. Binding shapes (`mach_task_basic_info`,
/// flavor/count constants, `task_info_t`) come from the pinned libc crate, so
/// no hand-rolled struct layout is involved. The task port trap itself comes
/// from `mach2` because `libc::mach_task_self` is deprecated.
#[cfg(target_os = "macos")]
pub(super) fn macos_current_process_rss_bytes() -> Option<u64> {
    // SAFETY: a zeroed info struct is valid task_info output; `resident_size`
    // is read via addr_of + read_unaligned because the libc struct is
    // repr(packed(4)).
    unsafe {
        let mut info: libc::mach_task_basic_info = std::mem::zeroed();
        let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
        let ret = libc::task_info(
            mach2::traps::mach_task_self(),
            libc::MACH_TASK_BASIC_INFO,
            &mut info as *mut _ as libc::task_info_t,
            &mut count,
        );
        if ret != libc::KERN_SUCCESS {
            return None;
        }
        Some(std::ptr::addr_of!(info.resident_size).read_unaligned())
    }
}

/// Previous per-snapshot implementation, kept as the fallback: forks ps(1)
/// and parses its plain-integer `rss=` output (no locale-sensitive CSV).
#[cfg(target_os = "macos")]
pub(super) fn ps_current_process_rss_bytes() -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output();
    output
        .ok()
        .filter(|result| result.status.success())
        .and_then(|result| {
            String::from_utf8_lossy(&result.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map(|kilobytes| kilobytes.saturating_mul(1024))
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMetrics {
    pub current_bytes: u64,
    pub peak_bytes: u64,
    pub snapshot_count: u64,
    pub uptime_seconds: u64,
}
