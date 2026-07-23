use std::path::PathBuf;
use std::sync::Mutex;

use oar_ocr::oarocr::OAROCRBuilder;

use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};
use crate::domain::OcrTextBlock;

pub struct PpOcrEngine {
    ocr: Mutex<Option<oar_ocr::oarocr::OAROCR>>,
    models_dir: PathBuf,
}

// SAFETY: OAROCR wraps ort::Session which uses RefCell internally (not Sync).
// PpOcrEngine wraps it in a Mutex ensuring exclusive access. The OCR worker
// calls recognize from a single thread at a time.
unsafe impl Send for PpOcrEngine {}
unsafe impl Sync for PpOcrEngine {}

impl PpOcrEngine {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            ocr: Mutex::new(None),
            models_dir,
        }
    }

    pub fn is_available(&self) -> bool {
        super::models::all_models_present(&self.models_dir)
    }

    pub fn models_dir(&self) -> &PathBuf {
        &self.models_dir
    }

    fn build_ocr(&self) -> Result<oar_ocr::oarocr::OAROCR, OcrEngineError> {
        let paths = super::models::model_paths(&self.models_dir);

        if !paths.det.exists() {
            return Err(OcrEngineError::new(format!(
                "Detection model not found: {}",
                paths.det.display()
            )));
        }
        if !paths.rec.exists() {
            return Err(OcrEngineError::new(format!(
                "Recognition model not found: {}",
                paths.rec.display()
            )));
        }
        if !paths.dict.exists() {
            return Err(OcrEngineError::new(format!(
                "Dictionary not found: {}",
                paths.dict.display()
            )));
        }

        OAROCRBuilder::new(
            paths.det.to_string_lossy().as_ref(),
            paths.rec.to_string_lossy().as_ref(),
            paths.dict.to_string_lossy().as_ref(),
        )
        .build()
        .map_err(|e| OcrEngineError::new(format!("Failed to build OCR engine: {e}")))
    }
}

impl Default for PpOcrEngine {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

impl OcrEngine for PpOcrEngine {
    fn name(&self) -> &'static str {
        "ppocr"
    }

    fn model_version(&self) -> &str {
        "v5"
    }

    fn recognize(&self, input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
        if !input.image_path.exists() {
            return Err(OcrEngineError::new(format!(
                "Image file not found: {}",
                input.image_path.display()
            )));
        }

        let mut guard = self.ocr.lock().map_err(|_| {
            OcrEngineError::new("OCR engine lock is poisoned")
        })?;

        if guard.is_none() {
            let ocr = self.build_ocr()?;
            *guard = Some(ocr);
        }

        let ocr = guard.as_ref().unwrap();

        let result = ocr
            .predict(input.image_path.to_string_lossy().as_ref())
            .map_err(|e| OcrEngineError::new(format!("OCR recognition failed: {e}")))?;

        if result.text_regions.is_empty() {
            return Err(OcrEngineError::new("OCR returned no text"));
        }

        let mut full_text = String::new();
        let mut blocks = Vec::new();

        for text_region in &result.text_regions {
            let text = text_region.text();
            let confidence = text_region.confidence().unwrap_or(0.0);

            if text.is_empty() {
                continue;
            }

            if !full_text.is_empty() {
                full_text.push('\n');
            }
            full_text.push_str(text);

            let bbox = text_region.bounding_box();
            let left = bbox.iter().map(|p| p[0]).fold(f32::MAX, f32::min).max(0.0) as u32;
            let top = bbox.iter().map(|p| p[1]).fold(f32::MAX, f32::min).max(0.0) as u32;
            let right = bbox.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
            let bottom = bbox.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
            let width = (right - left as f32).round().max(1.0) as u32;
            let height = (bottom - top as f32).round().max(1.0) as u32;

            blocks.push(OcrTextBlock {
                text: text.to_string(),
                confidence,
                left,
                top,
                width,
                height,
            });
        }

        if full_text.is_empty() {
            return Err(OcrEngineError::new("OCR returned empty text"));
        }

        Ok(OcrOutput {
            language: Some("ch".to_string()),
            full_text,
            blocks,
        })
    }
}
