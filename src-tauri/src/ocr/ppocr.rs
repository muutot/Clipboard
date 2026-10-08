use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use image::GenericImageView;
use oar_ocr::domain::tasks::TextDetectionConfig;
use oar_ocr::oarocr::OAROCRBuilder;

use super::models::PpOcrModelSpec;
use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};
use crate::domain::OcrTextBlock;

/// Longest side of the image handed to the detector.
///
/// oar-ocr clamps the long side to its own `DEFAULT_MAX_SIDE_LIMIT` (4000)
/// and then resizes to `DEFAULT_LIMIT_SIDE_LEN` (960), so source pixels
/// beyond 4000 are decoded, allocated and then thrown away. Clamping before
/// `predict` avoids paying for the decode: a 16384x12288 screenshot is
/// ~600 MB once decoded to RGB, versus ~34 MB at 4000px. The value is the
/// library's own limit rather than a new tunable, so nothing the detector
/// would have used is lost.
const MAX_OCR_SIDE: u32 = 4000;

/// Target size for the OCR input plus the factor that maps the clamped image
/// back to the original's pixel space.
///
/// The stored block geometry comes straight from the detector in the
/// coordinate space of whatever image it was given, so the caller has to
/// rescale it — otherwise clamping would silently redefine every stored
/// box against a smaller canvas.
fn ocr_input_geometry(width: u32, height: u32) -> (u32, u32, f64) {
    let longest = width.max(height);
    if longest <= MAX_OCR_SIDE || longest == 0 {
        return (width, height, 1.0);
    }

    let ratio = MAX_OCR_SIDE as f64 / longest as f64;
    let target_width = ((width as f64 * ratio).round() as u32).max(1);
    let target_height = ((height as f64 * ratio).round() as u32).max(1);
    (
        target_width,
        target_height,
        target_width as f64 / width as f64,
    )
}

pub struct PpOcrEngine {
    ocr: Mutex<Option<oar_ocr::oarocr::OAROCR>>,
    models_dir: PathBuf,
    model: &'static PpOcrModelSpec,
    score_threshold: f32,
    box_threshold: f32,
    unclip_ratio: f32,
}

// NOTE: no manual unsafe impl Send/Sync here. `Mutex<Option<OAROCR>>` is
// automatically Sync (and the engine Send) whenever `oar_ocr::OAROCR: Send`,
// which ort guarantees for its sessions; letting the compiler derive this
// keeps an upgrade from silently introducing a cross-thread data race.

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
        // `OAR_HOME` is process-global: serialize set-var + build so two
        // engines (e.g. restart overlap) cannot interleave different dirs.
        static OAR_BUILD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let build_lock = OAR_BUILD_LOCK.get_or_init(|| Mutex::new(()));
        let _held = build_lock
            .lock()
            .map_err(|_| OcrEngineError::new("OCR build lock is poisoned"))?;
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

        let ocr = guard
            .as_ref()
            .ok_or_else(|| OcrEngineError::new("OCR engine unavailable after init"))?;

        // Decode through the project's bounded decoder rather than
        // oar_ocr's loader: both guess the format from the same way, but only
        // this one caps dimensions and the allocation budget, so a crafted or
        // merely enormous clipboard image cannot allocate its full size here.
        let decoded =
            crate::content::hash::decode_image_file(&input.image_path).ok_or_else(|| {
                OcrEngineError::new(format!(
                    "Failed to load image: {}",
                    input.image_path.display()
                ))
            })?;

        let (original_width, original_height) = decoded.dimensions();
        let (target_width, target_height, coordinate_scale) =
            ocr_input_geometry(original_width, original_height);

        let image = if (target_width, target_height) == (original_width, original_height) {
            decoded.to_rgb8()
        } else {
            decoded
                .resize_exact(
                    target_width,
                    target_height,
                    image::imageops::FilterType::Lanczos3,
                )
                .to_rgb8()
        };

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
            // The detector reports boxes in the coordinate space of the image
            // it was given, which may have been clamped, so map every edge
            // back into the original image's pixels.
            let to_original = |value: f32| (value as f64 * coordinate_scale) as f32;
            let left = bbox
                .points
                .iter()
                .map(|p| to_original(p.x))
                .fold(f32::MAX, f32::min)
                .max(0.0) as u32;
            let top = bbox
                .points
                .iter()
                .map(|p| to_original(p.y))
                .fold(f32::MAX, f32::min)
                .max(0.0) as u32;
            let right = bbox
                .points
                .iter()
                .map(|p| to_original(p.x))
                .fold(f32::MIN, f32::max);
            let bottom = bbox
                .points
                .iter()
                .map(|p| to_original(p.y))
                .fold(f32::MIN, f32::max);
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

#[cfg(test)]
mod tests {
    use super::{ocr_input_geometry, PpOcrEngine, MAX_OCR_SIDE};

    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    /// Compile-time lock on the concurrency contract: the engine must keep
    /// deriving Send+Sync from its `Mutex<Option<OAROCR>>` field rather than
    /// relying on manual unsafe impls.
    #[test]
    fn engine_derives_send_and_sync_without_unsafe_impls() {
        assert_send::<PpOcrEngine>();
        assert_sync::<PpOcrEngine>();
    }

    /// An image the detector can use must pass through untouched, otherwise
    /// the clamp would cost accuracy on ordinary screenshots for nothing.
    #[test]
    fn images_within_the_limit_are_not_rescaled() {
        assert_eq!(ocr_input_geometry(1920, 1080), (1920, 1080, 1.0));
        assert_eq!(
            ocr_input_geometry(MAX_OCR_SIDE, 100),
            (MAX_OCR_SIDE, 100, 1.0)
        );
    }

    /// The clamp must apply to whichever side is longer, and the returned
    /// factor must be the exact multiplier that maps block geometry back.
    #[test]
    fn oversized_images_clamp_their_longest_side() {
        let (width, height, scale) = ocr_input_geometry(16384, 12288);
        assert_eq!(width, MAX_OCR_SIDE);
        assert_eq!(height, 3000);
        // The factor shrinks detector coordinates back up into the original
        // image's pixel space, so it is below 1 and inverts the clamp.
        assert!(scale < 1.0);
        assert!((width as f64 / scale - 16384.0).abs() < 1.0);
        assert!((height as f64 / scale - 12288.0).abs() < 1.0);

        let (width, height, scale) = ocr_input_geometry(100, 20000);
        assert_eq!(height, MAX_OCR_SIDE);
        assert_eq!(width, 20);
        assert!((width as f64 / scale - 100.0).abs() < 1.0);
    }

    /// Degenerate inputs must not divide by zero or collapse to a 0px image,
    /// which the detector would reject with a much less useful message.
    #[test]
    fn degenerate_geometry_stays_usable() {
        assert_eq!(ocr_input_geometry(0, 0), (0, 0, 1.0));
        let (width, height, _) = ocr_input_geometry(1, 10_000);
        assert!(width >= 1);
        assert_eq!(height, MAX_OCR_SIDE);
    }
}
