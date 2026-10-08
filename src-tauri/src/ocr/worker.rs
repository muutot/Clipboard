use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::domain::OcrResult;
use crate::storage::{Database, OcrRepository};

use super::{OcrEngine, OcrInput};

/// Total recognition passes for one image, the first attempt included.
///
/// The bound is what keeps a permanently unrecognizable image (a truncated
/// PNG, a file another process still holds) from spinning: without it every
/// failure would be requeued and the worker would burn inference attempts on
/// the same bytes forever. Two retries past the first attempt cover the
/// transient classes that actually occur — an ONNX Runtime allocation failure
/// under memory pressure, a model file being replaced mid-session, a Windows
/// file lock — while a user-triggered regenerate from the detail panel stays
/// unlimited.
const MAX_RECOGNITION_ATTEMPTS: u32 = 3;

/// Delay before the given number of attempts has been made, doubling per
/// attempt so a failing image cannot monopolize the single worker slot. The
/// exponent is capped so a large count cannot overflow the shift.
fn retry_backoff(attempts_made: u32) -> Duration {
    Duration::from_secs(1u64 << attempts_made.min(4))
}

/// An image whose recognition failed but still has attempts left.
///
/// The retry is held in memory rather than requeued through the database on
/// purpose: `retry_ocr` would set the row back to `pending`, and
/// `claim_next_ocr` orders by `created_at_ms`, so the same row would be
/// claimed again on the very next poll and the other pending images would
/// wait out the whole backoff. Keeping the row in `processing` also means the
/// UI honestly shows "recognizing" for the whole window, and a crash mid
/// retry is already covered — startup `requeue_interrupted_ocr` turns
/// `processing` rows back into `pending`.
struct PendingRetry {
    input: OcrInput,
    attempts_made: u32,
    due: Instant,
}

/// Owns the resources used by an OCR worker thread.
///
/// The worker is cloned by the Tauri state and by the shutdown handler.  The
/// thread handle and stop sender therefore live behind a shared `Arc`, so any
/// clone can request a stop while the last owner is still responsible for
/// joining the thread during drop.
struct WorkerInner {
    running: Arc<AtomicBool>,
    stop_sender: Mutex<Option<mpsc::Sender<()>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
pub struct OcrWorker {
    inner: Arc<WorkerInner>,
}

#[derive(Clone)]
pub struct OcrWorkerManager {
    worker: Arc<Mutex<OcrWorker>>,
}

impl OcrWorkerManager {
    pub fn start(engine: Arc<dyn OcrEngine>, database: Arc<Database>) -> Result<Self, String> {
        Ok(Self {
            worker: Arc::new(Mutex::new(OcrWorker::start(engine, database)?)),
        })
    }

    pub fn restart(
        &self,
        engine: Arc<dyn OcrEngine>,
        database: Arc<Database>,
    ) -> Result<(), String> {
        let mut worker = lock_unpoisoned(&self.worker);
        worker.stop();
        *worker = OcrWorker::start(engine, database)?;
        Ok(())
    }

    pub fn stop(&self) {
        lock_unpoisoned(&self.worker).stop();
    }

    pub fn is_running(&self) -> bool {
        lock_unpoisoned(&self.worker).is_running()
    }
}

impl OcrWorker {
    pub fn start(engine: Arc<dyn OcrEngine>, database: Arc<Database>) -> Result<Self, String> {
        // A previous process may have exited after claiming a task.  Recover
        // those rows before this worker starts claiming new work.  The startup
        // path also performs this repair, but doing it here covers hot engine
        // restarts and keeps worker construction self-contained.
        if let Err(error) = database.requeue_interrupted_ocr() {
            crate::log_error!("[ocr] failed to requeue interrupted tasks: {error}");
        }

        let running = Arc::new(AtomicBool::new(true));
        let (stop_sender, stop_receiver) = mpsc::channel();
        let inner = Arc::new(WorkerInner {
            running: Arc::clone(&running),
            stop_sender: Mutex::new(Some(stop_sender)),
            handle: Mutex::new(None),
        });

        let worker_running = Arc::clone(&running);
        let handle = thread::Builder::new()
            .name("ocr-worker".to_owned())
            .spawn(move || {
                Self::run_loop(engine, database, worker_running, stop_receiver);
            })
            .map_err(|error| format!("failed to spawn OCR worker thread: {error}"))?;

        *lock_unpoisoned(&inner.handle) = Some(handle);

        Ok(Self { inner })
    }

