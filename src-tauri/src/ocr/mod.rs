mod engine;
pub mod models;
mod noop;
mod ppocr;
mod tesseract;
mod worker;

pub use engine::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};
pub use noop::NoopOcrEngine;
pub use ppocr::PpOcrEngine;
pub use tesseract::TesseractOcrEngine;
pub use worker::OcrWorker;
