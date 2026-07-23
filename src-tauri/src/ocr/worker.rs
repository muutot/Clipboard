use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::domain::OcrResult;
use crate::storage::{Database, OcrRepository};

use super::OcrEngine;

pub struct OcrWorker {
    running: Arc<AtomicBool>,
}

impl OcrWorker {
    pub fn start(
        engine: Arc<dyn OcrEngine>,
        database: Arc<Database>,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        thread::spawn(move || {
            Self::run_loop(engine, database, running_clone);
        });

        Self { running }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn run_loop(
        engine: Arc<dyn OcrEngine>,
        database: Arc<Database>,
        running: Arc<AtomicBool>,
    ) {
        let poll_interval = Duration::from_secs(2);
        let mut consecutive_errors = 0u32;
        let max_consecutive_errors = 5;

        while running.load(Ordering::SeqCst) {
            let has_task = match database.claim_next_ocr() {
                Ok(Some(input)) => {
                    consecutive_errors = 0;

                    let engine_name = engine.name().to_string();
                    let model_version = engine.model_version().to_string();

                    match engine.recognize(&input) {
                        Ok(output) => {
                            let result = OcrResult::completed(
                                &input.item_id,
                                &engine_name,
                                &model_version,
                                output.language.as_deref(),
                                &output.full_text,
                                &output.blocks,
                                &input.image_hash,
                            );

                            if let Err(error) = database.save_ocr_result(&result) {
                                eprintln!(
                                    "[ocr] failed to save result for {}: {}",
                                    input.item_id, error
                                );
                            }
                        }
                        Err(error) => {
                            eprintln!(
                                "[ocr] recognition failed for {}: {}",
                                input.item_id, error
                            );
                            let _ = database.retry_ocr(&input.item_id);
                        }
                    }

                    true
                }
                Ok(None) => {
                    false
                }
                Err(error) => {
                    eprintln!("[ocr] failed to claim next task: {}", error);
                    consecutive_errors += 1;
                    if consecutive_errors >= max_consecutive_errors {
                        eprintln!(
                            "[ocr] too many consecutive errors ({}), pausing",
                            consecutive_errors
                        );
                        thread::sleep(Duration::from_secs(10));
                        consecutive_errors = 0;
                    }
                    false
                }
            };

            if !has_task {
                thread::sleep(poll_interval);
            }
        }
    }
}

impl Drop for OcrWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