    /// Requests shutdown and waits for the worker thread to finish.
    ///
    /// Joining is idempotent across clones: only the clone that takes the
    /// shared handle performs the actual join.  The stop channel wakes a
    /// worker that is waiting for its next polling interval immediately.
    pub fn stop(&self) {
        self.inner.running.store(false, Ordering::SeqCst);

        let sender = lock_unpoisoned(&self.inner.stop_sender).take();
        if let Some(sender) = sender {
            let _ = sender.send(());
        }

        self.join_thread();
    }

    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }

    fn join_thread(&self) {
        let handle = lock_unpoisoned(&self.inner.handle).take();
        let Some(handle) = handle else {
            // Another clone may be joining the shared handle.  Wait for the
            // worker's running flag to be cleared so every stop caller gets a
            // synchronous shutdown guarantee — but never hang on it: an
            // in-flight inference (e.g. ppocr on a large image) can take
            // minutes, so bound the wait and let shutdown proceed.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while self.inner.running.load(Ordering::SeqCst) && std::time::Instant::now() < deadline
            {
                thread::yield_now();
            }
            if self.inner.running.load(Ordering::SeqCst) {
                crate::log_error!("[ocr] worker still running 10s after stop; detaching");
            }
            return;
        };

        // A future implementation may call stop from inside the worker.  A
        // thread cannot join itself, so leave the handle available for the
        // final owner to join after the worker returns.
        if handle.thread().id() == thread::current().id() {
            *lock_unpoisoned(&self.inner.handle) = Some(handle);
            return;
        }

        // Bound the join: a stuck engine.recognize (ppocr has no timeout)
        // must not hang app shutdown indefinitely.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !handle.is_finished() && std::time::Instant::now() < deadline {
            thread::sleep(std::time::Duration::from_millis(50));
        }
        if handle.is_finished() {
            if handle.join().is_err() {
                crate::log_error!("[ocr] worker thread terminated with a panic");
            }
        } else {
            crate::log_error!("[ocr] worker thread did not finish within 10s of stop; detaching");
        }
    }

    fn run_loop(
        engine: Arc<dyn OcrEngine>,
        database: Arc<Database>,
        running: Arc<AtomicBool>,
        stop_receiver: mpsc::Receiver<()>,
    ) {
        let _running_guard = RunningGuard(Arc::clone(&running));
        let poll_interval = Duration::from_millis(500);
        let mut consecutive_errors = 0u32;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;
        let mut retries: VecDeque<PendingRetry> = VecDeque::new();

        while running.load(Ordering::SeqCst) {
            // Do not claim another item after a stop request raced with the
            // previous task.  `try_recv` also notices a dropped sender.
            if stop_requested(&stop_receiver) {
                break;
            }

            // A due retry runs before a fresh claim so a transient failure is
            // recovered while it is still recent. Every entry is bounded by
            // MAX_RECOGNITION_ATTEMPTS, so the queue drains and cannot starve
            // newly captured images.
            let due_retry = retries
                .iter()
                .position(|retry| retry.due <= Instant::now())
                .and_then(|position| retries.remove(position));

            let mut worked = false;

            if let Some(mut retry) = due_retry {
                worked = true;
                if let Err(message) = attempt_recognition(&engine, &database, &retry.input) {
                    retry.attempts_made += 1;
                    let attempts_made = retry.attempts_made;
                    handle_recognition_failure(
                        &database,
                        retry.input,
                        attempts_made,
                        &message,
                        &mut retries,
                    );
                }
            } else {
                match database.claim_next_ocr() {
                    Ok(Some(input)) => {
                        consecutive_errors = 0;
                        worked = true;

                        // A regenerate from the detail panel put this row back
                        // to `pending`; drop any retry this worker still held
                        // for it so the two paths cannot both recognize it.
                        retries.retain(|retry| retry.input.item_id != input.item_id);

                        // Check if an OCR result for this image hash already
                        // exists.  Reusing it avoids duplicate local inference.
                        if let Ok(Some(existing)) =
                            database.find_completed_ocr_by_hash(&input.image_hash)
                        {
                            let result = OcrResult::completed(
                                &input.item_id,
                                &existing.engine,
                                &existing.model_version,
                                existing.language.as_deref(),
                                &existing.full_text,
                                &existing.blocks,
                                &input.image_hash,
                            );
                            if let Err(error) = database.save_ocr_result(&result) {
                                let message = format!("failed to save reused OCR result: {error}");
                                crate::log_event!("[ocr] {message} for {}", input.item_id);
                                persist_failure(&database, &input.item_id, &message);
                            }
                        } else if let Err(message) = attempt_recognition(&engine, &database, &input)
                        {
                            handle_recognition_failure(&database, input, 1, &message, &mut retries);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        crate::log_error!("[ocr] failed to claim next task: {error}");
                        consecutive_errors = consecutive_errors.saturating_add(1);
                        if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                            crate::log_warn!(
                                "[ocr] too many consecutive errors ({}), pausing",
                                consecutive_errors
                            );
                            if wait_for_stop(&stop_receiver, Duration::from_secs(10)) {
                                break;
                            }
                            consecutive_errors = 0;
                        }
                    }
                }
            }

            if !worked && wait_for_stop(&stop_receiver, poll_interval) {
                break;
            }
        }

        running.store(false, Ordering::SeqCst);
    }
}

