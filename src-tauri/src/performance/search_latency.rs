//! Search latency capture: a bounded ring buffer and its percentile summary.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

const SEARCH_LATENCY_HISTORY_SIZE: usize = 1000;

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
            entries.push_back(LatencyEntry { duration_ms });
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
