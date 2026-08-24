use std::time::{Duration, Instant};

use crate::content::hash::{
    compute_clipboard_write_hashes, compute_content_hash, compute_file_capture_hash,
    compute_media_write_hashes,
};

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
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.hash == content_hash)
        {
            entry.timestamp = Instant::now();
            return;
        }
        self.entries.push(SelfTriggerEntry {
            hash: content_hash.to_owned(),
            timestamp: Instant::now(),
        });
    }

    pub fn mark_clipboard_write(&mut self, text: &str) {
        for hash in compute_clipboard_write_hashes(text) {
            self.mark_as_self_triggered(&hash);
        }
    }

    pub fn mark_media_write(&mut self, kind: &str, data: &[u8]) {
        for hash in compute_media_write_hashes(kind, data) {
            self.mark_as_self_triggered(&hash);
        }
    }

    pub fn is_self_triggered(&mut self, content_hash: &str) -> bool {
        self.cleanup_expired();
        self.entries.iter().any(|entry| entry.hash == content_hash)
    }

    pub fn is_text_write_self_triggered(&mut self, kind_name: &str, text: &str) -> bool {
        self.is_self_triggered(&compute_content_hash(kind_name, text, None))
    }

    pub fn is_file_write_self_triggered(&mut self, paths: &[String]) -> bool {
        let content_hash = compute_file_capture_hash(paths);
        self.is_self_triggered(&content_hash)
    }

    pub fn is_media_write_self_triggered(&mut self, kind: &str, data: &[u8]) -> bool {
        self.media_write_hashes(kind, data)
            .into_iter()
            .any(|content_hash| self.is_self_triggered(&content_hash))
    }

    /// The raw (byte-level) hashes behind [`SelfTriggerGuard::is_media_write_self_triggered`],
    /// exposed so the capture path can reuse them for storage instead of
    /// re-hashing the same buffer (for large screenshots this avoids a second
    /// full SHA-256 pass per capture).
    pub fn media_write_hashes(&self, kind: &str, data: &[u8]) -> Vec<String> {
        compute_media_write_hashes(kind, data)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::hash::{
        compute_clipboard_write_hashes, compute_content_hash, compute_file_capture_hash,
        compute_media_hash, compute_normalized_media_hash,
    };

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
    fn text_write_hashes_cover_text_and_link_capture_kinds() {
        let text = "https://example.com";
        let hashes = compute_clipboard_write_hashes(text);

        assert!(hashes.contains(&compute_content_hash("text", text, None)));
        assert!(hashes.contains(&compute_content_hash("link", text, None)));
        assert!(hashes.contains(&compute_content_hash("file", text, None)));
    }

    #[test]
    fn file_capture_hash_matches_clipboard_write_hashes() {
        let single = vec![r"C:\Users\admin\Documents\report.txt".to_owned()];
        assert!(compute_clipboard_write_hashes(&single[0])
            .contains(&compute_file_capture_hash(&single)));

        let paths = vec![
            r"C:\Users\admin\Documents\alpha.txt".to_owned(),
            r"C:\Users\admin\Documents\zeta.txt".to_owned(),
        ];
        assert!(compute_clipboard_write_hashes(&paths.join("\n"))
            .contains(&compute_file_capture_hash(&paths)));
    }

    #[test]
    fn guard_capture_checks_match_registered_text_write() {
        let mut guard = SelfTriggerGuard::new();
        guard.mark_clipboard_write("https://example.com");

        assert!(guard.is_text_write_self_triggered("text", "https://example.com"));
        assert!(guard.is_text_write_self_triggered("link", "https://example.com"));
        assert!(!guard.is_text_write_self_triggered("text", "other text"));
    }

    #[test]
    fn guard_capture_checks_match_registered_file_writes() {
        let single_path = r"C:\Users\admin\Documents\report.txt".to_owned();
        let mut single_guard = SelfTriggerGuard::new();
        single_guard.mark_clipboard_write(&single_path);
        assert!(single_guard.is_file_write_self_triggered(std::slice::from_ref(&single_path)));

        let paths = vec![
            r"C:\Users\admin\Documents\alpha.txt".to_owned(),
            r"C:\Users\admin\Documents\zeta.txt".to_owned(),
        ];
        let mut group_guard = SelfTriggerGuard::new();
        group_guard.mark_clipboard_write(&paths.join("\n"));
        assert!(group_guard.is_file_write_self_triggered(&paths));
    }

    #[test]
    fn self_trigger_guard_matches_windows_newline_normalization() {
        let mut guard = SelfTriggerGuard::new();
        guard.mark_clipboard_write("first\nsecond");

        assert!(guard.is_self_triggered(&compute_content_hash("text", "first\r\nsecond", None)));
        assert!(guard.is_self_triggered(&compute_content_hash("link", "first\r\nsecond", None)));
        assert!(guard.is_self_triggered(&compute_content_hash("files", "first\r\nsecond", None)));
    }

    #[test]
    fn normalized_media_hash_ignores_image_container_encoding() {
        use std::io::Cursor;

        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 255]));
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("PNG encoding should succeed");
        let mut bmp = Cursor::new(Vec::new());
        image
            .write_to(&mut bmp, image::ImageFormat::Bmp)
            .expect("BMP encoding should succeed");

        assert_ne!(
            compute_media_hash("image", png.get_ref()),
            compute_media_hash("image", bmp.get_ref())
        );
        assert_eq!(
            compute_normalized_media_hash("image", png.get_ref()),
            compute_normalized_media_hash("image", bmp.get_ref())
        );
    }

    #[test]
    fn media_write_registration_matches_a_differently_encoded_image() {
        use std::io::Cursor;

        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([200, 100, 50, 255]));
        let mut stored = Cursor::new(Vec::new());
        image
            .write_to(&mut stored, image::ImageFormat::Png)
            .expect("PNG encoding should succeed");
        let mut clipboard = Cursor::new(Vec::new());
        image
            .write_to(&mut clipboard, image::ImageFormat::Bmp)
            .expect("BMP encoding should succeed");

        let mut guard = SelfTriggerGuard::new();
        guard.mark_media_write("image", stored.get_ref());
        assert!(
            guard.is_self_triggered(&compute_normalized_media_hash("image", clipboard.get_ref()))
        );
    }
}
