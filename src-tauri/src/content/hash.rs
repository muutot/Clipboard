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
    hex::encode(hasher.finalize())
}

pub fn compute_media_hash(kind: &str, data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Computes an image hash from decoded RGBA pixels instead of the source
/// container bytes. Windows exposes a `ClipboardItem` image as DIB data and
/// the capture path re-encodes it as PNG, so hashing pixels keeps a copied
/// image stable across PNG metadata/encoding differences introduced by the
/// browser or the clipboard implementation.
///
/// Non-image data and undecodable image data intentionally fall back to the
/// byte hash so callers retain a deterministic identity even for malformed
/// or legacy records.
pub fn compute_normalized_media_hash(kind: &str, data: &[u8]) -> String {
    if kind != "image" {
        return compute_media_hash(kind, data);
    }

    let Ok(image) = image::load_from_memory(data) else {
        return compute_media_hash(kind, data);
    };
    let rgba = image.to_rgba8();
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(rgba.width().to_le_bytes());
    hasher.update(rgba.height().to_le_bytes());
    hasher.update(rgba.as_raw());
    hex::encode(hasher.finalize())
}

/// Returns the hashes that can be observed for a media write. The raw hash
/// keeps compatibility with existing persisted image records, while the
/// normalized hash is the stable identity used across clipboard encodings.
pub fn compute_media_write_hashes(kind: &str, data: &[u8]) -> Vec<String> {
    let raw = compute_media_hash(kind, data);
    let normalized = compute_normalized_media_hash(kind, data);
    if normalized == raw {
        vec![raw]
    } else {
        vec![raw, normalized]
    }
}

pub fn compute_clipboard_write_hashes(text: &str) -> Vec<String> {
    let text_variants = newline_variants(text);
    let mut hashes = Vec::with_capacity(text_variants.len() * 4);
    for variant in &text_variants {
        for kind in ["text", "link", "file"] {
            hashes.push(compute_content_hash(kind, variant, None));
        }
    }

    let mut sorted_paths = text_variants
        .first()
        .filter(|variant| variant.contains('\n'))
        .map(|variant| variant.lines().map(str::to_owned).collect::<Vec<_>>());
    if let Some(paths) = &mut sorted_paths {
        paths.sort();
        let joined = paths.join("\n");
        for variant in newline_variants(&joined) {
            hashes.push(compute_content_hash("files", &variant, None));
        }
    }

    hashes
}

/// Canonical hash for a CF_HDROP file capture: a single path hashes as
/// "file", a multi-path group hashes as the sorted "files" join. Text
/// write-backs register the same hashes via `compute_clipboard_write_hashes`,
/// so capture and write-back share one rule.
pub fn compute_file_capture_hash(paths: &[String]) -> String {
    if let [single] = paths {
        return compute_content_hash("file", single, None);
    }
    let mut sorted = paths.to_vec();
    sorted.sort();
    compute_content_hash("files", &sorted.join("\n"), None)
}

fn newline_variants(text: &str) -> Vec<String> {
    let normalized_lf = text.replace("\r\n", "\n").replace('\r', "\n");
    let normalized_crlf = normalized_lf.replace('\n', "\r\n");
    let mut variants = vec![text.to_owned()];
    for variant in [normalized_lf, normalized_crlf] {
        if !variants.contains(&variant) {
            variants.push(variant);
        }
    }
    variants
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

    /// Capture-side counterpart of `mark_clipboard_write` for observed text
    /// of the given capture kind ("text" or "link").
    pub fn is_text_write_self_triggered(&mut self, kind_name: &str, text: &str) -> bool {
        self.is_self_triggered(&compute_content_hash(kind_name, text, None))
    }

    /// Capture-side counterpart of `mark_clipboard_write` for CF_HDROP
    /// captures; applies the shared single-path/"files" group rule.
    pub fn is_file_write_self_triggered(&mut self, paths: &[String]) -> bool {
        let content_hash = compute_file_capture_hash(paths);
        self.is_self_triggered(&content_hash)
    }

    /// Capture-side counterpart of `mark_media_write`; checks both the raw
    /// and normalized media hashes.
    pub fn is_media_write_self_triggered(&mut self, kind: &str, data: &[u8]) -> bool {
        compute_media_write_hashes(kind, data)
            .into_iter()
            .any(|content_hash| self.is_self_triggered(&content_hash))
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
            r"C:\Users\admin\Documents\zeta.txt".to_owned(),
            r"C:\Users\admin\Documents\alpha.txt".to_owned(),
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
            r"C:\Users\admin\Documents\zeta.txt".to_owned(),
            r"C:\Users\admin\Documents\alpha.txt".to_owned(),
        ];
        let mut group_guard = SelfTriggerGuard::new();
        group_guard.mark_clipboard_write(&paths.join("\n"));
        assert!(group_guard.is_file_write_self_triggered(&paths));
    }

    #[test]
    fn self_trigger_guard_matches_windows_newline_normalization() {
        let mut guard = SelfTriggerGuard::new();
        guard.mark_clipboard_write("first\nsecond");

        assert!(guard.is_self_triggered(&compute_content_hash("text", "first\r\nsecond", None,)));
        assert!(guard.is_self_triggered(&compute_content_hash("link", "first\r\nsecond", None,)));
        assert!(guard.is_self_triggered(&compute_content_hash("files", "first\r\nsecond", None,)));
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
