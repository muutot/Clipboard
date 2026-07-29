use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use super::types::{StoragePaths, RESOURCE_ROOT_MARKER, RESOURCE_ROOT_MARKER_HEADER};
use crate::storage::StorageError;

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
        Self::initialize_with_resource_directories_internal(
            project,
            data_directory,
            image_storage_path,
            file_storage_path,
            false,
        )
    }

    /// Validate and explicitly claim empty custom resource roots selected in
    /// the settings UI. Runtime startup deliberately uses the non-claiming
    /// initializer above so a legacy or user-owned directory is never made
    /// cleanup-eligible merely because it happens to be empty.
    pub fn initialize_with_resource_directories_for_configuration(
        project: PathBuf,
        data_directory: Option<PathBuf>,
        image_storage_path: Option<PathBuf>,
        file_storage_path: Option<PathBuf>,
    ) -> Result<Self, StorageError> {
        Self::initialize_with_resource_directories_internal(
            project,
            data_directory,
            image_storage_path,
            file_storage_path,
            true,
        )
    }

    fn initialize_with_resource_directories_internal(
        project: PathBuf,
        data_directory: Option<PathBuf>,
        image_storage_path: Option<PathBuf>,
        file_storage_path: Option<PathBuf>,
        claim_custom_roots: bool,
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
        let default_resource_roots_require_marker = data_directory != project;
        let image_directory = resource_directory(
            image_storage_path,
            storage.join("image"),
            "storage.imageStoragePath",
            default_resource_roots_require_marker,
        )?;
        let file_directory = resource_directory(
            file_storage_path,
            storage.join("files"),
            "storage.fileStoragePath",
            default_resource_roots_require_marker,
        )?;
        let resource_paths_overlap = paths_intersect(&image_directory.path, &file_directory.path);
        if claim_custom_roots && resource_paths_overlap {
            if paths_equal(&image_directory.path, &file_directory.path) {
                return Err(StorageError::ResourceDirectoriesMustBeDistinct);
            }
            return Err(StorageError::ResourceDirectoriesOverlap {
                first: image_directory.path.clone(),
                second: file_directory.path.clone(),
            });
        }
        let database_directory = storage.join("database");
        let image_reserved = custom_resource_directory_conflict(
            &project,
            &data_directory,
            &storage,
            &database_directory,
            &storage.join("icons"),
            &image_directory,
        );
        let file_reserved = custom_resource_directory_conflict(
            &project,
            &data_directory,
            &storage,
            &database_directory,
            &storage.join("icons"),
            &file_directory,
        );
        if claim_custom_roots {
            if let Some(reserved) = image_reserved.as_ref() {
                return Err(StorageError::ResourceDirectoryReserved {
                    field: "storage.imageStoragePath",
                    path: image_directory.path.clone(),
                    reserved: reserved.clone(),
                });
            }
            if let Some(reserved) = file_reserved.as_ref() {
                return Err(StorageError::ResourceDirectoryReserved {
                    field: "storage.fileStoragePath",
                    path: file_directory.path.clone(),
                    reserved: reserved.clone(),
                });
            }
        }
        let paths = Self {
            previews: image_directory.path.join("previews"),
            files: file_directory.path.clone(),
            database: database_directory.join("clipboard.sqlite3"),
            search_index: database_directory.join("search-index"),
            project,
            data_directory,
            storage,
            images: image_directory.path.clone(),
            database_directory,
            image_cleanup_enabled: false,
            file_cleanup_enabled: false,
        };

        let image_cleanup_enabled = if image_directory.requires_marker {
            !resource_paths_overlap
                && image_reserved.is_none()
                && resource_root_cleanup_enabled(
                    &paths.images,
                    &paths.project,
                    "image",
                    "storage.imageStoragePath",
                    claim_custom_roots,
                )?
        } else {
            true
        };
        let file_cleanup_enabled = if file_directory.requires_marker {
            !resource_paths_overlap
                && file_reserved.is_none()
                && resource_root_cleanup_enabled(
                    &paths.files,
                    &paths.project,
                    "file",
                    "storage.fileStoragePath",
                    claim_custom_roots,
                )?
        } else {
            true
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

        let mut paths = paths;
        paths.image_cleanup_enabled = image_cleanup_enabled;
        paths.file_cleanup_enabled = file_cleanup_enabled;

        Ok(paths)
    }

    pub fn uses_custom_data_directory(&self) -> bool {
        self.data_directory != self.project
    }
}

