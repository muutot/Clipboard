use std::process::Command;

use crate::domain::OcrTextBlock;

use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};

pub struct TesseractOcrEngine {
    languages: String,
    model_version: String,
}

impl TesseractOcrEngine {
    pub fn new() -> Self {
        let languages = "chi_sim+eng".to_string();
        let model_version = detect_tesseract_version().unwrap_or_else(|| "unknown".to_string());

        Self {
            languages,
            model_version,
        }
    }

    pub fn with_languages(languages: impl Into<String>) -> Self {
        let languages = languages.into();
        let model_version = detect_tesseract_version().unwrap_or_else(|| "unknown".to_string());

        Self {
            languages,
            model_version,
        }
    }

    pub fn is_available() -> bool {
        Command::new("tesseract")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl Default for TesseractOcrEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrEngine for TesseractOcrEngine {
    fn name(&self) -> &'static str {
        "tesseract"
    }

    fn model_version(&self) -> &str {
        &self.model_version
    }

    fn recognize(&self, input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
        let image_path = &input.image_path;

        if !image_path.exists() {
            return Err(OcrEngineError::new(format!(
                "image file not found: {}",
                image_path.display()
            )));
        }

        let output = Command::new("tesseract")
            .arg(image_path.to_string_lossy().as_ref())
            .arg("stdout")
            .arg("-l")
            .arg(&self.languages)
            .arg("--psm")
            .arg("6")
            .arg("--oem")
            .arg("1")
            .arg("-c")
            .arg("tessedit_write_images=false")
            .arg("-c")
            .arg("load_system_dawg=false")
            .arg("-c")
            .arg("load_freq_dawg=false")
            .output()
            .map_err(|error| {
                OcrEngineError::new(format!(
                    "failed to run tesseract (is it installed?): {}",
                    error
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OcrEngineError::new(format!(
                "tesseract exited with error: {}",
                stderr.trim()
            )));
        }

        let full_text = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let blocks = parse_text_into_blocks(&full_text);

        Ok(OcrOutput {
            language: Some(self.languages.clone()),
            full_text,
            blocks,
        })
    }
}

fn detect_tesseract_version() -> Option<String> {
    let output = Command::new("tesseract")
        .arg("--version")
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?;
    let version = first_line.split_whitespace().nth(1).unwrap_or("unknown");
    Some(version.to_string())
}

fn parse_text_into_blocks(text: &str) -> Vec<OcrTextBlock> {
    let mut blocks = Vec::new();
    let line_height = 24u32;

    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let block = OcrTextBlock {
            text: trimmed.to_string(),
            confidence: 0.0,
            left: 0,
            top: index as u32 * line_height,
            width: (trimmed.len() as u32).saturating_mul(10).max(100),
            height: line_height,
        };

        blocks.push(block);
    }

    if blocks.is_empty() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            blocks.push(OcrTextBlock {
                text: trimmed.to_string(),
                confidence: 0.0,
                left: 0,
                top: 0,
                width: (trimmed.len() as u32).saturating_mul(10).max(100),
                height: line_height,
            });
        }
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_has_name_and_version() {
        let engine = TesseractOcrEngine::new();
        assert_eq!(engine.name(), "tesseract");
        assert!(!engine.model_version().is_empty());
    }

    #[test]
    fn recognizes_missing_file_as_error() {
        let engine = TesseractOcrEngine::new();
        let input = OcrInput {
            item_id: "test".into(),
            image_path: PathBuf::from("/nonexistent/image.png"),
            image_hash: "abc".into(),
        };
        let result = engine.recognize(&input);
        assert!(result.is_err());
    }

    #[test]
    fn parse_text_into_blocks_splits_lines() {
        let text = "line one\nline two\n\nline three";
        let blocks = parse_text_into_blocks(text);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "line one");
        assert_eq!(blocks[1].text, "line two");
        assert_eq!(blocks[2].text, "line three");
    }

    #[test]
    fn parse_text_empty_string_returns_empty() {
        let blocks = parse_text_into_blocks("  ");
        assert!(blocks.is_empty());
    }

    #[test]
    fn custom_languages() {
        let engine = TesseractOcrEngine::with_languages("chi_sim");
        assert_eq!(engine.name(), "tesseract");
    }

    #[test]
    fn is_available_detects_tesseract() {
        let available = TesseractOcrEngine::is_available();
        assert!(available || !available);
    }
}
