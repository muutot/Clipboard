use std::{error::Error, fmt, path::PathBuf};

use crate::domain::OcrTextBlock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrInput {
    pub item_id: String,
    pub image_path: PathBuf,
    pub image_hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrOutput {
    pub language: Option<String>,
    pub full_text: String,
    pub blocks: Vec<OcrTextBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrEngineError {
    message: String,
}

impl OcrEngineError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OcrEngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OcrEngineError {}

/// Cross-platform OCR boundary. Implementations may use ONNX Runtime,
/// platform APIs or a test double without changing the application pipeline.
pub trait OcrEngine: Send + Sync {
    fn name(&self) -> &'static str;
    fn model_version(&self) -> &str;
    fn recognize(&self, input: &OcrInput) -> Result<OcrOutput, OcrEngineError>;
}
