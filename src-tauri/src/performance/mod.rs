use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::process::Command;

use serde::Serialize;

const SEARCH_LATENCY_HISTORY_SIZE: usize = 1000;

pub struct PerformanceTracker {
    startup_metrics: Mutex<Option<StartupMetrics>>,
    search_tracker: SearchLatencyTracker,
    memory_monitor: MemoryMonitor,
}

impl PerformanceTracker {
    pub fn new() -> Self {
        Self {
            startup_metrics: Mutex::new(None),
            search_tracker: SearchLatencyTracker::new(),
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

    pub fn snapshot(&self) -> PerformanceSnapshot {
        let startup = self.startup_metrics.lock().ok().and_then(|m| m.clone());
        // A metrics request is also a sampling point. This keeps the peak
        // value useful even when the caller does not run a background ticker.
        self.memory_monitor.record_snapshot();

        PerformanceSnapshot {
            startup: startup.unwrap_or_default(),
            search_latency: self.search_tracker.summary(),
            memory: self.memory_monitor.snapshot(),
        }
    }
}

impl Default for PerformanceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupMetrics {
    pub total_startup_ms: u64,
    pub db_open_ms: u64,
    pub search_init_ms: u64,
    pub migrations_ms: u64,
}

impl StartupMetrics {
    pub fn log_summary(&self) {
        eprintln!(
            "[perf] startup: {}ms (db: {}ms, search: {}ms, migrations: {}ms)",
            self.total_startup_ms, self.db_open_ms, self.search_init_ms, self.migrations_ms
        );
    }
}

pub struct StartupTimer {
    start: Instant,
    segment_start: Instant,
}

impl StartupTimer {
    pub fn start() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            segment_start: now,
        }
    }

    pub fn finish_segment(&mut self) -> Duration {
        let elapsed = self.segment_start.elapsed();
        self.segment_start = Instant::now();
        elapsed
    }

    pub fn finish(mut self, search_init_ms: u64, migrations_ms: u64) -> StartupMetrics {
        let total = self.start.elapsed();
        let db_open = self.finish_segment();
        StartupMetrics {
            total_startup_ms: total.as_millis() as u64,
            db_open_ms: db_open.as_millis() as u64,
            search_init_ms,
            migrations_ms,
        }
    }
}

#[derive(Debug, Clone)]
struct LatencyEntry {
    duration_ms: u64,
}

pub struct SearchLatencyTracker {
    entries: Mutex<VecDeque<LatencyEntry>>,
}

impl SearchLatencyTracker {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(SEARCH_LATENCY_HISTORY_SIZE)),
        }
    }

    pub fn record_search(&self, _query: &str, duration_ms: u64, _result_count: usize) {
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= SEARCH_LATENCY_HISTORY_SIZE {
                entries.pop_front();
            }
            entries.push_back(LatencyEntry {
                duration_ms,
            });
        }
    }

    pub fn average_latency(&self) -> Option<f64> {
        let entries = self.entries.lock().ok()?;
        if entries.is_empty() {
            return None;
        }
        let total: u64 = entries.iter().map(|e| e.duration_ms).sum();
        Some(total as f64 / entries.len() as f64)
    }

    pub fn p95_latency(&self) -> Option<u64> {
        let entries = self.entries.lock().ok()?;
        if entries.is_empty() {
            return None;
        }
        let mut durations: Vec<u64> = entries.iter().map(|e| e.duration_ms).collect();
        durations.sort_unstable();
        let idx = ((entries.len() as f64) * 0.95).ceil() as usize;
        Some(durations[idx.saturating_sub(1).min(durations.len() - 1)])
    }

    pub fn p99_latency(&self) -> Option<u64> {
        let entries = self.entries.lock().ok()?;
        if entries.is_empty() {
            return None;
        }
        let mut durations: Vec<u64> = entries.iter().map(|e| e.duration_ms).collect();
        durations.sort_unstable();
        let idx = ((entries.len() as f64) * 0.99).ceil() as usize;
        Some(durations[idx.saturating_sub(1).min(durations.len() - 1)])
    }

    pub fn summary(&self) -> SearchLatencySummary {
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let count = entries.len() as u64;
        if entries.is_empty() {
            return SearchLatencySummary {
                searches_recorded: 0,
                average_ms: None,
                p95_ms: None,
                p99_ms: None,
            };
        }
        let total: u64 = entries.iter().map(|e| e.duration_ms).sum();
        let average_ms = Some(total as f64 / entries.len() as f64);

        let mut durations: Vec<u64> = entries.iter().map(|e| e.duration_ms).collect();
        durations.sort_unstable();
        let p95_idx = ((entries.len() as f64) * 0.95).ceil() as usize;
        let p99_idx = ((entries.len() as f64) * 0.99).ceil() as usize;
        let p95_ms = Some(durations[p95_idx.saturating_sub(1).min(durations.len() - 1)]);
        let p99_ms = Some(durations[p99_idx.saturating_sub(1).min(durations.len() - 1)]);

        SearchLatencySummary {
            searches_recorded: count,
            average_ms,
            p95_ms,
            p99_ms,
        }
    }
}

