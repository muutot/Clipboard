use std::path::PathBuf;
use std::sync::Mutex;

use oar_ocr::domain::tasks::TextDetectionConfig;
use oar_ocr::oarocr::OAROCRBuilder;

use super::models::PpOcrModelSpec;
use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};
use crate::domain::OcrTextBlock;

pub struct PpOcrEngine {
    ocr: Mutex<Option<oar_ocr::oarocr::OAROCR>>,
    models_dir: PathBuf,
    model: &'static PpOcrModelSpec,
    score_threshold: f32,
    box_threshold: f32,
    unclip_ratio: f32,
}

// SAFETY: OAROCR wraps ort::Session internally. The OCR worker calls recognize
// from a single thread at a time, and we protect access with a Mutex.
unsafe impl Send for PpOcrEngine {}
unsafe impl Sync for PpOcrEngine {}

impl PpOcrEngine {
    pub fn new(
        models_dir: PathBuf,
        model: &'static PpOcrModelSpec,
        score_threshold: f32,
        box_threshold: f32,
        unclip_ratio: f32,
    ) -> Self {
        Self {
            ocr: Mutex::new(None),
            models_dir,
            model,
            score_threshold,
            box_threshold,
            unclip_ratio,
        }
    }

    pub fn is_available(&self) -> bool {
        super::models::model_is_installed(&self.models_dir, self.model)
    }

    pub fn models_dir(&self) -> &PathBuf {
        &self.models_dir
    }

    fn build_ocr(&self) -> Result<oar_ocr::oarocr::OAROCR, OcrEngineError> {
        super::models::set_oar_home(&self.models_dir);

        OAROCRBuilder::new(
            self.model.detection.filename,
            self.model.recognition.filename,
            self.model.dictionary.filename,
        )
        .text_detection_config(TextDetectionConfig {
            score_threshold: self.score_threshold,
            box_threshold: self.box_threshold,
            unclip_ratio: self.unclip_ratio,
            ..Default::default()
        })
        .build()
        .map_err(|e| OcrEngineError::new(format!("Failed to build OCR engine: {e}")))
    }
}

impl Default for PpOcrEngine {
    fn default() -> Self {
        Self::new(
            PathBuf::new(),
            super::models::default_model_spec(),
            0.3,
            0.6,
            1.5,
        )
    }
}

impl OcrEngine for PpOcrEngine {
    fn name(&self) -> &'static str {
        "ppocr"
    }

    fn model_version(&self) -> &str {
        self.model.model_version
    }

    fn recognize(&self, input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
        if !input.image_path.exists() {
            return Err(OcrEngineError::new(format!(
                "Image file not found: {}",
                input.image_path.display()
            )));
        }

        let mut guard = self
            .ocr
            .lock()
            .map_err(|_| OcrEngineError::new("OCR engine lock is poisoned"))?;

        if guard.is_none() {
            let ocr = self.build_ocr()?;
            *guard = Some(ocr);
        }

        let ocr = guard.as_ref().unwrap();

        let image = oar_ocr::utils::load_image(&input.image_path)
            .map_err(|e| OcrEngineError::new(format!("Failed to load image: {e}")))?;

        let result = ocr
            .predict(vec![image])
            .map_err(|e| OcrEngineError::new(format!("OCR recognition failed: {e}")))?;

        let page = result
            .into_iter()
            .next()
            .ok_or_else(|| OcrEngineError::new("OCR returned no results"))?;

        if page.text_regions.is_empty() {
            return Err(OcrEngineError::new("OCR returned no text"));
        }

        let mut full_text = String::new();
        let mut blocks = Vec::new();

        for region in &page.text_regions {
            let text = match &region.text {
                Some(t) if !t.is_empty() => t,
                _ => continue,
            };

            let confidence = region.confidence.unwrap_or(0.0);

            if !full_text.is_empty() {
                full_text.push('\n');
            }
            full_text.push_str(text);

            let bbox = &region.bounding_box;
            let left = bbox
                .points
                .iter()
                .map(|p| p.x)
                .fold(f32::MAX, f32::min)
                .max(0.0) as u32;
            let top = bbox
                .points
                .iter()
                .map(|p| p.y)
                .fold(f32::MAX, f32::min)
                .max(0.0) as u32;
            let right = bbox.points.iter().map(|p| p.x).fold(f32::MIN, f32::max);
            let bottom = bbox.points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
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
