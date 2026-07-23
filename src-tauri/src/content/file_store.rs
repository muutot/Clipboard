use std::{
    fs,
    io::Read,
    path::Path,
};

use sha2::{Digest, Sha256};

use crate::storage::StorageError;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStorageInfo {
    pub storage_path: String,
    pub original_name: String,
    pub size_bytes: u64,
    pub content_hash: String,
}

pub struct FileStore;

impl FileStore {
    pub fn save_file(
        source_path: &Path,
        file_storage_dir: &Path,
        max_copy_size: u64,
    ) -> Result<FileStorageInfo, StorageError> {
        fs::create_dir_all(file_storage_dir)?;
        let metadata = fs::metadata(source_path)?;
        let size_bytes = metadata.len();

        let original_name = source_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_owned();

        let content_hash = hash_file(source_path)?;

        let storage_dir = file_storage_dir.to_path_buf();
        let storage_path = storage_dir.join(&content_hash);

        if max_copy_size > 0 && size_bytes > max_copy_size {
            return Ok(FileStorageInfo {
                storage_path: source_path.display().to_string(),
                original_name,
                size_bytes,
                content_hash,
            });
        }

        if !storage_path.exists() {
            fs::copy(source_path, &storage_path)?;
        }

        Ok(FileStorageInfo {
            storage_path: storage_path.display().to_string(),
            original_name,
            size_bytes,
            content_hash,
        })
    }

    pub fn save_screenshot(
        data: &[u8],
        screenshot_storage_dir: &Path,
        max_screenshot_size: u64,
    ) -> Result<FileStorageInfo, StorageError> {
        let size_bytes = data.len() as u64;

        let mut hasher = Sha256::new();
        hasher.update(data);
        let content_hash = format!("{:x}", hasher.finalize());

        let ext = if data.len() >= 3 && &data[0..3] == b"\x89PNG" {
            "png"
        } else if data.len() >= 3 && &data[0..3] == b"\xff\xd8\xff" {
            "jpg"
        } else if data.len() >= 4 && &data[0..4] == b"RIFF" {
            "webp"
        } else {
            "png"
        };

        let original_name = format!("screenshot.{}", ext);

        if max_screenshot_size > 0 && size_bytes > max_screenshot_size {
            return Ok(FileStorageInfo {
                storage_path: String::new(),
                original_name,
                size_bytes,
                content_hash,
            });
        }

        fs::create_dir_all(screenshot_storage_dir)?;
        let storage_path = screenshot_storage_dir.join(format!("{}.{}", content_hash, ext));

        if !storage_path.exists() {
            fs::write(&storage_path, data)?;
        }

        Ok(FileStorageInfo {
            storage_path: storage_path.display().to_string(),
            original_name,
            size_bytes,
            content_hash,
        })
    }

    pub fn delete_file(path: &Path) -> Result<(), StorageError> {
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn hash_file(path: &Path) -> Result<String, StorageError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_deletes_a_file() {
        let temp = std::env::temp_dir().join(format!(
            "clipboard-file-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp).unwrap();

        let source = temp.join("source.txt");
        fs::write(&source, b"hello clipboard").unwrap();

        let storage_dir = temp.join("storage");
        let info = FileStore::save_file(&source, &storage_dir, 0).unwrap();

        assert!(Path::new(&info.storage_path).exists());
        assert_eq!(info.original_name, "source.txt");
        assert_eq!(info.size_bytes, 15);

        FileStore::delete_file(Path::new(&info.storage_path)).unwrap();
        assert!(!Path::new(&info.storage_path).exists());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn skips_copy_when_file_exceeds_max_size() {
        let temp = std::env::temp_dir().join(format!(
            "clipboard-file-store-max-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp).unwrap();

        let source = temp.join("large.bin");
        fs::write(&source, vec![0u8; 100]).unwrap();

        let storage_dir = temp.join("storage");
        let info = FileStore::save_file(&source, &storage_dir, 50).unwrap();

        assert_eq!(info.storage_path, source.display().to_string());
        assert!(!storage_dir.exists() || fs::read_dir(&storage_dir).unwrap().count() == 0);

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn saves_screenshot_as_png() {
        let temp = std::env::temp_dir().join(format!(
            "clipboard-screenshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage_dir = temp.join("screenshots");

        let png_data: &[u8] = &[0x89, b'P', b'N', b'G', 0, 0, 0, 1, 2, 3];
        let info = FileStore::save_screenshot(png_data, &storage_dir, 0).unwrap();

        assert!(Path::new(&info.storage_path).exists());
        assert_eq!(info.original_name, "screenshot.png");
        assert_eq!(info.size_bytes, 10);

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn screenshot_exceeding_max_size_returns_empty_path() {
        let temp = std::env::temp_dir().join(format!(
            "clipboard-screenshot-max-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage_dir = temp.join("screenshots");

        let data = vec![0u8; 100];
        let info = FileStore::save_screenshot(&data, &storage_dir, 50).unwrap();

        assert!(info.storage_path.is_empty());
        assert_eq!(info.size_bytes, 100);

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn deduplicates_identical_files() {
        let temp = std::env::temp_dir().join(format!(
            "clipboard-dedup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp).unwrap();

        let source = temp.join("dup.txt");
        fs::write(&source, b"duplicate content").unwrap();

        let storage_dir = temp.join("storage");
        let info1 = FileStore::save_file(&source, &storage_dir, 0).unwrap();
        let info2 = FileStore::save_file(&source, &storage_dir, 0).unwrap();

        assert_eq!(info1.content_hash, info2.content_hash);
        assert_eq!(info1.storage_path, info2.storage_path);

        let _ = fs::remove_dir_all(&temp);
    }
}
