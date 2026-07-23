use std::{fs, path::PathBuf};

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub root: PathBuf,
    pub database: PathBuf,
    pub files: PathBuf,
    pub screenshots: PathBuf,
    pub previews: PathBuf,
    pub search_index: PathBuf,
    pub models: PathBuf,
}

impl StoragePaths {
    pub fn initialize(root: PathBuf) -> Result<Self, std::io::Error> {
        let paths = Self {
            database: root.join("clipboard.sqlite3"),
            files: root.join("files"),
            screenshots: root.join("screenshots"),
            previews: root.join("previews"),
            search_index: root.join("search-index"),
            models: root.join("models"),
            root,
        };

        for directory in [
            &paths.root,
            &paths.files,
            &paths.screenshots,
            &paths.previews,
            &paths.search_index,
            &paths.models,
        ] {
            fs::create_dir_all(directory)?;
        }

        Ok(paths)
    }
}
