use std::path::{Path, PathBuf};

const OAR_HOME_DIR: &str = "ppocr";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpOcrModelFile {
    pub filename: &'static str,
    pub label: &'static str,
    pub size_bytes: u64,
    pub url: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpOcrModelSpec {
    pub id: &'static str,
    pub model_version: &'static str,
    pub detection: PpOcrModelFile,
    pub recognition: PpOcrModelFile,
    pub dictionary: PpOcrModelFile,
}

impl PpOcrModelSpec {
    pub fn files(&self) -> [PpOcrModelFile; 3] {
        [self.detection, self.recognition, self.dictionary]
    }
}

const TINY_MODEL: PpOcrModelSpec = PpOcrModelSpec {
    id: "tiny",
    model_version: "v6-tiny",
    detection: PpOcrModelFile {
        filename: "pp-ocrv6_tiny_det.onnx",
        label: "检测模型",
        size_bytes: 1_780_590,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/pp-ocrv6_tiny_det.onnx"
        ),
    },
    recognition: PpOcrModelFile {
        filename: "pp-ocrv6_tiny_rec.onnx",
        label: "识别模型",
        size_bytes: 4_462_639,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/pp-ocrv6_tiny_rec.onnx"
        ),
    },
    dictionary: PpOcrModelFile {
        filename: "ppocrv6_tiny_dict.txt",
        label: "字典文件",
        size_bytes: 27_156,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/ppocrv6_tiny_dict.txt"
        ),
    },
};

const SMALL_MODEL: PpOcrModelSpec = PpOcrModelSpec {
    id: "small",
    model_version: "v6-small",
    detection: PpOcrModelFile {
        filename: "pp-ocrv6_small_det.onnx",
        label: "检测模型",
        size_bytes: 9_880_512,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/pp-ocrv6_small_det.onnx"
        ),
    },
    recognition: PpOcrModelFile {
        filename: "pp-ocrv6_small_rec.onnx",
        label: "识别模型",
        size_bytes: 21_159_378,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/pp-ocrv6_small_rec.onnx"
        ),
    },
    dictionary: PpOcrModelFile {
        filename: "ppocrv6_dict.txt",
        label: "字典文件",
        size_bytes: 74_947,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/ppocrv6_dict.txt"
        ),
    },
};

const MEDIUM_MODEL: PpOcrModelSpec = PpOcrModelSpec {
    id: "medium",
    model_version: "v6-medium",
    detection: PpOcrModelFile {
        filename: "pp-ocrv6_medium_det.onnx",
        label: "检测模型",
        size_bytes: 62_032_837,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/pp-ocrv6_medium_det.onnx"
        ),
    },
    recognition: PpOcrModelFile {
        filename: "pp-ocrv6_medium_rec.onnx",
        label: "识别模型",
        size_bytes: 76_554_979,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/pp-ocrv6_medium_rec.onnx"
        ),
    },
    dictionary: PpOcrModelFile {
        filename: "ppocrv6_dict.txt",
        label: "字典文件",
        size_bytes: 74_947,
        url: concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0",
            "/ppocrv6_dict.txt"
        ),
    },
};

pub const SUPPORTED_MODEL_SPECS: [PpOcrModelSpec; 3] = [TINY_MODEL, SMALL_MODEL, MEDIUM_MODEL];

pub fn models_dir(storage_path: &Path) -> PathBuf {
    storage_path.join("models").join(OAR_HOME_DIR)
}

pub fn set_oar_home(dir: &Path) {
    std::env::set_var("OAR_HOME", dir);
}

pub fn model_spec(value: &str) -> Option<&'static PpOcrModelSpec> {
    match value.trim() {
        "tiny" => Some(&SUPPORTED_MODEL_SPECS[0]),
        "small" | "large" => Some(&SUPPORTED_MODEL_SPECS[1]),
        "medium" => Some(&SUPPORTED_MODEL_SPECS[2]),
        _ => None,
    }
}

pub fn default_model_spec() -> &'static PpOcrModelSpec {
    &SUPPORTED_MODEL_SPECS[1]
}

pub fn model_file_is_installed(dir: &Path, file: &PpOcrModelFile) -> bool {
    dir.join(file.filename)
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == file.size_bytes)
}

/// Path of the locally recorded SHA-256 digest for an installed model file
/// (trust-on-first-use pinning: written after every successful download).
pub fn model_digest_path(dir: &Path, file: &PpOcrModelFile) -> PathBuf {
    dir.join(format!("{}.sha256", file.filename))
}

