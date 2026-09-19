use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tauri::Manager;

use crate::commands::sync::run_sync;
use crate::config::ConfigStore;

/// How long `stop` waits for an in-flight `run_sync` before leaking the
/// worker thread. A network-slow run has no cancellation checkpoints, so an
/// unbounded join could hang exit/restart until the S3 timeouts fire (up to
/// 30 minutes for a streaming transfer).
const STOP_JOIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Background worker that periodically runs the same S3-first v1 engine as the
/// manual `sync_now` command. Both paths share `SYNC_RUN_LOCK`, so a second run
/// fails fast instead of interleaving local publication state.
///
/// The worker owns an `AppHandle` only; all managed states are resolved from it
/// on each tick. Config is re-read every second so a settings change to
/// `auto_sync` or `auto_sync_interval_secs` takes effect without a restart.
pub struct AutoSyncWorker {
    stop_flag: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl AutoSyncWorker {
    pub fn start(app: tauri::AppHandle) -> Result<Self, String> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let worker_stop_flag = Arc::clone(&stop_flag);

        let handle = std::thread::Builder::new()
            .name("auto-sync".to_owned())
            .spawn(move || {
                let mut last_sync_ms: i64 = 0;
                while !worker_stop_flag.load(Ordering::Relaxed) {
                    let (enabled, interval_secs) = {
                        let config = app.state::<std::sync::Mutex<ConfigStore>>();
                        let lock_result = config.lock();
                        if let Err(e) = &lock_result {
                            crate::log_event!("[auto-sync] config lock poisoned: {e}");
                        }
                        let guard = match lock_result {
                            Ok(guard) => guard,
                            // Sleep before retrying: falling through to the
                            // loop-end sleep keeps a poisoned lock from
                            // spinning the thread at full CPU.
                            Err(_) => {
                                std::thread::sleep(Duration::from_millis(1000));
                                continue;
                            }
                        };
                        // Read the two values, then drop the guard so it no
                        // longer borrows the config state across the loop body.
                        let enabled = guard.auto_sync();
                        let interval_secs = guard.auto_sync_interval_secs();
                        drop(guard);
                        (enabled, interval_secs)
                    };
                    let interval_secs = interval_secs.max(1);

                    let now_ms = SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;

                    if enabled && now_ms - last_sync_ms >= interval_secs as i64 * 1000 {
                        match run_sync(&app) {
                            Ok(result) => {
                                crate::log_event!(
                                    "[auto-sync] done: {} uploaded, {} downloaded, {} applied, {} peers failed",
                                    result.uploaded_entries,
                                    result.downloaded_entries,
                                    result.applied_entries,
                                    result.failed_peers,
                                );
                                last_sync_ms = SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    as i64;
                            }
                            Err(e) => {
                                // A manual sync holding SYNC_RUN_LOCK is not
                                // a completed auto-sync cycle: keep the old
                                // last_sync_ms so the next tick retries
                                // immediately instead of deferring a full
                                // interval.
                                if e != "sync already in progress" {
                                    last_sync_ms = SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                        as i64;
                                }
                                crate::log_event!("[auto-sync] failed: {e}");
                            }
                        }
                    }

                    std::thread::sleep(Duration::from_millis(1000));
                }
            })
            .map_err(|e| format!("failed to start auto-sync worker: {e}"))?;

        Ok(Self {
            stop_flag,
            handle: Some(handle),
        })
    }

    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // Join through a helper thread so the wait is bounded: if the
            // worker is stuck in a long sync, log and leak it — the stop flag
            // stays set (the loop exits after the in-flight run) and process
            // exit reclaims the thread.
            let (done_tx, done_rx) = mpsc::channel::<()>();
            let join_helper = std::thread::Builder::new()
                .name("auto-sync-join".to_owned())
                .spawn(move || {
                    if let Err(panic) = handle.join() {
                        crate::log_event!("[auto-sync] worker terminated with a panic: {panic:?}");
                    }
                    let _ = done_tx.send(());
                });
            match join_helper {
                Ok(_) => {
                    if done_rx.recv_timeout(STOP_JOIN_TIMEOUT).is_err() {
                        crate::log_event!(
                            "[auto-sync] worker still running after 30s; leaking the thread so shutdown can proceed"
                        );
                    }
                }
                Err(error) => {
                    crate::log_event!("[auto-sync] failed to spawn the join helper: {error}");
                }
            }
        }
    }
}

impl Drop for AutoSyncWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
