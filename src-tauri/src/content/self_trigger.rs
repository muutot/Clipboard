use std::time::{Duration, Instant};

use crate::content::hash::{
    compute_clipboard_write_hashes, compute_content_hash, compute_file_capture_hash,
    compute_media_write_hashes,
};

/// How long a text/file self-trigger marker stays valid. Matches the media
/// window on purpose: the capture thread can be blocked for several seconds
/// while it ingests a large image (DIB read + PNG re-encode + database write)
/// before it reaches the hash check for a text written right after, and an
/// expired marker would re-capture the app's own paste. The cost — an external
/// copy of identical content within the window is swallowed once — is the same
/// trade the media path already accepts.
const ENTRY_TTL: Duration = Duration::from_secs(5);
/// Media markers use the same window: the capture thread can spend several
/// seconds waking up (500ms poll), reading the DIB and re-encoding a large
/// screenshot to PNG before it reaches the hash check, and an expired marker
/// would re-capture the image the app itself just pasted.
const MEDIA_ENTRY_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupResult {
    pub is_duplicate: bool,
    pub existing_id: Option<String>,
}

struct SelfTriggerEntry {
    hash: String,
    registration: String,
    timestamp: Instant,
    ttl: Duration,
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

    /// Registers one hash and returns the receipt for cancelling this attempt.
    pub fn mark_as_self_triggered(&mut self, content_hash: &str) -> String {
        self.mark_with_ttl(vec![content_hash.to_owned()], ENTRY_TTL)
    }

    fn mark_with_ttl(&mut self, hashes: Vec<String>, ttl: Duration) -> String {
        self.cleanup_expired();
        let registration = uuid::Uuid::new_v4().to_string();
        let timestamp = Instant::now();
        self.entries
            .extend(hashes.into_iter().map(|hash| SelfTriggerEntry {
                hash,
                registration: registration.clone(),
                timestamp,
                ttl,
            }));
        registration
    }

    /// Registers canonical text/link/file hashes under one independent receipt.
    pub fn mark_clipboard_write(&mut self, text: &str) -> String {
        self.mark_with_ttl(compute_clipboard_write_hashes(text), ENTRY_TTL)
    }

    /// Registers raw and normalized media hashes under one independent receipt.
    pub fn mark_media_write(&mut self, kind: &str, data: &[u8]) -> String {
        self.mark_with_ttl(compute_media_write_hashes(kind, data), MEDIA_ENTRY_TTL)
    }

    /// Rolls back only this write attempt's hashes. Other attempts can own the
    /// same hashes independently, including a successful write before a retry.
    pub fn cancel_registration(&mut self, registration: &str) {
        self.entries
            .retain(|entry| entry.registration != registration);
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
            .retain(|entry| now.duration_since(entry.timestamp) < entry.ttl);
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
    fn unmarking_a_failed_write_restores_external_capture() {
        let mut guard = SelfTriggerGuard::new();
        let registration = guard.mark_clipboard_write("https://example.com");
        guard.cancel_registration(&registration);

        assert!(!guard.is_text_write_self_triggered("text", "https://example.com"));
        assert!(!guard.is_text_write_self_triggered("link", "https://example.com"));
        // Unrelated markers survive.
        guard.mark_clipboard_write("other");
        guard.cancel_registration("unknown-registration");
        assert!(guard.is_text_write_self_triggered("text", "other"));
    }

    #[test]
    fn cancellation_preserves_each_other_owner_and_is_idempotent() {
        let mut guard = SelfTriggerGuard::new();
        let first = guard.mark_clipboard_write("same");
        let second = guard.mark_clipboard_write("same");
        assert_ne!(first, second);
        guard.cancel_registration(&first);
        guard.cancel_registration(&first);
        assert!(guard.is_text_write_self_triggered("text", "same"));
        guard.cancel_registration(&second);
        assert!(!guard.is_text_write_self_triggered("text", "same"));
    }

    #[test]
    fn failed_retry_does_not_extend_an_expired_successful_registration() {
        let mut guard = SelfTriggerGuard::new();
        guard.mark_clipboard_write("same");
        for entry in &mut guard.entries {
            entry.timestamp -= ENTRY_TTL;
        }
        let retry = guard.mark_clipboard_write("same");
        guard.cancel_registration(&retry);
        assert!(!guard.is_text_write_self_triggered("text", "same"));
        assert!(guard.entries.is_empty());
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
