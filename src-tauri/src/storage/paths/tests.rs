use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::store::resource_root_marker_content;
use super::{ResourceRootRole, StoragePaths, RESOURCE_ROOT_MARKER};
use crate::storage::StorageError;

#[test]
fn failed_marker_replacement_preserves_existing_entry_and_removes_staging() {
    let root = temporary_test_directory("failed-marker");
    let resource = root.join("resources");
    let paths = StoragePaths::initialize_with_resource_directories(
        root.join("project"),
        None,
        None,
        Some(resource.clone()),
    )
    .unwrap();
    let marker = resource.join(RESOURCE_ROOT_MARKER);
    fs::create_dir(&marker).unwrap();
    fs::write(marker.join("keep.txt"), b"existing contents").unwrap();
    let result = paths.claim_resource_root(ResourceRootRole::File);
    let retained = fs::read(marker.join("keep.txt")).unwrap();
    let leftovers = fs::read_dir(&resource)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".clipboard-resource-root.tmp-")
        })
        .count();
    fs::remove_dir_all(root).unwrap();
    assert!(result.is_err());
    assert_eq!(retained, b"existing contents");
    assert_eq!(leftovers, 0);
}

#[test]
fn claiming_a_root_preserves_an_external_hard_link_target_of_the_marker() {
    let root = temporary_test_directory("linked-marker");
    let resource = root.join("resources");
    let paths = StoragePaths::initialize_with_resource_directories(
        root.join("project"),
        None,
        None,
        Some(resource.clone()),
    )
    .unwrap();
    let external = root.join("external-user-file.txt");
    let original = b"unrelated external contents must remain unchanged";
    fs::write(&external, original).unwrap();
    fs::hard_link(&external, resource.join(RESOURCE_ROOT_MARKER)).unwrap();
    paths.claim_resource_root(ResourceRootRole::File).unwrap();
    let retained = fs::read(&external).unwrap();
    let marker_valid = paths.resource_root_marker_valid(ResourceRootRole::File);
    fs::remove_dir_all(root).unwrap();
    assert_eq!(retained, original);
    assert!(marker_valid);
}

#[test]
fn ownership_transfer_waits_for_unlink_guard_and_stale_revoke_keeps_foreign_marker() {
    let root = temporary_test_directory("transfer-lock");
    let shared = root.join("resources");
    let first = StoragePaths::initialize_with_resource_directories_for_configuration(
        root.join("first"),
        None,
        None,
        Some(shared.clone()),
    )
    .unwrap();
    let second = StoragePaths::initialize_with_resource_directories(
        root.join("second"),
        None,
        None,
        Some(shared.clone()),
    )
    .unwrap();
    let ownership = first
        .resource_root_cleanup_lock(ResourceRootRole::File)
        .unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let transfer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        second.claim_resource_root(ResourceRootRole::File).unwrap();
        finished_tx.send(()).unwrap();
        second
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    assert!(matches!(
        finished_rx.recv_timeout(std::time::Duration::from_millis(25)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout),
    ));
    assert!(first.resource_root_marker_valid(ResourceRootRole::File));
    drop(ownership);
    finished_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let second = transfer.join().unwrap();
    assert!(first
        .resource_root_cleanup_lock(ResourceRootRole::File)
        .is_none());
    first
        .remove_resource_root_marker(ResourceRootRole::File)
        .unwrap();
    assert!(second.resource_root_marker_valid(ResourceRootRole::File));
    fs::remove_dir_all(root).unwrap();
}

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
fn claiming_an_unowned_resource_root_enables_cleanup_on_the_next_startup() {
    let root = temporary_test_directory("claim-resource-root");
    let project = root.join("project");
    let data_directory = root.join("data");
    let image_directory = data_directory.join("storage/image");
    fs::create_dir_all(&image_directory).unwrap();
    fs::write(image_directory.join("keep.png"), b"existing managed file").unwrap();

    // First startup against an unmarked, non-empty custom root: cleanup stays
    // off and the existing files are untouched.
    let paths =
        StoragePaths::initialize_with_data_directory(project.clone(), Some(data_directory.clone()))
            .unwrap();
    assert!(!paths.image_cleanup_enabled);
    assert!(!paths.file_cleanup_enabled);
    assert!(!paths.resource_root_marker_valid(ResourceRootRole::Image));
    assert!(!image_directory.join(RESOURCE_ROOT_MARKER).exists());

    // The explicit claim writes coordination metadata, never resource contents.
    paths.claim_resource_root(ResourceRootRole::Image).unwrap();
    assert!(paths.resource_root_marker_valid(ResourceRootRole::Image));
    assert_eq!(
        fs::read(image_directory.join(RESOURCE_ROOT_MARKER)).unwrap(),
        resource_root_marker_content(&project, "image")
    );
    assert!(image_directory.join("keep.png").exists());

    // A later non-claiming startup now sees the valid marker and enables
    // cleanup for that root only.
    let reloaded =
        StoragePaths::initialize_with_data_directory(project.clone(), Some(data_directory.clone()))
            .unwrap();
    assert!(reloaded.image_cleanup_enabled);
    assert!(!reloaded.file_cleanup_enabled);
    assert!(reloaded.image_marker_required);
    assert!(reloaded.file_marker_required);

    // The running state matches the marker. Removing and re-adding it in one
    // session ends where it started, so it must not require a restart.
    assert!(reloaded.cleanup_flags_match_markers());
    reloaded
        .remove_resource_root_marker(ResourceRootRole::Image)
        .unwrap();
    assert!(!image_directory.join(RESOURCE_ROOT_MARKER).exists());
    assert!(!reloaded.cleanup_flags_match_markers());
    reloaded
        .claim_resource_root(ResourceRootRole::Image)
        .unwrap();
    assert!(reloaded.cleanup_flags_match_markers());

    // Removing the marker for good disables cleanup on the next startup.
    reloaded
        .remove_resource_root_marker(ResourceRootRole::Image)
        .unwrap();
    let unclaimed =
        StoragePaths::initialize_with_data_directory(project, Some(data_directory)).unwrap();
    assert!(!unclaimed.image_cleanup_enabled);

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
