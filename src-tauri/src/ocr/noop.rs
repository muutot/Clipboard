use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};

/// A no-op OCR engine that skips all recognition tasks.
/// Used when no real OCR engine is available on the system.
pub struct NoopOcrEngine;

impl OcrEngine for NoopOcrEngine {
    fn name(&self) -> &'static str {
        "none"
    }

    fn model_version(&self) -> &str {
        "0"
    }

    fn recognize(&self, _input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
        Err(OcrEngineError::new(
            "no OCR engine configured — install tesseract or configure PP-OCR"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_engine_returns_error() {
        let engine = NoopOcrEngine;
        let input = OcrInput {
            item_id: "test".into(),
            image_path: std::path::PathBuf::from("/nonexistent.png"),
            image_hash: "abc".into(),
        };
        assert!(engine.recognize(&input).is_err());
        assert_eq!(engine.name(), "none");
    }
}