/// Decides what a failed recognition pass means: the budget is spent, so the
/// failure becomes the row's persisted state, or another attempt is scheduled.
///
/// Both the first attempt and every retry go through here on purpose. Checking
/// the budget only on the retry path made `MAX_RECOGNITION_ATTEMPTS = 1` mean
/// two passes, which the attempt-count test caught.
fn handle_recognition_failure(
    database: &Database,
    input: OcrInput,
    attempts_made: u32,
    message: &str,
    retries: &mut VecDeque<PendingRetry>,
) {
    if attempts_made >= MAX_RECOGNITION_ATTEMPTS {
        crate::log_error!(
            "[ocr] giving up on {} after {} attempts: {}",
            input.item_id,
            attempts_made,
            message
        );
        persist_failure(database, &input.item_id, message);
        return;
    }

    retries.push_back(PendingRetry {
        input,
        attempts_made,
        due: Instant::now() + retry_backoff(attempts_made),
    });
}

/// Runs one recognition pass and persists a success.
///
/// A recognition failure returns its message instead of persisting it: the
/// caller decides whether another attempt is still allowed, and only the last
/// failure becomes the row's persisted state. A failure to *store* a
/// successful recognition is terminal and is persisted immediately, because
/// retrying the inference would not fix the write.
fn attempt_recognition(
    engine: &Arc<dyn OcrEngine>,
    database: &Database,
    input: &OcrInput,
) -> Result<(), String> {
    let engine_name = engine.name();
    let model_version = engine.model_version();

    match engine.recognize(input) {
        Ok(output) => {
            let result = OcrResult::completed(
                &input.item_id,
                engine_name,
                model_version,
                output.language.as_deref(),
                &output.full_text,
                &output.blocks,
                &input.image_hash,
            );
            if let Err(error) = database.save_ocr_result(&result) {
                let message = format!("failed to save OCR result: {error}");
                crate::log_event!("[ocr] {message} for {}", input.item_id);
                persist_failure(database, &input.item_id, &message);
            }
            Ok(())
        }
        Err(error) => {
            let message = error.to_string();
            crate::log_error!(
                "[ocr] recognition failed for {}: {}",
                input.item_id,
                message
            );
            Err(message)
        }
    }
}

fn persist_failure(database: &Database, item_id: &str, message: &str) {
    if let Err(error) = database.mark_ocr_failed(item_id, message) {
        crate::log_error!("[ocr] failed to persist failure for {item_id}: {error}");
    }
}

