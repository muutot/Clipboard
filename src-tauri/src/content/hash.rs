use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::storage::StorageError;

pub fn compute_content_hash(kind: &str, text: &str, resource_path: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(text.as_bytes());
    if let Some(path) = resource_path {
        hasher.update(path.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub fn compute_media_hash(kind: &str, data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupResult {
    pub is_duplicate: bool,
    pub existing_id: Option<String>,
}

struct SelfTriggerEntry {
    hash: String,
    timestamp: Instant,
}

pub struct SelfTriggerGuard {
    entries: Vec<SelfTriggerEntry>,
}

impl SelfTriggerGuard {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn mark_as_self_triggered(&mut self, content_hash: &str) {
        self.cleanup_expired();
        self.entries.push(SelfTriggerEntry {
            hash: content_hash.to_owned(),
            timestamp: Instant::now(),
        });
    }

    pub fn is_self_triggered(&mut self, content_hash: &str) -> bool {
        self.cleanup_expired();
        self.entries.iter().any(|entry| entry.hash == content_hash)
    }

    fn cleanup_expired(&mut self) {
        let now = Instant::now();
        self.entries
            .retain(|entry| now.duration_since(entry.timestamp) < Duration::from_secs(2));
    }
}

impl Default for SelfTriggerGuard {
    fn default() -> Self {
        Self::new()
    }
}

pub fn icon_key(source_app: &str) -> String {
    source_app
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_owned()
}

pub struct AppIconStore {
    icons_dir: PathBuf,
}

impl AppIconStore {
    pub fn new(icons_dir: PathBuf) -> Result<Self, StorageError> {
        fs::create_dir_all(&icons_dir)?;
        Ok(Self { icons_dir })
    }

    pub fn store_icon(&self, key: &str, data: &[u8]) -> Result<PathBuf, StorageError> {
        let hash = compute_content_hash("icon", key, None);
        let ext = infer_icon_extension(data);
        let filename = format!("{}.{}", hash, ext);
        let path = self.icons_dir.join(&filename);

        if !path.exists() {
            fs::write(&path, data)?;
        }

        Ok(path)
    }

    pub fn get_icon_path(&self, key: &str) -> PathBuf {
        let hash = compute_content_hash("icon", key, None);
        let possible_exts = ["png", "ico", "svg", "jpg", "jpeg"];
        for ext in &possible_exts {
            let path = self.icons_dir.join(format!("{}.{}", hash, ext));
            if path.exists() {
                return path;
            }
        }
        self.icons_dir.join(format!("{}.png", hash))
    }
}

fn infer_icon_extension(data: &[u8]) -> &'static str {
    if data.len() >= 3 && &data[0..3] == b"\x89PNG" {
        return "png";
    }
    if data.len() >= 3 && &data[0..3] == b"\xff\xd8\xff" {
        return "jpg";
    }
    if data.len() >= 4 && &data[0..4] == b"RIFF" {
        return "webp";
    }
    if data.len() >= 4 && &data[0..4] == b"\x00\x00\x01\x00" {
        return "ico";
    }
    if data.starts_with(b"<svg") || data.starts_with(b"<?xml") {
        return "svg";
    }
    "png"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_deterministic() {
        let a = compute_content_hash("text", "hello", None);
        let b = compute_content_hash("text", "hello", None);
        assert_eq!(a, b);
    }

    #[test]
    fn content_hash_differs_by_kind() {
        let a = compute_content_hash("text", "hello", None);
        let b = compute_content_hash("link", "hello", None);
        assert_ne!(a, b);
    }

    #[test]
    fn content_hash_includes_resource_path() {
        let a = compute_content_hash("file", "test", Some("/path/a.txt"));
        let b = compute_content_hash("file", "test", Some("/path/b.txt"));
        assert_ne!(a, b);
    }

    #[test]
    fn self_trigger_guard_detects_recent_hash() {
        let mut guard = SelfTriggerGuard::new();
        guard.mark_as_self_triggered("hash-123");
        assert!(guard.is_self_triggered("hash-123"));
        assert!(!guard.is_self_triggered("hash-456"));
    }

    #[test]
    fn self_trigger_guard_has_no_entries_by_default() {
        let mut guard = SelfTriggerGuard::new();
        assert!(!guard.is_self_triggered("anything"));
    }

    #[test]
    fn icon_key_normalizes_app_names() {
        assert_eq!(icon_key("Google Chrome"), "google_chrome");
        assert_eq!(icon_key("Visual Studio Code"), "visual_studio_code");
        assert_eq!(icon_key("  1Password  "), "1password");
    }

    #[test]
    fn app_icon_store_creates_and_retrieves_icons() {
        let temp = std::env::temp_dir().join(format!(
            "clipboard-icon-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = AppIconStore::new(temp.clone()).unwrap();

        let png_data: &[u8] = &[0x89, b'P', b'N', b'G', 0, 0, 0];
        let path = store.store_icon("firefox", png_data).unwrap();
        assert!(path.exists());

        let retrieved = store.get_icon_path("firefox");
        assert_eq!(retrieved, path);

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn infer_icon_extension_detects_png() {
        assert_eq!(infer_icon_extension(&[0x89, b'P', b'N', b'G', 0]), "png");
    }

    #[test]
    fn infer_icon_extension_detects_jpg() {
        assert_eq!(infer_icon_extension(&[0xff, 0xd8, 0xff, 0]), "jpg");
    }
}
