use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::domain::OcrTextBlock;

use super::{OcrEngine, OcrEngineError, OcrInput, OcrOutput};

pub struct WindowsOcrEngine;

impl WindowsOcrEngine {
    pub fn new() -> Self { Self }
    
    pub fn is_available() -> bool {
        #[cfg(target_os = "windows")]
        {
            Command::new("powershell")
                .args(["-NoProfile", "-Command", "try { Add-Type -AssemblyName System.Runtime.WindowsRuntime; $null = [Windows.Media.Ocr.OcrEngine]; $true } catch { $false }"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "True")
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "windows"))]
        { false }
    }
}

impl Default for WindowsOcrEngine {
    fn default() -> Self { Self }
}

impl OcrEngine for WindowsOcrEngine {
    fn name(&self) -> &'static str { "windows-ocr" }
    fn model_version(&self) -> &str { "win10+" }

    fn recognize(&self, input: &OcrInput) -> Result<OcrOutput, OcrEngineError> {
        let image_path = input.image_path.to_string_lossy().replace('\\', "\\\\");
        let temp_ps1 = std::env::temp_dir().join(format!("ocr_{}.ps1", std::process::id()));
        
        let script = format!(r#"
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$bmp = New-Object System.Drawing.Bitmap '{image_path}'
$ms = New-Object System.IO.MemoryStream
$bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose(); $ms.Position = 0
$rs = [System.IO.WindowsRuntimeStreamExtensions]::AsRandomAccessStream($ms)
$dec = [Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($rs)
$task = $dec
while (-not $task.Status -eq 'Completed') {{ Start-Sleep -Milliseconds 5 }}
$dec = $task.GetResults()
$sbTask = $dec.GetSoftwareBitmapAsync()
while (-not $sbTask.Status -eq 'Completed') {{ Start-Sleep -Milliseconds 5 }}
$sb = $sbTask.GetResults()
$eng = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage((New-Object Windows.Globalization.Language 'zh-Hans'))
if (-not $eng) {{ $eng = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage((New-Object Windows.Globalization.Language 'en-US')) }}
if (-not $eng) {{ throw 'OCR engine not available' }}
$ocrTask = $eng.RecognizeAsync($sb)
while (-not $ocrTask.Status -eq 'Completed') {{ Start-Sleep -Milliseconds 5 }}
$res = $ocrTask.GetResults()
$res.Lines | % {{ $_.Text }}"#);

        fs::write(&temp_ps1, &script)
            .map_err(|e| OcrEngineError::new(format!("write script: {e}")))?;

        let output = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&temp_ps1)
            .output()
            .map_err(|e| OcrEngineError::new(format!("PowerShell: {e}")))?;

        let _ = fs::remove_file(&temp_ps1);

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if err.is_empty() { return Err(OcrEngineError::new("OCR failed")); }
            return Err(OcrEngineError::new(err));
        }

        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Err(OcrEngineError::new("OCR returned empty text"));
        }

        let blocks: Vec<OcrTextBlock> = text.lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, line)| OcrTextBlock {
                text: line.trim().to_string(),
                confidence: 0.0, left: 0,
                top: i as u32 * 24,
                width: (line.len() as u32).saturating_mul(10).max(100),
                height: 24,
            })
            .collect();

        Ok(OcrOutput {
            language: Some("zh-Hans".to_string()),
            full_text: text,
            blocks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_has_name() { assert_eq!(WindowsOcrEngine.name(), "windows-ocr"); }

    #[test]
    fn missing_file_errors() {
        let input = OcrInput {
            item_id: "t".into(), image_path: PathBuf::from("/nonexistent.png"), image_hash: "a".into(),
        };
        assert!(WindowsOcrEngine.recognize(&input).is_err());
    }
}
