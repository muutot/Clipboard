use std::path::{Path, PathBuf};

pub const MODEL_VERSION: &str = "v6";
const OAR_HOME_DIR: &str = "ppocr";

pub fn models_dir(storage_path: &Path) -> PathBuf {
    storage_path.join("models").join(OAR_HOME_DIR)
}

pub fn set_oar_home(dir: &Path) {
    std::env::set_var("OAR_HOME", dir);
}

pub fn all_models_present(dir: &Path) -> bool {
    dir.join("pp-ocrv6_small_det.onnx").exists()
        && dir.join("pp-ocrv6_small_rec.onnx").exists()
        && dir.join("ppocrv6_dict.txt").exists()
}
