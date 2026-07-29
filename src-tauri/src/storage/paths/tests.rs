use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::store::resource_root_marker_content;
use super::{StoragePaths, RESOURCE_ROOT_MARKER};
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

    let paths =
        StoragePaths::initialize_with_data_directory(project.clone(), Some(data_directory.clone()))
            .unwrap();

    assert_eq!(paths.project, project);
    assert_eq!(paths.data_directory, data_directory);
    assert_eq!(paths.storage, root.join("data/storage"));
    assert!(paths.uses_custom_data_directory());
    assert!(!paths.image_cleanup_enabled);
    assert!(!paths.file_cleanup_enabled);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_custom_data_directory_configuration_claims_resource_roots() {
    let root = temporary_test_directory("claimed-custom-data");
    let project = root.join("project");
    let data_directory = root.join("data");

    let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
        project,
        Some(data_directory),
        None,
        None,
    )
    .unwrap();

    assert!(paths.image_cleanup_enabled);
    assert!(paths.file_cleanup_enabled);
    assert!(paths.images.join(RESOURCE_ROOT_MARKER).is_file());
    assert!(paths.files.join(RESOURCE_ROOT_MARKER).is_file());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uses_independent_image_and_file_storage_directories() {
    let root = temporary_test_directory("resource-directories");
    let project = root.join("project");
    let images = root.join("screenshots");
    let files = root.join("managed-files");

    let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
        project.clone(),
        None,
        Some(images.clone()),
        Some(files.clone()),
    )
    .unwrap();

    assert_eq!(paths.images, images);
    assert_eq!(paths.files, files);
    assert!(paths.images.is_dir());
    assert!(paths.files.is_dir());
    assert!(paths.image_cleanup_enabled);
    assert!(paths.file_cleanup_enabled);
    assert_eq!(
        fs::read(paths.images.join(RESOURCE_ROOT_MARKER)).unwrap(),
        resource_root_marker_content(&project, "image")
    );
    assert_eq!(
        fs::read(paths.files.join(RESOURCE_ROOT_MARKER)).unwrap(),
        resource_root_marker_content(&project, "file")
    );
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
        StoragePaths::initialize_with_resource_directories_for_configuration(
            project,
            None,
            Some(shared.clone()),
            Some(shared),
        ),
        Err(StorageError::ResourceDirectoriesMustBeDistinct)
    ));
}

#[test]
fn rejects_custom_resource_directories_that_overlap_application_storage() {
    let root = temporary_test_directory("reserved-resource-directories");
    let project = root.join("project");
    let reserved = project.join("storage/database");

    let error = StoragePaths::initialize_with_resource_directories_for_configuration(
        project,
        None,
        Some(reserved),
        Some(root.join("files")),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        StorageError::ResourceDirectoryReserved { .. }
    ));
    assert!(!root.exists());
}

#[test]
fn rejects_nested_custom_resource_directories() {
    let root = temporary_test_directory("nested-resource-directories");
    let project = root.join("project");
    let image_path = root.join("resources");
    let file_path = image_path.join("files");

    let error = StoragePaths::initialize_with_resource_directories_for_configuration(
        project,
        None,
        Some(image_path),
        Some(file_path),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        StorageError::ResourceDirectoriesOverlap { .. }
    ));
    assert!(!root.exists());
}

#[test]
fn rejects_equivalent_paths_with_parent_segments() {
    let root = temporary_test_directory("normalized-resource-directories");
    let project = root.join("project");
    let image_path = root.join("resources");
    let file_path = root.join("resources/subdir/..");

    let error = StoragePaths::initialize_with_resource_directories_for_configuration(
        project,
        None,
        Some(image_path),
        Some(file_path),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        StorageError::ResourceDirectoriesMustBeDistinct
            | StorageError::ResourceDirectoriesOverlap { .. }
    ));
    assert!(!root.exists());
}

#[test]
fn disables_cleanup_for_existing_unmarked_custom_directories() {
    let root = temporary_test_directory("legacy-resource-directory");
    let project = root.join("project");
    let images = root.join("screenshots");
    let files = root.join("managed-files");
    fs::create_dir_all(&images).unwrap();
    fs::create_dir_all(&files).unwrap();
    fs::write(images.join("keep.txt"), b"user data").unwrap();
    fs::write(files.join("keep.txt"), b"user data").unwrap();

    let paths = StoragePaths::initialize_with_resource_directories(
        project,
        None,
        Some(images.clone()),
        Some(files.clone()),
    )
    .unwrap();

    assert!(!paths.image_cleanup_enabled);
    assert!(!paths.file_cleanup_enabled);
    assert!(!images.join(RESOURCE_ROOT_MARKER).exists());
    assert!(!files.join(RESOURCE_ROOT_MARKER).exists());
    assert!(images.join("keep.txt").exists());
    assert!(files.join("keep.txt").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_does_not_claim_empty_custom_resource_directories() {
    let root = temporary_test_directory("empty-unclaimed-resource-directory");
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

    assert!(!paths.image_cleanup_enabled);
    assert!(!paths.file_cleanup_enabled);
    assert!(!images.join(RESOURCE_ROOT_MARKER).exists());
    assert!(!files.join(RESOURCE_ROOT_MARKER).exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_or_foreign_markers_never_enable_cleanup() {
    let root = temporary_test_directory("foreign-resource-marker");
    let project = root.join("project");
    let images = root.join("screenshots");
    let files = root.join("managed-files");
    fs::create_dir_all(&images).unwrap();
    fs::create_dir_all(&files).unwrap();
    fs::write(
        images.join(RESOURCE_ROOT_MARKER),
        b"not this application's marker",
    )
    .unwrap();
    fs::write(
        files.join(RESOURCE_ROOT_MARKER),
        resource_root_marker_content(&root.join("other-project"), "file"),
    )
    .unwrap();

    let paths = StoragePaths::initialize_with_resource_directories(
        project.clone(),
        None,
        Some(images.clone()),
        Some(files.clone()),
    )
    .unwrap();
    assert!(!paths.image_cleanup_enabled);
    assert!(!paths.file_cleanup_enabled);

    assert!(matches!(
        StoragePaths::initialize_with_resource_directories_for_configuration(
            project,
            None,
            Some(images),
            Some(files),
        ),
        Err(StorageError::ResourceDirectoryMustBeEmptyOrOwned { .. })
    ));

    fs::remove_dir_all(root).unwrap();
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
