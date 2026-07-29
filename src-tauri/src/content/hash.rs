use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

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
