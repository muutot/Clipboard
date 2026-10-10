//! Startup timing: a segmented timer and the metrics it reports.

use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupMetrics {
    pub total_startup_ms: u64,
    pub db_open_ms: u64,
    pub search_init_ms: u64,
}

impl StartupMetrics {
    pub fn log_summary(&self) {
        crate::log_event!(
            "[perf] startup: {}ms (db: {}ms, search: {}ms)",
            self.total_startup_ms,
            self.db_open_ms,
            self.search_init_ms
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

    pub fn finish(mut self, search_init_ms: u64) -> StartupMetrics {
        let total = self.start.elapsed();
        let db_open = self.finish_segment();
        StartupMetrics {
            total_startup_ms: total.as_millis() as u64,
            db_open_ms: db_open.as_millis() as u64,
            search_init_ms,
        }
    }
}
