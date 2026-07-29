use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

use crate::content;
use crate::privacy::PrivacyManager;

/// Shared state for ignored applications list, synced between capture thread and Tauri commands.
/// Capture policy shared by every clipboard ingestion path.
#[derive(Clone)]
pub struct CaptureState {
    pub paused: Arc<AtomicBool>,
    pub(crate) max_file_copy_size_bytes: Arc<AtomicU64>,
    pub(crate) ignored_apps: Arc<Mutex<Vec<String>>>,
    pub(crate) policy: Arc<CapturePolicy>,
    pub ingestion_guard: Arc<Mutex<()>>,
    pub(crate) worker: Arc<Mutex<Option<CaptureWorker>>>,
}

#[derive(Clone)]
pub struct CapturePolicy {
    pub sensitive_patterns: Arc<Vec<regex_lite::Regex>>,
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
    ) -> Self {
        let sensitive_patterns = privacy.sensitive_patterns.clone();
        Self {
            paused: Arc::new(AtomicBool::new(privacy.is_paused())),
            max_file_copy_size_bytes: Arc::new(AtomicU64::new(max_file_copy_size_bytes)),
            ignored_apps: Arc::new(Mutex::new(normalize_app_list(&ignored_apps))),
            policy: Arc::new(CapturePolicy {
                sensitive_patterns: Arc::new(sensitive_patterns),
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

    pub(crate) fn set_max_file_copy_size_bytes(&self, value: u64) {
        self.max_file_copy_size_bytes.store(value, Ordering::SeqCst);
    }

    pub(crate) fn max_file_copy_size_bytes(&self) -> u64 {
        self.max_file_copy_size_bytes.load(Ordering::SeqCst)
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
        self.policy.should_skip(&ignored, source_app, text)
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
        source_app: Option<&str>,
        text: Option<&str>,
    ) -> bool {
        if source_app.is_some_and(|app| {
            app_matches(app, ignored_apps) || app_matches(app, &self.password_manager_apps)
        }) {
            return true;
        }

        text.is_some_and(|text| {
            self.sensitive_patterns
                .iter()
                .any(|pattern| pattern.is_match(text))
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
                    eprintln!("[clipboard-worker] capture thread terminated with a panic");
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