fn compute_file_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut hasher = Sha256::new();
    let mut source = std::fs::File::open(path)?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Hashes `path` and persists the digest next to the model file so later
/// install attempts can detect on-disk corruption or replacement without
/// relying on upstream-pinned hashes.
pub fn record_model_digest(dir: &Path, file: &PpOcrModelFile) -> std::io::Result<()> {
    let digest = compute_file_sha256(&dir.join(file.filename))?;
    std::fs::write(model_digest_path(dir, file), digest)
        .map_err(|error| error_with_path("record model digest", dir.join(file.filename), error))
}

/// Returns `false` when a locally recorded digest exists but the model file
/// no longer matches it. Missing digests (models installed before TOFU
/// pinning was introduced) fall back to the size-only check.
pub fn model_digest_matches(dir: &Path, file: &PpOcrModelFile) -> bool {
    let digest_path = model_digest_path(dir, file);
    let Ok(recorded) = std::fs::read_to_string(&digest_path) else {
        return true;
    };
    let recorded = recorded.trim();
    match compute_file_sha256(&dir.join(file.filename)) {
        Ok(actual) => actual.eq_ignore_ascii_case(recorded),
        Err(error) => {
            crate::log_event!(
                "[ocr] cannot hash {} for digest verification: {error}",
                file.filename
            );
            false
        }
    }
}

fn error_with_path(context: &str, path: PathBuf, error: std::io::Error) -> std::io::Error {
    std::io::Error::new(
        error.kind(),
        format!("{context} {}: {error}", path.display()),
    )
}

pub fn model_is_installed(dir: &Path, spec: &PpOcrModelSpec) -> bool {
    spec.files()
        .iter()
        .all(|file| model_file_is_installed(dir, file))
}

pub fn installed_model_variants(dir: &Path) -> Vec<&'static str> {
    SUPPORTED_MODEL_SPECS
        .iter()
        .filter(|spec| model_is_installed(dir, spec))
        .map(|spec| spec.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::time::SystemTime;

    use super::{
        model_digest_matches, model_is_installed, model_spec, record_model_digest, PpOcrModelFile,
    };

    #[test]
    fn maps_supported_and_legacy_variant_names_to_canonical_specs() {
        assert_eq!(model_spec("tiny").unwrap().id, "tiny");
        assert_eq!(model_spec("small").unwrap().id, "small");
        assert_eq!(model_spec("medium").unwrap().id, "medium");
        assert_eq!(model_spec("large").unwrap().id, "small");
        assert!(model_spec("unknown").is_none());
    }

    #[test]
    fn detects_only_complete_files_for_the_selected_variant() {
        let dir = temporary_test_directory("variant-files");
        fs::create_dir_all(&dir).unwrap();
        let tiny = model_spec("tiny").unwrap();

        assert!(!model_is_installed(&dir, tiny));
        for file in tiny.files() {
            create_sized_file(&dir, &file);
        }
        assert!(model_is_installed(&dir, tiny));
        assert!(!model_is_installed(&dir, model_spec("small").unwrap()));

        File::options()
            .write(true)
            .open(dir.join(tiny.detection.filename))
            .unwrap()
            .set_len(tiny.detection.size_bytes - 1)
            .unwrap();
        assert!(!model_is_installed(&dir, tiny));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tofu_digest_detects_post_install_modification() {
        let dir = temporary_test_directory("tofu-digest");
        fs::create_dir_all(&dir).unwrap();
        let file = model_spec("tiny").unwrap().dictionary;

        create_sized_file(&dir, &file);
        // No digest recorded yet (legacy install): size-only fallback applies.
        assert!(model_digest_matches(&dir, &file));

        record_model_digest(&dir, &file).unwrap();
        assert!(model_digest_matches(&dir, &file));

        // Flip one byte in place: same size, different content.
        let path = dir.join(file.filename);
        let mut bytes = fs::read(&path).unwrap();
        bytes[0] ^= 0xff;
        fs::write(&path, &bytes).unwrap();
        assert!(!model_digest_matches(&dir, &file));

        fs::remove_dir_all(dir).unwrap();
    }

    fn create_sized_file(dir: &std::path::Path, file: &PpOcrModelFile) {
        File::create(dir.join(file.filename))
            .unwrap()
            .set_len(file.size_bytes)
            .unwrap();
    }

    fn temporary_test_directory(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "clipboard-ocr-models-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