impl Default for SearchLatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchLatencySummary {
    pub searches_recorded: u64,
    pub average_ms: Option<f64>,
    pub p95_ms: Option<u64>,
    pub p99_ms: Option<u64>,
}

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
        let output = Command::new("tasklist")
            .args([
                "/FI",
                &format!("PID eq {}", std::process::id()),
                "/FO",
                "CSV",
                "/NH",
            ])
            .output();
        return output
            .ok()
            .filter(|result| result.status.success())
            .and_then(|result| {
                let line = String::from_utf8_lossy(&result.stdout);
                let memory_field = line.trim().split(',').skip(4).collect::<Vec<_>>().join(",");
                let digits = memory_field
                    .chars()
                    .filter(|character| character.is_ascii_digit())
                    .collect::<String>();
                digits
                    .parse::<u64>()
                    .ok()
                    .map(|kilobytes| kilobytes.saturating_mul(1024))
            })
            .unwrap_or(0);
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output();
        return output
            .ok()
            .filter(|result| result.status.success())
            .and_then(|result| {
                String::from_utf8_lossy(&result.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .map(|kilobytes| kilobytes.saturating_mul(1024))
            .unwrap_or(0);
    }

    #[allow(unreachable_code)]
    0
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceSnapshot {
    pub startup: StartupMetrics,
    pub search_latency: SearchLatencySummary,
    pub memory: MemoryMetrics,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_timer_records_segments() {
        let mut timer = StartupTimer::start();
        std::thread::sleep(Duration::from_millis(10));
        let db_open = timer.finish_segment();
        let metrics = timer.finish(5, 2);

        assert!(db_open.as_millis() > 0);
        assert!(metrics.total_startup_ms > 0);
        assert_eq!(metrics.search_init_ms, 5);
        assert_eq!(metrics.migrations_ms, 2);
    }

    #[test]
    fn search_latency_tracker_computes_percentiles() {
        let tracker = SearchLatencyTracker::new();
        for i in 1..=100 {
            tracker.record_search("test", i * 10, 5);
        }

        let avg = tracker.average_latency().unwrap();
        assert!((avg - 505.0).abs() < 1.0);

        let p95 = tracker.p95_latency().unwrap();
        assert!(p95 >= 950);

        let p99 = tracker.p99_latency().unwrap();
        assert!(p99 >= 990);

        let summary = tracker.summary();
        assert_eq!(summary.searches_recorded, 100);
    }

    #[test]
    fn latency_ring_buffer_keeps_most_recent_1000() {
        let tracker = SearchLatencyTracker::new();
        for i in 0..1500 {
            tracker.record_search("q", i as u64, 1);
        }
        let summary = tracker.summary();
        assert_eq!(summary.searches_recorded, 1000);
    }

    #[test]
    fn memory_monitor_tracks_peak() {
        let monitor = MemoryMonitor::new();
        monitor.record_snapshot();
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.snapshot_count, 1);
        // Peak should be >= current
        assert!(snapshot.peak_bytes >= snapshot.current_bytes);
    }

    #[test]
    fn memory_usage_probe_is_best_effort_and_non_panicking() {
        let usage = MemoryMonitor::new().current_usage_bytes();
        // Some hardened sandboxes do not expose process RSS. In that case
        // zero is an intentional fallback; otherwise the value is bytes.
        if usage > 0 {
            assert_eq!(usage % 1024, 0);
        }
    }

    #[test]
    fn performance_snapshot_serializes() {
        let tracker = PerformanceTracker::new();
        tracker.record_search("test", 25, 10);
        tracker.record_memory_snapshot();
        let snapshot = tracker.snapshot();
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("startup"));
        assert!(json.contains("searchLatency"));
        assert!(json.contains("memory"));
    }
}
