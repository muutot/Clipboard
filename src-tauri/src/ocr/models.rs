use std::fs;
use std::path::{Path, PathBuf};

const PPOCRV5_DET_URL: &str =
    "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.0/onnx/PP-OCRv5/det/ch_PP-OCRv5_det_mobile.onnx";
const PPOCRV5_REC_URL: &str =
    "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.0/onnx/PP-OCRv5/rec/ch_PP-OCRv5_rec_mobile.onnx";
const PPOCRV5_DICT_URL: &str =
    "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.0/paddle/PP-OCRv5/rec/ch_PP-OCRv5_rec_server/ppocrv5_dict.txt";

const DET_FILENAME: &str = "det.onnx";
const REC_FILENAME: &str = "rec.onnx";
const DICT_FILENAME: &str = "ppocrv5_dict.txt";

#[derive(Debug, Clone)]
pub struct ModelPaths {
    pub det: PathBuf,
    pub rec: PathBuf,
    pub dict: PathBuf,
}

pub enum DownloadProgress {
    Starting { filename: String },
    Complete { filename: String },
    Error { filename: String, message: String },
}

pub fn models_dir(storage_path: &Path) -> PathBuf {
    storage_path.join("models").join("ppocr")
}

pub fn all_models_present(dir: &Path) -> bool {
    dir.join(DET_FILENAME).exists()
        && dir.join(REC_FILENAME).exists()
        && dir.join(DICT_FILENAME).exists()
}

pub fn model_paths(dir: &Path) -> ModelPaths {
    ModelPaths {
        det: dir.join(DET_FILENAME),
        rec: dir.join(REC_FILENAME),
        dict: dir.join(DICT_FILENAME),
    }
}

pub fn download_models(
    dir: &Path,
    progress_cb: Option<Box<dyn Fn(DownloadProgress) + Send>>,
) -> Result<ModelPaths, String> {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create models dir: {e}"))?;

    let files: Vec<(&str, &str)> = vec![
        (DET_FILENAME, PPOCRV5_DET_URL),
        (REC_FILENAME, PPOCRV5_REC_URL),
        (DICT_FILENAME, PPOCRV5_DICT_URL),
    ];

    for (filename, url) in &files {
        let target = dir.join(filename);
        if target.exists() {
            if let Some(ref cb) = progress_cb {
                cb(DownloadProgress::Complete {
                    filename: filename.to_string(),
                });
            }
            continue;
        }

        if let Some(ref cb) = progress_cb {
            cb(DownloadProgress::Starting {
                filename: filename.to_string(),
            });
        }

        download_file(url, &target).map_err(|e| {
            let msg = format!("Failed to download {filename}: {e}");
            // Clean up partial file so it's re-downloaded next time
            let _ = fs::remove_file(&target);
            if let Some(ref cb) = progress_cb {
                cb(DownloadProgress::Error {
                    filename: filename.to_string(),
                    message: msg.clone(),
                });
            }
            msg
        })?;

        if let Some(ref cb) = progress_cb {
            cb(DownloadProgress::Complete {
                filename: filename.to_string(),
            });
        }
    }

    Ok(model_paths(dir))
}

fn download_file(url: &str, target: &Path) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::new_with_defaults();
    let mut response = agent.get(url).call().map_err(|e| format!("HTTP error: {e}"))?;

    let bytes = response
        .body_mut()
        .with_config()
        .limit(200 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| format!("Read body error: {e}"))?;

    fs::write(target, &bytes).map_err(|e| format!("File write error: {e}"))?;

    Ok(())
}
