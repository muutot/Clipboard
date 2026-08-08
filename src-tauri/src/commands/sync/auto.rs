use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tauri::Manager;

use crate::commands::sync::run_sync;
use crate::config::ConfigStore;

/// Background worker that periodically runs `run_sync` while the user has
/// auto-sync enabled. The manual `sync_upload_backup` command and this worker
/// share the same `SYNC_RUN_LOCK`, so a manual sync while the worker is running
/// fails fast instead of interleaving the non-reentrant merge/apply/cleanup.
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
                            eprintln!("[auto-sync] config lock poisoned: {e}");
                        }
                        let guard = match lock_result {
                            Ok(guard) => guard,
                            Err(_) => continue,
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
                                eprintln!(
                                    "[auto-sync] done: {} uploaded, {} downloaded, {} applied",
                                    result.uploaded_entries,
                                    result.downloaded_entries,
                                    result.applied_entries,
                                );
                                last_sync_ms = SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    as i64;
                            }
                            Err(e) => {
                                eprintln!("[auto-sync] failed: {e}");
                                last_sync_ms = SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    as i64;
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
            let _ = handle.join();
        }
    }
}

impl Drop for AutoSyncWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
