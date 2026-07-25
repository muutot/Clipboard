use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
};

use image::codecs::jpeg::JpegEncoder;
use image::GenericImageView;
use serde::Serialize;

use crate::storage::{Database, StorageError};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailInfo {
    pub preview_path: String,
    pub width: u32,
    pub height: u32,
    pub original_size: u64,
    pub preview_size: u64,
}

pub struct ThumbnailGenerator {
    max_width: u32,
    quality: u8,
}

impl ThumbnailGenerator {
    pub fn new() -> Self {
        Self {
            max_width: 400,
            quality: 85,
        }
    }

    pub fn generate(
        &self,
        image_path: &Path,
        preview_dir: &Path,
    ) -> Result<ThumbnailInfo, StorageError> {
        let original_size = fs::metadata(image_path)?.len();
        let img = image::open(image_path).map_err(|e| {
            StorageError::Io(std::io::Error::other(format!("image decode error: {e}")))
        })?;
        let (orig_w, orig_h) = img.dimensions();
        let (w, h) = if orig_w > self.max_width {
            let ratio = self.max_width as f64 / orig_w as f64;
            (self.max_width, (orig_h as f64 * ratio) as u32)
        } else {
            (orig_w, orig_h)
        };
        let resized = img.resize_exact(w.max(1), h.max(1), image::imageops::FilterType::Lanczos3);

        fs::create_dir_all(preview_dir)?;
        let preview_filename = format!("{}.jpg", hash_path(image_path));
        let preview_path = preview_dir.join(&preview_filename);

        let rgb = resized.into_rgb8();
        let file = fs::File::create(&preview_path)?;
        let mut encoder = JpegEncoder::new_with_quality(file, self.quality);
        encoder
            .encode(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| {
                StorageError::Io(std::io::Error::other(format!("jpeg encode error: {e}")))
            })?;

        let preview_size = fs::metadata(&preview_path)?.len();

        Ok(ThumbnailInfo {
            preview_path: preview_path.display().to_string(),
            width: w,
            height: h,
            original_size,
            preview_size,
        })
    }
}

impl Default for ThumbnailGenerator {
    fn default() -> Self {
        Self::new()
    }
}

fn hash_path(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(path.display().to_string().as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_owned()
}

enum ThumbnailTask {
    Generate {
        item_id: String,
        image_path: PathBuf,
    },
    Shutdown,
}

pub struct ThumbnailWorker {
    sender: Sender<ThumbnailTask>,
    handle: Option<JoinHandle<()>>,
}

impl ThumbnailWorker {
    pub fn start(preview_dir: PathBuf, database: Arc<Database>) -> Self {
        let (sender, receiver) = mpsc::channel::<ThumbnailTask>();

        let handle = thread::spawn(move || {
            let generator = ThumbnailGenerator::new();
            while let Ok(ThumbnailTask::Generate {
                item_id,
                image_path,
            }) = receiver.recv()
            {
                match generator.generate(&image_path, &preview_dir) {
                    Ok(info) => {
                        let preview_path = &info.preview_path;
                        if let Err(e) = database.set_preview_path(&item_id, preview_path) {
                            eprintln!("[thumbnail] failed to update preview for {item_id}: {e}");
                        }
                    }
                    Err(e) => {
                        eprintln!("[thumbnail] thumbnail generation failed for {item_id}: {e}");
                    }
                }
            }
        });

        Self {
            sender,
            handle: Some(handle),
        }
    }

    pub fn enqueue(&self, item_id: String, image_path: PathBuf) {
        let _ = self.sender.send(ThumbnailTask::Generate {
            item_id,
            image_path,
        });
    }

    pub fn stop(&mut self) {
        let _ = self.sender.send(ThumbnailTask::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
