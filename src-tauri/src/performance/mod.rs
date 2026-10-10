//! Read-only performance metrics: startup timing, search latency, and memory.

use std::sync::Mutex;

use serde::Serialize;

mod memory_monitor;
mod search_latency;
mod startup;

#[cfg(test)]
mod tests;

pub use memory_monitor::{MemoryMetrics, MemoryMonitor};
pub use search_latency::{SearchLatencySummary, SearchLatencyTracker};
pub use startup::{StartupMetrics, StartupTimer};

pub struct PerformanceTracker {
    startup_metrics: Mutex<Option<StartupMetrics>>,
    search_tracker: SearchLatencyTracker,
    search_input_tracker: SearchLatencyTracker,
    memory_monitor: MemoryMonitor,
}

impl PerformanceTracker {
    pub fn new() -> Self {
        Self {
            startup_metrics: Mutex::new(None),
            search_tracker: SearchLatencyTracker::new(),
            search_input_tracker: SearchLatencyTracker::new(),
            memory_monitor: MemoryMonitor::new(),
        }
    }

    pub fn record_startup(&self, metrics: StartupMetrics) {
        if let Ok(mut stored) = self.startup_metrics.lock() {
            *stored = Some(metrics);
        }
    }

    pub fn record_search(&self, query: &str, duration_ms: u64, result_count: usize) {
        self.search_tracker
            .record_search(query, duration_ms, result_count);
    }

    pub fn record_memory_snapshot(&self) {
        self.memory_monitor.record_snapshot();
    }

    /// Frontend-visible interaction duration, including debounce and a frame boundary.
    pub fn record_search_input(&self, duration_ms: u64) {
        if duration_ms <= 60_000 {
            self.search_input_tracker.record_search("", duration_ms, 0);
        }
    }

    pub fn snapshot(&self) -> PerformanceSnapshot {
        let startup = self.startup_metrics.lock().ok().and_then(|m| m.clone());
        // A metrics request is also a sampling point. This keeps the peak
        // value useful even when the caller does not run a background ticker.
        self.memory_monitor.record_snapshot();

        PerformanceSnapshot {
            startup: startup.unwrap_or_default(),
            search_latency: self.search_tracker.summary(),
            search_input_latency: self.search_input_tracker.summary(),
            memory: self.memory_monitor.snapshot(),
        }
    }
}

impl Default for PerformanceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceSnapshot {
    pub search_input_latency: SearchLatencySummary,
    pub startup: StartupMetrics,
    pub search_latency: SearchLatencySummary,
    pub memory: MemoryMetrics,
}