struct ResourceDirectory {
    path: PathBuf,
    configured: bool,
    requires_marker: bool,
}

fn resource_directory(
    configured: Option<PathBuf>,
    default: PathBuf,
    field: &'static str,
    default_requires_marker: bool,
) -> Result<ResourceDirectory, StorageError> {
    let configured_resource = configured.is_some();
    let directory = configured.unwrap_or(default);
    if !directory.is_absolute() {
        return Err(StorageError::ResourceDirectoryMustBeAbsolute {
            field,
            path: directory,
        });
    }
    Ok(ResourceDirectory {
        path: directory,
        configured: configured_resource,
        requires_marker: configured_resource || default_requires_marker,
    })
}

fn custom_resource_directory_conflict(
    project: &Path,
    data_directory: &Path,
    storage: &Path,
    database_directory: &Path,
    icons_directory: &Path,
    resource: &ResourceDirectory,
) -> Option<PathBuf> {
    if !resource.configured {
        return None;
    }

    for reserved in [
        project,
        data_directory,
        storage,
        database_directory,
        icons_directory,
    ] {
        if paths_intersect(&resource.path, reserved) {
            return Some(reserved.to_path_buf());
        }
    }

    None
}

fn resource_root_cleanup_enabled(
    path: &Path,
    project: &Path,
    role: &str,
    field: &'static str,
    claim: bool,
) -> Result<bool, StorageError> {
    let marker = path.join(RESOURCE_ROOT_MARKER);
    let expected = resource_root_marker_content(project, role);
    if marker.is_file() {
        let valid = fs::read(&marker)
            .map(|content| content == expected)
            .unwrap_or(false);
        if valid {
            return Ok(true);
        }
        if claim {
            return Err(StorageError::ResourceDirectoryMustBeEmptyOrOwned {
                field,
                path: path.to_path_buf(),
            });
        }
        return Ok(false);
    }

    if !claim {
        return Ok(false);
    }

    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    let has_entries = fs::read_dir(path)?.next().is_some();
    if has_entries {
        return Err(StorageError::ResourceDirectoryMustBeEmptyOrOwned {
            field,
            path: path.to_path_buf(),
        });
    }

    fs::write(marker, expected)?;
    Ok(true)
}

pub(super) fn resource_root_marker_content(project: &Path, role: &str) -> Vec<u8> {
    format!(
        "{RESOURCE_ROOT_MARKER_HEADER}\nproject={}\nrole={role}\n",
        lexical_path_key(project)
    )
    .into_bytes()
}

fn paths_intersect(first: &Path, second: &Path) -> bool {
    let first = comparable_path_components(first);
    let second = comparable_path_components(second);
    is_component_prefix(&first, &second) || is_component_prefix(&second, &first)
}

fn paths_equal(first: &Path, second: &Path) -> bool {
    comparable_path_components(first) == comparable_path_components(second)
}

fn is_component_prefix(first: &[String], second: &[String]) -> bool {
    first.len() <= second.len() && first.iter().zip(second).all(|(a, b)| a == b)
}

fn comparable_path_components(path: &Path) -> Vec<String> {
    let path = canonicalize_through_existing_parent(path);
    let value = path.to_string_lossy().replace('\\', "/");
    let value = if cfg!(target_os = "windows") {
        value.to_ascii_lowercase()
    } else {
        value
    };
    value
        .split('/')
        .filter(|component| !component.is_empty())
        .map(str::to_owned)
        .collect()
}

fn canonicalize_through_existing_parent(path: &Path) -> PathBuf {
    let mut existing = lexical_normalize(path);
    let mut missing_components = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name() else {
            break;
        };
        missing_components.push(name.to_owned());
        if !existing.pop() {
            break;
        }
    }

    let mut normalized =
        fs::canonicalize(&existing).unwrap_or_else(|_| lexical_normalize(&existing));
    for component in missing_components.iter().rev() {
        normalized.push(component);
    }
    lexical_normalize(&normalized)
}

fn lexical_path_key(path: &Path) -> String {
    let value = lexical_normalize(path).to_string_lossy().replace('\\', "/");
    if cfg!(target_os = "windows") {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}
