//! macOS process-group probe (best-effort `ps` sample).

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
