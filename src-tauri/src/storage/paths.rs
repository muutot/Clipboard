use std::{fs, path::PathBuf};

use super::StorageError;

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub project: PathBuf,
    pub data_directory: PathBuf,
    pub storage: PathBuf,
    pub images: PathBuf,
    pub previews: PathBuf,
    pub files: PathBuf,
    pub database_directory: PathBuf,
    pub database: PathBuf,
    pub search_index: PathBuf,
}

impl StoragePaths {
    pub fn initialize(project: PathBuf) -> Result<Self, StorageError> {
        Self::initialize_with_data_directory(project, None)
    }

    pub fn initialize_with_data_directory(
        project: PathBuf,
        data_directory: Option<PathBuf>,
    ) -> Result<Self, StorageError> {
        Self::initialize_with_resource_directories(project, data_directory, None, None)
    }

    pub fn initialize_with_resource_directories(
        project: PathBuf,
        data_directory: Option<PathBuf>,
        image_storage_path: Option<PathBuf>,
        file_storage_path: Option<PathBuf>,
    ) -> Result<Self, StorageError> {
        let data_directory = data_directory.unwrap_or_else(|| project.clone());
        if !data_directory.is_absolute() {
            return Err(StorageError::DataDirectoryMustBeAbsolute(data_directory));
        }

        let storage = if data_directory
            .file_name()
            .map(|n| n == "storage")
            .unwrap_or(false)
        {
            data_directory.clone()
        } else {
            data_directory.join("storage")
        };
        let images = resource_directory(
            image_storage_path,
            storage.join("image"),
            "storage.imageStoragePath",
        )?;
        let files = resource_directory(
            file_storage_path,
            storage.join("files"),
            "storage.fileStoragePath",
        )?;
        if images == files {
            return Err(StorageError::ResourceDirectoriesMustBeDistinct);
        }
        let database_directory = storage.join("database");
        let paths = Self {
            previews: images.join("previews"),
            files,
            database: database_directory.join("clipboard.sqlite3"),
            search_index: database_directory.join("search-index"),
            project,
            data_directory,
            storage,
            images,
            database_directory,
        };

        for directory in [
            &paths.project,
            &paths.data_directory,
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

    pub fn uses_custom_data_directory(&self) -> bool {
        self.data_directory != self.project
    }
}

fn resource_directory(
    configured: Option<PathBuf>,
    default: PathBuf,
    field: &'static str,
) -> Result<PathBuf, StorageError> {
    let directory = configured.unwrap_or(default);
    if !directory.is_absolute() {
        return Err(StorageError::ResourceDirectoryMustBeAbsolute {
            field,
            path: directory,
        });
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::SystemTime,
    };

    use super::StoragePaths;
    use crate::storage::StorageError;

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

        assert_eq!(paths.data_directory, project);
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

    #[test]
    fn places_storage_under_a_custom_data_directory() {
        let root = temporary_test_directory("custom");
        let project = root.join("project");
        let data_directory = root.join("data");

        let paths = StoragePaths::initialize_with_data_directory(
            project.clone(),
            Some(data_directory.clone()),
        )
        .unwrap();

        assert_eq!(paths.project, project);
        assert_eq!(paths.data_directory, data_directory);
        assert_eq!(paths.storage, root.join("data/storage"));
        assert!(paths.uses_custom_data_directory());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uses_independent_image_and_file_storage_directories() {
        let root = temporary_test_directory("resource-directories");
        let project = root.join("project");
        let images = root.join("screenshots");
        let files = root.join("managed-files");

        let paths = StoragePaths::initialize_with_resource_directories(
            project,
            None,
            Some(images.clone()),
            Some(files.clone()),
        )
        .unwrap();

        assert_eq!(paths.images, images);
        assert_eq!(paths.files, files);
        assert!(paths.images.is_dir());
        assert!(paths.files.is_dir());
        assert!(paths.previews.starts_with(&paths.images));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_relative_or_shared_resource_directories() {
        let project = temporary_test_directory("invalid-resource-directories");
        let shared = project.join("resources");

        assert!(matches!(
            StoragePaths::initialize_with_resource_directories(
                project.clone(),
                None,
                Some(PathBuf::from("relative")),
                None,
            ),
            Err(StorageError::ResourceDirectoryMustBeAbsolute { .. })
        ));
        assert!(matches!(
            StoragePaths::initialize_with_resource_directories(
                project,
                None,
                Some(shared.clone()),
                Some(shared),
            ),
            Err(StorageError::ResourceDirectoriesMustBeDistinct)
        ));
    }

    #[test]
    fn rejects_a_relative_data_directory() {
        let project = temporary_test_directory("relative");

        let error = StoragePaths::initialize_with_data_directory(
            project.clone(),
            Some(PathBuf::from("relative/data")),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            StorageError::DataDirectoryMustBeAbsolute(path)
                if path == Path::new("relative/data")
        ));
        assert!(!project.exists());
    }

    fn temporary_test_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "clipboard-storage-paths-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
