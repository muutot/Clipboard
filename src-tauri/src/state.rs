use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread;
use std::thread::JoinHandle;

use crate::content;
use crate::privacy::PrivacyManager;

/// Shared state for ignored applications list, synced between capture thread and Tauri commands.
/// Capture policy shared by every clipboard ingestion path.
#[derive(Clone)]
pub struct CaptureState {
    pub paused: Arc<AtomicBool>,
    pub(crate) capture_sensitive_sources: Arc<AtomicBool>,
    pub(crate) max_file_copy_size_bytes: Arc<AtomicU64>,
    pub(crate) max_text_capture_bytes: Arc<AtomicU64>,
    pub(crate) ignored_apps: Arc<Mutex<Vec<String>>>,
    pub(crate) policy: Arc<CapturePolicy>,
    pub ingestion_guard: Arc<Mutex<()>>,
    pub(crate) worker: Arc<Mutex<Option<CaptureWorker>>>,
}

#[derive(Clone)]
pub struct CapturePolicy {
    pub sensitive_patterns: Arc<RwLock<Vec<regex_lite::Regex>>>,
    pub password_manager_apps: Arc<Vec<String>>,
}

pub struct CaptureWorker {
    pub stop_flag: Arc<AtomicBool>,
    pub stop_sender: Option<mpsc::Sender<()>>,
    pub handle: Option<JoinHandle<()>>,
}

/// Shared state for self-trigger guard to prevent capturing app's own clipboard writes.
#[derive(Clone)]
pub struct SelfTriggerState(pub Arc<Mutex<content::self_trigger::SelfTriggerGuard>>);

impl CaptureState {
    pub fn new(
        privacy: &PrivacyManager,
        ignored_apps: Vec<String>,
        max_file_copy_size_bytes: u64,
        max_text_capture_bytes: u64,
    ) -> Self {
        let sensitive_patterns = privacy.sensitive_patterns.clone();
        Self {
            paused: Arc::new(AtomicBool::new(privacy.is_paused())),
            capture_sensitive_sources: Arc::new(AtomicBool::new(false)),
            max_file_copy_size_bytes: Arc::new(AtomicU64::new(max_file_copy_size_bytes)),
            max_text_capture_bytes: Arc::new(AtomicU64::new(max_text_capture_bytes)),
            ignored_apps: Arc::new(Mutex::new(normalize_app_list(&ignored_apps))),
            policy: Arc::new(CapturePolicy {
                sensitive_patterns: Arc::new(RwLock::new(sensitive_patterns)),
                password_manager_apps: Arc::new(privacy.password_manager_apps.clone()),
            }),
            ingestion_guard: Arc::new(Mutex::new(())),
            worker: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub(crate) fn set_capture_sensitive_sources(&self, value: bool) {
        self.capture_sensitive_sources
            .store(value, Ordering::SeqCst);
    }

    pub(crate) fn capture_sensitive_sources(&self) -> bool {
        self.capture_sensitive_sources.load(Ordering::SeqCst)
    }

    /// Swaps the sensitive-content regex list at runtime. The capture thread
    /// reads the patterns through the same `RwLock`, so the change applies to
    /// the next clipboard event without a worker restart. A poisoned lock is
    /// recovered via `into_inner` rather than silently dropping the update.
    pub(crate) fn set_sensitive_patterns(&self, patterns: Vec<regex_lite::Regex>) {
        let mut guard = match self.policy.sensitive_patterns.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = patterns;
    }

    pub(crate) fn set_max_file_copy_size_bytes(&self, value: u64) {
        self.max_file_copy_size_bytes.store(value, Ordering::SeqCst);
    }

    pub(crate) fn max_file_copy_size_bytes(&self) -> u64 {
        self.max_file_copy_size_bytes.load(Ordering::SeqCst)
    }

    pub(crate) fn set_max_text_capture_bytes(&self, value: u64) {
        self.max_text_capture_bytes.store(value, Ordering::SeqCst);
    }

    pub(crate) fn max_text_capture_bytes(&self) -> u64 {
        self.max_text_capture_bytes.load(Ordering::SeqCst)
    }

    pub(crate) fn set_ignored_apps(&self, apps: Vec<String>) -> Vec<String> {
        let normalized = normalize_app_list(&apps);
        if let Ok(mut ignored) = self.ignored_apps.lock() {
            *ignored = normalized.clone();
        }
        normalized
    }

    pub(crate) fn ignored_apps(&self) -> Vec<String> {
        self.ignored_apps
            .lock()
            .map(|apps| apps.clone())
            .unwrap_or_default()
    }

    pub(crate) fn should_skip(&self, source_app: Option<&str>, text: Option<&str>) -> bool {
        if self.is_paused() {
            return true;
        }

        let ignored = match self.ignored_apps.lock() {
            Ok(apps) => apps,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.policy
            .should_skip(&ignored, self.capture_sensitive_sources(), source_app, text)
    }

    pub fn install_worker(&self, worker: CaptureWorker) {
        self.stop_worker();
        let mut slot = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(worker);
    }

    pub fn stop_worker(&self) {
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(mut worker) = worker {
            worker.stop();
        }
    }

    pub fn worker_running(&self) -> bool {
        self.worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .and_then(|worker| worker.handle.as_ref())
            .is_some_and(|handle| !handle.is_finished())
    }
}

impl CapturePolicy {
    fn should_skip(
        &self,
        ignored_apps: &[String],
        capture_sensitive_sources: bool,
        source_app: Option<&str>,
        text: Option<&str>,
    ) -> bool {
        // The user-managed ignore list always wins.
        if source_app.is_some_and(|app| app_matches(app, ignored_apps)) {
            return true;
        }

        // Password managers and similar sensitive sources are skipped unless
        // the user opted into capturing them.
        if !capture_sensitive_sources
            && source_app.is_some_and(|app| app_matches(app, &self.password_manager_apps))
        {
            return true;
        }

        text.is_some_and(|text| {
            // Privacy filtering must fail closed: if the lock was poisoned by
            // a panicking writer, still evaluate the recovered pattern list
            // instead of letting sensitive content through.
            let patterns = match self.sensitive_patterns.read() {
                Ok(patterns) => patterns,
                Err(poisoned) => poisoned.into_inner(),
            };
            patterns.iter().any(|pattern| pattern.is_match(text))
        })
    }
}

impl CaptureWorker {
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }

        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                if handle.join().is_err() {
                    crate::log_event!("[clipboard-worker] capture thread terminated with a panic");
                }
            } else {
                self.handle = Some(handle);
            }
        }
    }
}

impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn normalize_app_list(apps: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for app in apps {
        let app = app.trim().to_owned();
        if !app.is_empty()
            && !normalized
                .iter()
                .any(|existing| normalize_app_name(existing) == normalize_app_name(&app))
        {
            normalized.push(app);
        }
    }
    normalized
}

fn app_matches(app: &str, candidates: &[String]) -> bool {
    let app = normalize_app_name(app);
    !app.is_empty()
        && candidates
            .iter()
            .map(|candidate| normalize_app_name(candidate))
            .any(|candidate| candidate == app)
}

fn normalize_app_name(app: &str) -> String {
    let trimmed = app.trim();
    let leaf = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let path_name = Path::new(leaf)
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| leaf.to_owned());
    path_name.to_lowercase()
}
