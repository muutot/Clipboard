use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};

/// Per-image cap so a hung `tesseract` process cannot stall the OCR worker
/// queue forever.
const RECOGNIZE_TIMEOUT: Duration = Duration::from_secs(120);
/// Cap for `--version` probes. They run while callers may hold the global
/// config lock (`apply_ocr_runtime_settings`), so a hung binary must not
/// block every config-dependent command forever.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Fallback languages when the stored value is blank or contains characters
/// outside the traineddata-stem alphabet. Passing an invalid `-l` value
/// would fail every recognition, bricking all OCR until the config is fixed.
const DEFAULT_LANGUAGES: &str = "chi_sim+eng";

/// Returns the stored languages when usable as a `tesseract -l` argument
/// (one or more `+`-joined traineddata stems), otherwise the default.
pub fn sanitize_languages(languages: &str) -> &str {
    let trimmed = languages.trim();
    if trimmed.is_empty() {
        return DEFAULT_LANGUAGES;
    }
    let valid = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '.'));
    if valid {
        trimmed
    } else {
        DEFAULT_LANGUAGES
    }
}

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
        let languages = sanitize_languages(&languages.into()).to_owned();
        let model_version = detect_tesseract_version().unwrap_or_else(|| "unknown".to_string());

        Self {
            languages,
            model_version,
        }
    }

    pub fn is_available() -> bool {
        run_probe_with_timeout("tesseract", &["--version"], PROBE_TIMEOUT)
            .is_some_and(|output| output.status.success())
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

        let output = run_tesseract_with_timeout(
            image_path.to_string_lossy().as_ref(),
            &self.languages,
            RECOGNIZE_TIMEOUT,
        )?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OcrEngineError::new(format!(
                "tesseract exited with error: {}",
                stderr.trim()
            )));
        }

        let full_text = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Plain-text CLI output carries no geometry or confidence data, so no
        // per-line blocks are produced: fabricating plausible-looking boxes
        // (and a 0.0 confidence) would mislead any future block-level
        // consumer. Search and display rely on `full_text`.
        let blocks = Vec::new();

        Ok(OcrOutput {
            language: Some(self.languages.clone()),
            full_text,
            blocks,
        })
    }
}

/// Runs `tesseract` with the engine's fixed flags, killing the child if it
/// exceeds `timeout`. OCR text output is small, so polling `try_wait` while
/// the pipes buffer is safe; a hung process is killed and reported instead
/// of stalling the worker queue.
fn run_tesseract_with_timeout(
    image_path: &str,
    languages: &str,
    timeout: Duration,
) -> Result<std::process::Output, OcrEngineError> {
    let mut child = Command::new("tesseract")
        .arg(image_path)
        .arg("stdout")
        .arg("-l")
        .arg(languages)
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            OcrEngineError::new(format!(
                "failed to run tesseract (is it installed?): {error}"
            ))
        })?;

    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|error| {
            OcrEngineError::new(format!("failed to wait for tesseract: {error}"))
        })? {
            Some(_) => {
                return child.wait_with_output().map_err(|error| {
                    OcrEngineError::new(format!("failed to read tesseract output: {error}"))
                });
            }
            None => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(OcrEngineError::new(format!(
                        "tesseract timed out after {}s",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Runs a short probe command, killing the child if it exceeds `timeout`.
/// A hung `tesseract` binary must not block the global config lock or the
/// status path forever.
fn run_probe_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Option<std::process::Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(_) => return child.wait_with_output().ok(),
            None => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn detect_tesseract_version() -> Option<String> {
    let output = run_probe_with_timeout("tesseract", &["--version"], PROBE_TIMEOUT)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?;
    let version = first_line.split_whitespace().nth(1).unwrap_or("unknown");
    Some(version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    fn custom_languages() {
        let engine = TesseractOcrEngine::with_languages("chi_sim");
        assert_eq!(engine.name(), "tesseract");
    }

    #[test]
    fn sanitize_languages_keeps_valid_stems() {
        assert_eq!(sanitize_languages("chi_sim+eng"), "chi_sim+eng");
        assert_eq!(sanitize_languages("eng"), "eng");
    }

    #[test]
    fn sanitize_languages_falls_back_on_blank_or_invalid() {
        assert_eq!(sanitize_languages(""), DEFAULT_LANGUAGES);
        assert_eq!(sanitize_languages("   "), DEFAULT_LANGUAGES);
        assert_eq!(sanitize_languages("eng;rm -rf"), DEFAULT_LANGUAGES);
        assert_eq!(sanitize_languages("chi_sim|eng"), DEFAULT_LANGUAGES);
        // Shell metacharacters never reach the child process (Command::arg
        // bypasses the shell), but they still indicate a typo that would
        // fail every recognition, so they fall back too.
        assert_eq!(sanitize_languages("eng$HOME"), DEFAULT_LANGUAGES);
    }

    #[test]
    fn is_available_does_not_panic_when_tesseract_is_optional() {
        let _ = TesseractOcrEngine::is_available();
    }
}
