use std::process::Command;

use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};
use crate::domain::OcrTextBlock;

pub struct PpOcrEngine;

impl PpOcrEngine {
    pub fn new() -> Self { Self }

    pub fn is_available() -> bool {
        Command::new("python")
            .args(["-c", "from paddleocr import PaddleOCR; print('ok')"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "ok")
            .unwrap_or(false)
    }

    pub fn install() -> Result<(), String> {
        if !Command::new("python").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
            return Err("Python not found. Install Python 3.8+ from https://python.org".to_string());
        }
        eprintln!("[ppocr] installing paddleocr...");
        let out = Command::new("pip").args(["install", "paddlepaddle", "paddleocr"]).output()
            .map_err(|e| format!("pip failed: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        if !Self::is_available() {
            return Err("Install OK but import failed. Check Python env.".to_string());
        }
        Ok(())
    }
}

impl Default for PpOcrEngine { fn default() -> Self { Self } }

impl OcrEngine for PpOcrEngine {
    fn name(&self) -> &'static str { "ppocr" }
    fn model_version(&self) -> &str { "v6" }

    fn recognize(&self, input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
        let image_path = input.image_path.display().to_string().replace('\\', "\\\\");
        let script = format!(
            "from paddleocr import PaddleOCR; ocr = PaddleOCR(lang='ch', use_gpu=False); r = ocr.ocr('{}', cls=False); [print(l[1][0]) for l in r[0]] if r and r[0] else None",
            image_path
        );
        let out = Command::new("python").args(["-c", &script]).output()
            .map_err(|e| OcrEngineError::new(format!("Python: {e}")))?;
        if !out.status.success() {
            return Err(OcrEngineError::new(String::from_utf8_lossy(&out.stderr).trim().to_string()));
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if text.is_empty() { return Err(OcrEngineError::new("OCR returned empty text")); }
        let blocks: Vec<OcrTextBlock> = text.lines().filter(|l| !l.trim().is_empty()).enumerate()
            .map(|(i, l)| OcrTextBlock { text: l.trim().to_string(), confidence: 0.0, left: 0, top: i as u32 * 24, width: (l.len() as u32).saturating_mul(10).max(100), height: 24 }).collect();
        Ok(OcrOutput { language: Some("ch".to_string()), full_text: text, blocks })
    }
}
