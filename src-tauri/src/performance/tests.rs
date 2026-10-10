//! Unit tests for the tracker, startup timer, latency percentiles, and memory probes.

use std::time::Duration;

use super::memory_monitor::MemoryMonitor;
use super::search_latency::SearchLatencyTracker;
use super::startup::StartupTimer;
use super::PerformanceTracker;

#[cfg(target_os = "macos")]
use super::memory_monitor::{macos_current_process_rss_bytes, ps_current_process_rss_bytes};

#[test]
fn input_latency_is_bounded_and_separate_from_backend_latency() {
    let tracker = PerformanceTracker::new();
    tracker.record_search("private query", 5, 1);
    tracker.record_search_input(350);
    tracker.record_search_input(60_001);
    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.search_latency.average_ms, Some(5.0));
    assert_eq!(snapshot.search_input_latency.searches_recorded, 1);
    assert_eq!(snapshot.search_input_latency.average_ms, Some(350.0));
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(json.contains("searchInputLatency"));
    assert!(!json.contains("private query"));
}

#[test]
fn startup_timer_records_segments() {
    let mut timer = StartupTimer::start();
    std::thread::sleep(Duration::from_millis(10));
    let db_open = timer.finish_segment();
    let metrics = timer.finish(5);

    assert!(db_open.as_millis() > 0);
    assert!(metrics.total_startup_ms > 0);
    assert_eq!(metrics.search_init_ms, 5);
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

/// The native task_info probe and the ps fallback sample the same process
/// moments apart, so they must agree in magnitude. Either side may be
/// denied in a hardened sandbox; then the test passes vacuously and the
/// best-effort contract above covers the chain.
#[cfg(target_os = "macos")]
#[test]
fn macos_native_probe_agrees_with_ps_fallback() {
    let (Some(native), Some(fallback)) = (
        macos_current_process_rss_bytes(),
        ps_current_process_rss_bytes(),
    ) else {
        return;
    };
    assert!(native > 0 && fallback > 0);
    let (low, high) = (native.min(fallback), native.max(fallback));
    assert!(
        high < low.saturating_mul(4),
        "native={native} ps={fallback}"
    );
}