struct RunningGuard(Arc<AtomicBool>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn stop_requested(receiver: &mpsc::Receiver<()>) -> bool {
    match receiver.try_recv() {
        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => true,
        Err(mpsc::TryRecvError::Empty) => false,
    }
}

fn wait_for_stop(receiver: &mpsc::Receiver<()>, timeout: Duration) -> bool {
    match receiver.recv_timeout(timeout) {
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => true,
        Err(mpsc::RecvTimeoutError::Timeout) => false,
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Drop for OcrWorker {
    fn drop(&mut self) {
        // A clone held by the application or shutdown callback still owns the
        // worker.  Only the final owner stops and joins it implicitly.
        if Arc::strong_count(&self.inner) == 1 {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus};
    use crate::ocr::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};
    use crate::storage::{ClipboardRepository, Database, OcrRepository};

    use super::{retry_backoff, OcrWorker, OcrWorkerManager, MAX_RECOGNITION_ATTEMPTS};

    fn image_item(id: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Image,
            title: format!("{id}.png"),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: Some(format!("{id}.png")),
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: None,
            size_bytes: 1,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    struct FailingEngine {
        calls: Arc<AtomicUsize>,
        dropped: Arc<AtomicBool>,
    }

    impl OcrEngine for FailingEngine {
        fn name(&self) -> &'static str {
            "test"
        }

        fn model_version(&self) -> &str {
            "test-1"
        }

        fn recognize(&self, _input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(OcrEngineError::new("synthetic OCR failure"))
        }
    }

    impl Drop for FailingEngine {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    struct SuccessfulEngine {
        calls: Arc<AtomicUsize>,
    }

    impl OcrEngine for SuccessfulEngine {
        fn name(&self) -> &'static str {
            "test"
        }

        fn model_version(&self) -> &str {
            "test-1"
        }

        fn recognize(&self, _input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(OcrOutput {
                language: Some("en".to_owned()),
                full_text: "recognized".to_owned(),
                blocks: Vec::new(),
            })
        }
    }

    struct FlakyEngine {
        calls: Arc<AtomicUsize>,
        failures_before_success: usize,
    }

    impl OcrEngine for FlakyEngine {
        fn name(&self) -> &'static str {
            "test"
        }

        fn model_version(&self) -> &str {
            "test-1"
        }

        fn recognize(&self, _input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call < self.failures_before_success {
                Err(OcrEngineError::new("synthetic transient failure"))
            } else {
                Ok(OcrOutput {
                    language: Some("en".to_owned()),
                    full_text: "recognized after retry".to_owned(),
                    blocks: Vec::new(),
                })
            }
        }
    }

    #[test]
    fn stop_joins_worker_and_releases_engine() {
        let dropped = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let database = Arc::new(Database::open_in_memory().unwrap());
        let engine = Arc::new(FailingEngine {
            calls: Arc::clone(&calls),
            dropped: Arc::clone(&dropped),
        });

        let worker = OcrWorker::start(engine, database).unwrap();
        worker.stop();

        assert!(!worker.is_running());
        assert!(dropped.load(Ordering::SeqCst));
        assert!(calls.load(Ordering::SeqCst) <= 1);
    }

    #[test]
    fn dropping_last_worker_owner_joins_thread() {
        let dropped = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let database = Arc::new(Database::open_in_memory().unwrap());
        let engine = Arc::new(FailingEngine {
            calls,
            dropped: Arc::clone(&dropped),
        });

        {
            let _worker = OcrWorker::start(engine, database).unwrap();
        }

        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn a_permanently_failing_image_is_retried_a_bounded_number_of_times() {
        let database = Arc::new(Database::open_in_memory().unwrap());
        database.save_item(&image_item("image")).unwrap();
        assert!(database.enqueue_ocr("image").unwrap());

        let calls = Arc::new(AtomicUsize::new(0));
        let engine = Arc::new(FailingEngine {
            calls: Arc::clone(&calls),
            dropped: Arc::new(AtomicBool::new(false)),
        });
        let worker = OcrWorker::start(engine, Arc::clone(&database)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if database
                .get_ocr_result("image")
                .unwrap()
                .is_some_and(|result| result.status == OcrStatus::Failed)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "OCR failure was never persisted after the retry budget ran out"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        worker.stop();
        let result = database.get_ocr_result("image").unwrap().unwrap();
        assert_eq!(result.status, OcrStatus::Failed);
        assert_eq!(
            result.error_message.as_deref(),
            Some("synthetic OCR failure")
        );
        assert_eq!(
            calls.load(Ordering::SeqCst) as u32,
            MAX_RECOGNITION_ATTEMPTS,
            "a permanently failing image must stop after the bounded attempt count"
        );
    }

    /// The whole point of the retry budget: a failure that would succeed on a
    /// second pass must not need the user to open the item and press regenerate.
    #[test]
    fn a_transient_failure_recovers_without_user_action() {
        let database = Arc::new(Database::open_in_memory().unwrap());
        database.save_item(&image_item("image")).unwrap();
        assert!(database.enqueue_ocr("image").unwrap());

        let calls = Arc::new(AtomicUsize::new(0));
        let engine = Arc::new(FlakyEngine {
            calls: Arc::clone(&calls),
            failures_before_success: 1,
        });
        let worker = OcrWorker::start(engine, Arc::clone(&database)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if database
                .get_ocr_result("image")
                .unwrap()
                .is_some_and(|result| result.status == OcrStatus::Completed)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "a transiently failing image was never retried to completion"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        worker.stop();
        let result = database.get_ocr_result("image").unwrap().unwrap();
        assert_eq!(result.full_text, "recognized after retry");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn retry_backoff_grows_and_stays_bounded() {
        assert_eq!(retry_backoff(0), Duration::from_secs(1));
        assert_eq!(retry_backoff(1), Duration::from_secs(2));
        assert_eq!(retry_backoff(2), Duration::from_secs(4));
        // The exponent cap keeps a large attempt count from overflowing the
        // shift and turning a backoff into hours.
        assert_eq!(retry_backoff(3), Duration::from_secs(8));
        assert_eq!(retry_backoff(64), Duration::from_secs(16));
    }

    #[test]
    fn successful_recognition_still_completes_after_lifecycle_changes() {
        let database = Arc::new(Database::open_in_memory().unwrap());
        database.save_item(&image_item("image")).unwrap();
        database.enqueue_ocr("image").unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let engine = Arc::new(SuccessfulEngine {
            calls: Arc::clone(&calls),
        });
        let worker = OcrWorker::start(engine, Arc::clone(&database)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        while database
            .get_ocr_result("image")
            .unwrap()
            .is_none_or(|result| result.status != OcrStatus::Completed)
        {
            assert!(Instant::now() < deadline, "OCR result was not completed");
            std::thread::sleep(Duration::from_millis(10));
        }

        worker.stop();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn regenerated_same_hash_tasks_reuse_the_fresh_result() {
        let database = Arc::new(Database::open_in_memory().unwrap());
        let mut first = image_item("first");
        first.content_hash = "shared-hash".to_owned();
        database.save_item(&first).unwrap();
        database.save_item(&image_item("second")).unwrap();

        for item_id in ["first", "second"] {
            database
                .save_ocr_result(&OcrResult::completed(
                    item_id,
                    "old-engine",
                    "old-model",
                    Some("en"),
                    "stale",
                    &[],
                    "shared-hash",
                ))
                .unwrap();
        }
        database.regenerate_ocr("first").unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let worker = OcrWorker::start(
            Arc::new(SuccessfulEngine {
                calls: Arc::clone(&calls),
            }),
            Arc::clone(&database),
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let first = database.get_ocr_result("first").unwrap().unwrap();
            let second = database.get_ocr_result("second").unwrap().unwrap();
            if first.status == OcrStatus::Completed && second.status == OcrStatus::Completed {
                assert_eq!(first.full_text, "recognized");
                assert_eq!(second.full_text, "recognized");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "regenerated OCR results did not complete"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        worker.stop();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn manager_restart_replaces_the_worker_for_all_clones() {
        let first_engine_dropped = Arc::new(AtomicBool::new(false));
        let first_database = Arc::new(Database::open_in_memory().unwrap());
        let manager = OcrWorkerManager::start(
            Arc::new(FailingEngine {
                calls: Arc::new(AtomicUsize::new(0)),
                dropped: Arc::clone(&first_engine_dropped),
            }),
            first_database,
        )
        .unwrap();
        let observer = manager.clone();

        let second_database = Arc::new(Database::open_in_memory().unwrap());
        second_database.save_item(&image_item("image")).unwrap();
        second_database.enqueue_ocr("image").unwrap();
        let second_calls = Arc::new(AtomicUsize::new(0));
        manager
            .restart(
                Arc::new(SuccessfulEngine {
                    calls: Arc::clone(&second_calls),
                }),
                Arc::clone(&second_database),
            )
            .unwrap();

        assert!(first_engine_dropped.load(Ordering::SeqCst));
        let deadline = Instant::now() + Duration::from_secs(2);
        while second_database
            .get_ocr_result("image")
            .unwrap()
            .is_none_or(|result| result.status != OcrStatus::Completed)
        {
            assert!(Instant::now() < deadline, "replacement worker did not run");
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(observer.is_running());
        observer.stop();
        assert!(!manager.is_running());
        assert_eq!(second_calls.load(Ordering::SeqCst), 1);
    }
}
