use std::{fs, path::PathBuf};

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub project: PathBuf,
    pub storage: PathBuf,
    pub images: PathBuf,
    pub previews: PathBuf,
    pub files: PathBuf,
    pub database_directory: PathBuf,
    pub database: PathBuf,
    pub search_index: PathBuf,
}

impl StoragePaths {
    pub fn initialize(project: PathBuf) -> Result<Self, std::io::Error> {
        let storage = project.join("storage");
        let images = storage.join("image");
        let database_directory = storage.join("database");
        let paths = Self {
            previews: images.join("previews"),
            files: storage.join("files"),
            database: database_directory.join("clipboard.sqlite3"),
            search_index: database_directory.join("search-index"),
            project,
            storage,
            images,
            database_directory,
        };

        for directory in [
            &paths.project,
            &paths.storage,
            &paths.images,
            &paths.previews,
            &paths.files,
            &paths.database_directory,
            &paths.search_index,
        ] {
            fs::create_dir_all(directory)?;
        }

        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::StoragePaths;

    #[test]
    fn creates_the_project_storage_layout() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project = std::env::temp_dir().join(format!(
            "clipboard-storage-paths-{}-{unique}",
            std::process::id()
        ));

        let paths = StoragePaths::initialize(project.clone()).unwrap();

        assert_eq!(paths.storage, project.join("storage"));
        assert_eq!(paths.images, project.join("storage/image"));
        assert_eq!(paths.files, project.join("storage/files"));
        assert_eq!(paths.database_directory, project.join("storage/database"));
        assert_eq!(
            paths.database,
            project.join("storage/database/clipboard.sqlite3")
        );
        assert!(paths.previews.is_dir());
        assert!(paths.search_index.is_dir());

        fs::remove_dir_all(project).unwrap();
    }
}
