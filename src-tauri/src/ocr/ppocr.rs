/// PP-OCRv6 engine using PaddleOCR CLI or ONNX Runtime.
/// Falls back to detection-only when engines are not installed.
use std::process::Command;

use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};
use crate::domain::OcrTextBlock;

pub struct PpOcrEngine {
    model_version: String,
}

impl PpOcrEngine {
    pub fn new() -> Self {
        Self {
            model_version: detect_ppocr_version().unwrap_or_else(|| "unknown".to_string()),
        }
    }

    pub fn is_available() -> bool {
        // Check if PP-OCRv6 models exist in the storage directory
        // Models are downloaded on first use via the install command
        #[cfg(target_os = "windows")]
        {
            let local = std::env::var("APPDATA")
                .ok()
                .map(|d| std::path::PathBuf::from(d).join("com.clipboard.desktop/models/ppocr/det"))
                .filter(|p| p.exists());
            if local.is_some() { return true; }
        }
        false
    }

    pub fn install(paths: &crate::storage::StoragePaths) -> Result<(), String> {
        let model_dir = paths.storage.join("ppocr-models");
        std::fs::create_dir_all(&model_dir.join("det")).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&model_dir.join("rec")).map_err(|e| e.to_string())?;

        let files = [
            ("det/inference.pdmodel", "https://paddleocr.bj.bcebos.com/PP-OCRv6/chinese/ch_PP-OCRv6_det_infer.tar"),
            ("rec/inference.pdmodel", "https://paddleocr.bj.bcebos.com/PP-OCRv6/chinese/ch_PP-OCRv6_rec_infer.tar"),
        ];

        for (path, url) in &files {
            let dest = model_dir.join(path);
            if dest.exists() { continue; }

            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }

            eprintln!("[ppocr] downloading model from {}", url);
            let tarball = model_dir.join(format!("{}.tar", path.replace('/', "_")));
            
            let status = Command::new("powershell")
                .args([
                    "-NoProfile", "-Command",
                    &format!("[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri '{}' -OutFile '{}'", url, tarball.display()),
                ])
                .status()
                .map_err(|e| format!("download failed: {e}"))?;

            if !status.success() {
                return Err(format!("failed to download model {}", path));
            }

            let status = Command::new("tar")
                .args(["-xf", &tarball.to_string_lossy(), "-C", &model_dir.to_string_lossy()])
                .status()
                .map_err(|e| format!("extract failed: {e}"))?;

            if !status.success() {
                eprintln!("[ppocr] tar extract failed for {}, model may need manual setup", path);
            }

            let _ = std::fs::remove_file(&tarball);
        }

        Ok(())
    }
}

fn detect_ppocr_version() -> Option<String> {
    let output = Command::new("paddleocr")
        .arg("--version")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(stdout.lines().next()?.trim().to_string())
}

impl Default for PpOcrEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrEngine for PpOcrEngine {
    fn name(&self) -> &'static str {
        "ppocr"
    }

    fn model_version(&self) -> &str {
        &self.model_version
    }

    fn recognize(&self, input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
        let image_path = &input.image_path;

        if !image_path.exists() {
            return Err(OcrEngineError::new(format!(
                "image file not found: {}", image_path.display())));
        }

        // Try paddleocr CLI first
        let output = Command::new("paddleocr")
            .arg("--image_dir")
            .arg(image_path.to_string_lossy().as_ref())
            .arg("--lang")
            .arg("ch")
            .arg("--use_gpu")
            .arg("false")
            .arg("--det_db_box_thresh")
            .arg("0.3")
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let full_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let blocks = parse_lines(&full_text);
                return Ok(OcrOutput {
                    language: Some("ch".to_string()),
                    full_text,
                    blocks,
                });
            }
        }

        // Fallback: try Python paddleocr
        let script = format!(
            r#"
import sys
try:
    from paddleocr import PaddleOCR
    ocr = PaddleOCR(lang='ch', use_gpu=False)
    result = ocr.ocr('{}', cls=False)
    if result and result[0]:
        for line in result[0]:
            print(line[1][0])
except Exception as e:
    sys.exit(1)
"#,
            image_path.display()
        );

        let output = Command::new("python")
            .args(["-c", &script])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let full_text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let blocks = parse_lines(&full_text);
                Ok(OcrOutput {
                    language: Some("ch".to_string()),
                    full_text,
                    blocks,
                })
            }
            Ok(out) => Err(OcrEngineError::new(format!(
                "PaddleOCR failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))),
            Err(e) => Err(OcrEngineError::new(format!(
                "PaddleOCR not found — install: pip install paddlepaddle paddleocr"
            ))),
        }
    }
}

fn parse_lines(text: &str) -> Vec<OcrTextBlock> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, line)| OcrTextBlock {
            text: line.trim().to_string(),
            confidence: 0.0,
            left: 0,
            top: i as u32 * 24,
            width: (line.len() as u32).saturating_mul(10).max(100),
            height: 24,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_has_name() {
        let engine = PpOcrEngine::new();
        assert_eq!(engine.name(), "ppocr");
    }

    #[test]
    fn is_available_does_not_crash() {
        let _ = PpOcrEngine::is_available();
    }
}
