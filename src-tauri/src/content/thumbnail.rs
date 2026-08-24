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
    hex::encode(hasher.finalize())[..16].to_owned()
}

enum ThumbnailTask {
    Generate {
        item_id: String,
        image_path: PathBuf,
    },
    Shutdown,
}

/// Cloneable enqueue handle so producer threads (e.g. the capture worker)
/// never own the worker lifecycle.
#[derive(Clone)]
pub struct ThumbnailQueue {
    sender: Sender<ThumbnailTask>,
}

impl ThumbnailQueue {
    pub fn enqueue(&self, item_id: String, image_path: PathBuf) {
        let _ = self.sender.send(ThumbnailTask::Generate {
            item_id,
            image_path,
        });
    }
}

pub struct ThumbnailWorker {
    sender: Sender<ThumbnailTask>,
    handle: Option<JoinHandle<()>>,
}

impl ThumbnailWorker {
    pub fn start(preview_dir: PathBuf, database: Arc<Database>) -> Self {
        let (sender, receiver) = mpsc::channel::<ThumbnailTask>();

        let handle = thread::Builder::new()
            .name("thumbnail".to_owned())
            .spawn(move || {
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
                                crate::log_event!(
                                    "[thumbnail] failed to update preview for {item_id}: {e}"
                                );
                            }
                        }
                        Err(e) => {
                            crate::log_event!(
                                "[thumbnail] thumbnail generation failed for {item_id}: {e}"
                            );
                        }
                    }
                }
            })
            .expect("failed to spawn the thumbnail worker thread");

        Self {
            sender,
            handle: Some(handle),
        }
    }

    pub fn queue(&self) -> ThumbnailQueue {
        ThumbnailQueue {
            sender: self.sender.clone(),
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
            if handle.thread().id() != thread::current().id() && handle.join().is_err() {
                crate::log_event!("[thumbnail] worker thread terminated with a panic");
            }
        }
    }
}

impl Drop for ThumbnailWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime};

    use crate::domain::{ClipboardItem, ClipboardKind};
    use crate::storage::{ClipboardRepository, Database};

    use super::{ThumbnailGenerator, ThumbnailWorker};

    fn temporary_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "clipboard-thumbnail-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_png(dir: &std::path::Path, width: u32, height: u32) -> PathBuf {
        let path = dir.join("source.png");
        image::RgbImage::new(width, height).save(&path).unwrap();
        path
    }

    fn image_item(id: &str, image_path: &std::path::Path) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Image,
            title: id.to_owned(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: Some(image_path.display().to_string()),
            preview_path: Some(image_path.display().to_string()),
            content_hash: format!("hash-{id}"),
            source_app: None,
            size_bytes: 1,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    #[test]
    fn generator_caps_preview_width_and_writes_the_file() {
        let dir = temporary_dir("generator");
        let source = write_png(&dir, 800, 40);

        let info = ThumbnailGenerator::new().generate(&source, &dir).unwrap();

        assert_eq!(info.width, 400);
        assert_eq!(info.height, 20);
        assert!(std::fs::metadata(&info.preview_path).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worker_updates_the_preview_path_and_stops_cleanly() {
        let dir = temporary_dir("worker");
        let source = write_png(&dir, 100, 60);
        let database = Arc::new(Database::open(dir.join("db.sqlite")).unwrap());
        database.save_item(&image_item("img", &source)).unwrap();

        let mut worker = ThumbnailWorker::start(dir.join("previews"), Arc::clone(&database));
        worker.queue().enqueue("img".to_owned(), source.clone());

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let stored = database.get_item("img").unwrap().unwrap();
            if stored.preview_path.as_deref() != Some(source.display().to_string().as_str()) {
                assert!(stored.preview_path.unwrap().ends_with(".jpg"));
                break;
            }
            assert!(Instant::now() < deadline, "thumbnail was never generated");
            std::thread::sleep(Duration::from_millis(20));
        }

        worker.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stop_is_idempotent_and_enqueue_after_stop_does_not_panic() {
        let dir = temporary_dir("stop");
        let database = Arc::new(Database::open(dir.join("db.sqlite")).unwrap());

        let mut worker = ThumbnailWorker::start(dir.join("previews"), database);
        let queue = worker.queue();
        worker.stop();
        worker.stop();
        queue.enqueue("img".to_owned(), dir.join("missing.png"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
