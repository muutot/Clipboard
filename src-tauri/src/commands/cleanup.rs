use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::Manager;

use crate::commands::lock::lock_state;
use crate::config::ConfigStore;
use crate::state::CaptureState;
use crate::storage::{
    ClipboardRepository, Database, ResourceRootRole, StorageFileReferences, StoragePaths,
    RESOURCE_ROOT_LOCK, RESOURCE_ROOT_MARKER,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCleanupResult {
    pub(crate) removed_files: u64,
    pub(crate) freed_bytes: u64,
}

/// Extra retention for fresh files, including interrupted publication. Active
/// resource writers are protected by the database's publication guard; mtime
/// alone cannot protect reuse of an old content-addressed file.
pub const ORPHAN_FILE_GRACE: Duration = Duration::from_secs(10 * 60);

/// The retention inputs a cleanup run needs, snapshotted from configuration.
///
/// The cleanup walks every resource root and canonicalizes every referenced
/// path, which can take minutes on a large library. Passing the values instead
/// of a `&ConfigStore` is what lets the command read the global config mutex for
/// a few statements rather than for the whole run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupPolicy {
    pub retention_days: u32,
    pub max_items: u64,
    pub recycle_bin_days: u32,
}

impl CleanupPolicy {
    pub fn from_config(config: &ConfigStore) -> Self {
        Self {
            retention_days: config.retention_days(),
            max_items: config.max_items() as u64,
            recycle_bin_days: config.recycle_bin_days(),
        }
    }
}

/// Async: the cleanup walks every resource root and canonicalizes every
/// referenced path, which can take minutes on a large library, so it runs on
/// the dedicated blocking pool instead of the main thread.
#[tauri::command]
pub async fn enforce_history_cleanup(app: tauri::AppHandle) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        let config = app.state::<Mutex<ConfigStore>>();
        let paths = app.state::<StoragePaths>();
        let capture = app.state::<CaptureState>();
        // Read the three values and drop the guard before doing any work: the
        // cleanup walks every resource root and canonicalizes every referenced
        // path, so holding the process-wide config mutex across it would stall
        // every other command that reads configuration (settings, window config, the
        // tray) and the auto-sync worker's per-tick read.
        let policy = {
            let guard = lock_state(&config, "configuration lock is poisoned")?;
            CleanupPolicy::from_config(&guard)
        };
        let _maintenance = lock_state(
            &capture.storage_maintenance_lock,
            "storage maintenance lock is poisoned",
        )?;
        enforce_history_cleanup_with_policy(&database, &paths, policy, ORPHAN_FILE_GRACE)
    })
    .await
    .map_err(|error| format!("cleanup task join failed: {error}"))?
}

pub fn enforce_history_cleanup_for(
    database: &Database,
    config: &ConfigStore,
    paths: &StoragePaths,
    orphan_file_grace: Duration,
) -> Result<u64, String> {
    enforce_history_cleanup_with_policy(
        database,
        paths,
        CleanupPolicy::from_config(config),
        orphan_file_grace,
    )
}

pub fn enforce_history_cleanup_with_policy(
    database: &Database,
    paths: &StoragePaths,
    policy: CleanupPolicy,
    orphan_file_grace: Duration,
) -> Result<u64, String> {
    let mut total_deleted = 0u64;
    total_deleted += database
        .delete_older_than(policy.retention_days)
        .map_err(|error| error.to_string())?;
    total_deleted += database
        .enforce_capacity_limit(policy.max_items)
        .map_err(|error| error.to_string())?;
    total_deleted += database
        .permanently_delete_expired(policy.recycle_bin_days)
        .map_err(|error| error.to_string())?;

    if let Err(error) = cleanup_orphan_storage_files_with_grace(database, paths, orphan_file_grace)
    {
        // Surface the failure instead of reporting a clean run that silently
        // left orphan files on disk.
        crate::log_error!("[cleanup] orphan file cleanup failed: {error}");
    }

    Ok(total_deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_defers_busy_or_invalid_custom_root_ownership_lock() {
        for invalid_lock in [false, true] {
            let root = std::env::temp_dir()
                .join(format!("clipboard-ownership-busy-{}", uuid::Uuid::new_v4()));
            let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
                root.join("project"),
                None,
                None,
                Some(root.join("resources")),
            )
            .unwrap();
            let database = Database::open(&paths.database).unwrap();
            let orphan = paths.files.join("orphan.txt");
            std::fs::write(&orphan, b"fixture").unwrap();
            let lock_path = paths.files.join(RESOURCE_ROOT_LOCK);
            let held = if invalid_lock {
                std::fs::remove_file(&lock_path).unwrap();
                std::fs::create_dir(&lock_path).unwrap();
                None
            } else {
                Some(
                    paths
                        .resource_root_cleanup_lock(ResourceRootRole::File)
                        .unwrap(),
                )
            };
            let result = cleanup_orphan_storage_files(&database, &paths).unwrap();
            assert_eq!(result.removed_files, 0);
            assert!(orphan.is_file());
            drop(held);
            if invalid_lock {
                std::fs::remove_dir(&lock_path).unwrap();
            }
            let result = cleanup_orphan_storage_files(&database, &paths).unwrap();
            assert_eq!(result.removed_files, 1);
            assert!(
                lock_path.is_file(),
                "cleanup must retain its coordination lock"
            );
            assert!(paths.resource_root_marker_valid(ResourceRootRole::File));
            drop(database);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn cleanup_rechecks_custom_root_ownership_after_snapshot() {
        use crate::domain::{ClipboardItem, ClipboardKind};
        use crate::storage::ResourceRootRole;

        let root = std::env::temp_dir().join(format!(
            "clipboard-ownership-transfer-{}",
            uuid::Uuid::new_v4()
        ));
        let shared = root.join("resources");
        let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
            root.join("first"),
            None,
            None,
            Some(shared.clone()),
        )
        .unwrap();
        let other = StoragePaths::initialize_with_resource_directories(
            root.join("second"),
            None,
            None,
            Some(shared.clone()),
        )
        .unwrap();
        let database = Database::open(&paths.database).unwrap();
        let other_database = Database::open(&other.database).unwrap();
        let body = shared.join("other-owner.txt");
        std::fs::write(&body, b"other owner's bytes").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&body)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(3600))
            .unwrap();
        let result = cleanup_orphans_after_snapshot(&database, &paths, ORPHAN_FILE_GRACE, || {
            // The explicit ownership action can transfer an already-used root.
            // Its new owner's references belong to an independent database.
            other.claim_resource_root(ResourceRootRole::File).unwrap();
            other_database
                .save_item(&ClipboardItem {
                    id: "other-owner".into(),
                    kind: ClipboardKind::File,
                    title: "fixture".into(),
                    text_content: None,
                    html_content: None,
                    rtf_content: None,
                    resource_path: Some(body.display().to_string()),
                    preview_path: None,
                    content_hash: "fixture".into(),
                    source_app: None,
                    size_bytes: 19,
                    created_at_ms: 1,
                    last_used_at_ms: None,
                    is_favorite: false,
                    icon_path: None,
                    metadata_json: None,
                })
                .unwrap();
            assert!(!paths.resource_root_marker_valid(ResourceRootRole::File));
            assert!(other.resource_root_marker_valid(ResourceRootRole::File));
        })
        .unwrap();
        let retained = body.exists();
        drop(other_database);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            retained,
            "a stale owner's cleanup must retain the new owner's body"
        );
        assert_eq!(result.removed_files, 0);
    }

    fn policy_paths() -> StoragePaths {
        let project = std::env::temp_dir().join(format!(
            "clipboard-cleanup-policy-test-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        StoragePaths::initialize(project).expect("temporary storage paths")
    }

    #[test]
    fn cleanup_policy_reads_the_clamped_configuration_values() {
        let paths = policy_paths();
        let config = ConfigStore::load(&paths.project).expect("temporary config");
        let policy = CleanupPolicy::from_config(&config);
        assert_eq!(policy.retention_days, config.retention_days());
        assert_eq!(policy.max_items, config.max_items() as u64);
        assert_eq!(policy.recycle_bin_days, config.recycle_bin_days());
        let _ = std::fs::remove_dir_all(&paths.project);
    }

    #[test]
    fn cleanup_defers_active_publication_and_resumes_after_failure() {
        let paths = policy_paths();
        let database = Database::open(&paths.database).unwrap();
        let writer = Database::open(&paths.database).unwrap();
        let orphan = paths.files.join("pending.txt");
        std::fs::write(&orphan, b"fixture").unwrap();
        let guard = writer.begin_resource_write().unwrap();
        let result = cleanup_orphan_storage_files(&database, &paths).unwrap();
        assert_eq!(result.removed_files, 0);
        assert!(orphan.exists());
        drop(guard); // Simulate a failed operation that did not publish a DB row.
        let result = cleanup_orphan_storage_files(&database, &paths).unwrap();
        assert_eq!(result.removed_files, 1);
        assert!(!orphan.exists());
        drop(writer);
        drop(database);
        std::fs::remove_dir_all(paths.project).unwrap();
    }

    #[test]
    fn cleanup_preserves_old_resource_reused_after_reference_snapshot() {
        use crate::content::FileStore;
        use crate::domain::{ClipboardItem, ClipboardKind};
        let paths = policy_paths();
        let database = Database::open(&paths.database).unwrap();
        let writer = Database::open(&paths.database).unwrap();
        let source = paths.project.join("fixture.txt");
        std::fs::write(&source, b"fixture").unwrap();
        let stored = FileStore::save_file(&source, &paths.files, 0).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stored.storage_path)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(3600)),
            )
            .unwrap();
        let result = cleanup_orphans_after_snapshot(&database, &paths, ORPHAN_FILE_GRACE, || {
            let _publication = writer.begin_resource_write().unwrap();
            let reused = FileStore::save_file(&source, &paths.files, 0).unwrap();
            assert_eq!(stored.storage_path, reused.storage_path);
            writer
                .save_item(&ClipboardItem {
                    id: "reused".into(),
                    kind: ClipboardKind::File,
                    title: "fixture".into(),
                    text_content: None,
                    html_content: None,
                    rtf_content: None,
                    resource_path: Some(reused.storage_path),
                    preview_path: None,
                    content_hash: reused.content_hash,
                    source_app: None,
                    size_bytes: 7,
                    created_at_ms: 1,
                    last_used_at_ms: None,
                    is_favorite: false,
                    icon_path: None,
                    metadata_json: None,
                })
                .unwrap();
        })
        .unwrap();
        assert!(
            Path::new(&stored.storage_path).is_file(),
            "a newly referenced file must survive cleanup"
        );
        assert_eq!(result.removed_files, 0);
        assert!(writer.get_item("reused").unwrap().is_some());
        drop(writer);
        drop(database);
        std::fs::remove_dir_all(paths.project).unwrap();
    }

    #[test]
    fn cleanup_runs_from_a_snapshotted_policy_without_the_config_store() {
        let paths = policy_paths();
        let database = Database::open(&paths.database).expect("temporary database");
        let policy = CleanupPolicy {
            retention_days: 30,
            max_items: 10_000,
            recycle_bin_days: 7,
        };
        // The point of the snapshot is that the run needs no `ConfigStore` at
        // all, so a long cleanup can never hold the shared configuration mutex.
        let total =
            enforce_history_cleanup_with_policy(&database, &paths, policy, ORPHAN_FILE_GRACE)
                .expect("cleanup must run");
        assert_eq!(total, 0, "an empty library deletes nothing");
        let _ = std::fs::remove_dir_all(&paths.project);
    }

    #[test]
    #[ignore = "subprocess fixture invoked by the interprocess resource test"]
    fn resource_publication_child_fixture() {
        use crate::content::FileStore;
        use crate::domain::{ClipboardItem, ClipboardKind};
        let project = PathBuf::from(
            std::env::var_os("CLIPBOARD_RESOURCE_TEST_PROJECT").expect("fixture project"),
        );
        assert_eq!(
            project.parent().unwrap().canonicalize().unwrap(),
            std::env::temp_dir().canonicalize().unwrap()
        );
        assert_eq!(
            std::fs::read(project.join("publication-test.marker")).unwrap(),
            b"temporary resource-publication fixture"
        );
        let paths = StoragePaths::initialize(project).unwrap();
        let database = Database::open(&paths.database).unwrap();
        let _publication = database.begin_resource_write().unwrap();
        if std::env::var_os("CLIPBOARD_RESOURCE_TEST_HOLD").is_some() {
            std::fs::write(paths.project.join("writer-ready"), b"ready").unwrap();
            // The parent kills only this fixture to verify OS lock release on
            // abrupt exit. A bounded wait also prevents a stranded child.
            std::thread::sleep(Duration::from_secs(15));
            return;
        }
        let stored =
            FileStore::save_file(&paths.project.join("fixture.txt"), &paths.files, 0).unwrap();
        database
            .save_item(&ClipboardItem {
                id: "other-process".into(),
                kind: ClipboardKind::File,
                title: "fixture".into(),
                text_content: None,
                html_content: None,
                rtf_content: None,
                resource_path: Some(stored.storage_path),
                preview_path: None,
                content_hash: stored.content_hash,
                source_app: None,
                size_bytes: 7,
                created_at_ms: 1,
                last_used_at_ms: None,
                is_favorite: false,
                icon_path: None,
                metadata_json: None,
            })
            .unwrap();
    }

    #[test]
    fn cleanup_preserves_a_resource_published_by_another_process_after_the_snapshot() {
        use crate::content::FileStore;
        let paths = policy_paths();
        let database = Database::open(&paths.database).unwrap();
        std::fs::write(
            paths.project.join("publication-test.marker"),
            b"temporary resource-publication fixture",
        )
        .unwrap();
        let source = paths.project.join("fixture.txt");
        std::fs::write(&source, b"fixture").unwrap();
        let stored = FileStore::save_file(&source, &paths.files, 0).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stored.storage_path)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(3600)),
            )
            .unwrap();
        let result = cleanup_orphans_after_snapshot(&database, &paths, ORPHAN_FILE_GRACE, || {
            let mut child = PublicationChild::start(&paths, false);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    panic!("resource fixture timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
        .unwrap();
        assert!(database.get_item("other-process").unwrap().is_some());
        assert!(
            Path::new(&stored.storage_path).is_file(),
            "another process's newly referenced resource must survive"
        );
        assert_eq!(result.removed_files, 0);
        drop(database);
        std::fs::remove_dir_all(paths.project).unwrap();
    }

    struct PublicationChild(std::process::Child);

    impl PublicationChild {
        fn start(paths: &StoragePaths, hold: bool) -> Self {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "commands::cleanup::tests::resource_publication_child_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("CLIPBOARD_RESOURCE_TEST_PROJECT", &paths.project)
                .env_remove("CLIPBOARD_RESOURCE_TEST_HOLD");
            if hold {
                command.env("CLIPBOARD_RESOURCE_TEST_HOLD", "1");
            }
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            Self(command.spawn().unwrap())
        }
    }

    impl Drop for PublicationChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn cleanup_defers_another_process_writer_and_resumes_after_its_abrupt_exit() {
        let paths = policy_paths();
        let database = Database::open(&paths.database).unwrap();
        std::fs::write(
            paths.project.join("publication-test.marker"),
            b"temporary resource-publication fixture",
        )
        .unwrap();
        let orphan = paths.files.join("orphan.txt");
        std::fs::write(&orphan, b"fixture").unwrap();
        let mut child = PublicationChild::start(&paths, true);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !paths.project.join("writer-ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "writer exited before readiness"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "writer fixture timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            cleanup_orphan_storage_files(&database, &paths)
                .unwrap()
                .removed_files,
            0
        );
        assert!(orphan.is_file());
        drop(child); // A killed process does not run ResourceWriteGuard::drop.
        assert_eq!(
            cleanup_orphan_storage_files(&database, &paths)
                .unwrap()
                .removed_files,
            1
        );
        assert!(!orphan.exists());
        drop(database);
        std::fs::remove_dir_all(paths.project).unwrap();
    }
}

pub fn cleanup_orphan_storage_files(
    database: &Database,
    paths: &StoragePaths,
) -> Result<StorageCleanupResult, String> {
    cleanup_orphan_storage_files_with_grace(database, paths, Duration::ZERO)
}

pub fn cleanup_orphan_storage_files_with_grace(
    database: &Database,
    paths: &StoragePaths,
    orphan_file_grace: Duration,
) -> Result<StorageCleanupResult, String> {
    cleanup_orphans_after_snapshot(database, paths, orphan_file_grace, || {})
}

fn cleanup_orphans_after_snapshot(
    database: &Database,
    paths: &StoragePaths,
    orphan_file_grace: Duration,
    after_snapshot: impl FnOnce(),
) -> Result<StorageCleanupResult, String> {
    let Some(publication_snapshot) = database.resource_cleanup_snapshot() else {
        return Ok(StorageCleanupResult {
            removed_files: 0,
            freed_bytes: 0,
        });
    };
    let references = database
        .list_storage_file_references()
        .map_err(|error| error.to_string())?;
    let icons = paths.storage.join("icons");
    let referenced_paths = resolve_storage_file_references(paths, &icons, references);
    after_snapshot();

    let mut removed_files = 0u64;
    let mut freed_bytes = 0u64;

    let image_ownership = paths
        .image_marker_required
        .then_some(ResourceRootRole::Image);
    let file_ownership = paths.file_marker_required.then_some(ResourceRootRole::File);
    let scan_dirs: &[(&Path, bool, Option<ResourceRootRole>)] = &[
        (&paths.images, paths.image_cleanup_enabled, image_ownership),
        (
            &paths.previews,
            paths.image_cleanup_enabled,
            image_ownership,
        ),
        (&paths.files, paths.file_cleanup_enabled, file_ownership),
        (&icons, true, None),
    ];

    'roots: for (dir, cleanup_enabled, ownership) in scan_dirs {
        if !cleanup_enabled || ownership.is_some_and(|role| !paths.resource_root_marker_valid(role))
        {
            crate::log_warn!(
                "[cleanup] skipping unowned resource directory {}",
                dir.display()
            );
            continue;
        }
        if !dir.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                // A single unreadable root must not abort cleanup of the
                // remaining roots; log it and keep scanning.
                crate::log_error!(
                    "[cleanup] failed to read directory {}: {error}",
                    dir.display()
                );
                continue;
            }
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                continue;
            }
            if entry_path
                .file_name()
                .is_some_and(|name| name == RESOURCE_ROOT_MARKER || name == RESOURCE_ROOT_LOCK)
            {
                continue;
            }
            if referenced_paths.contains(&normalized_cleanup_path(&entry_path)) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !orphan_file_grace.is_zero() {
                let Ok(modified_at) = metadata.modified() else {
                    continue;
                };
                if modified_at.elapsed().unwrap_or_default() < orphan_file_grace {
                    continue;
                }
            }
            let size_bytes = metadata.len();
            // The marker may change during the scan. Hold the same root lock
            // used by claim/revoke from validation through this one unlink.
            let _ownership = match ownership {
                Some(role) => match paths.resource_root_cleanup_lock(*role) {
                    Some(lock) => Some(lock),
                    None => continue 'roots,
                },
                None => None,
            };
            let Some(removal) =
                publication_snapshot.remove_if_current(|| std::fs::remove_file(&entry_path))
            else {
                // A producer reused or published a resource since the snapshot.
                // Defer remaining cleanup; never delete from obsolete references.
                break 'roots;
            };
            if let Err(e) = removal {
                crate::log_error!(
                    "[cleanup] failed to remove orphan file {}: {e}",
                    entry_path.display()
                );
            } else {
                removed_files += 1;
                // Only count space actually freed; a failed removal above must
                // not inflate the reported total.
                freed_bytes += size_bytes;
            }
        }
    }

    Ok(StorageCleanupResult {
        removed_files,
        freed_bytes,
    })
}

/// Async: the orphan scan walks every resource root, so it runs on the
/// dedicated blocking pool instead of the main thread.
#[tauri::command]
pub async fn cleanup_storage_files(app: tauri::AppHandle) -> Result<StorageCleanupResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        let paths = app.state::<StoragePaths>();
        let capture = app.state::<CaptureState>();
        let _maintenance = lock_state(
            &capture.storage_maintenance_lock,
            "storage maintenance lock is poisoned",
        )?;
        cleanup_orphan_storage_files_with_grace(&database, &paths, ORPHAN_FILE_GRACE)
    })
    .await
    .map_err(|error| format!("cleanup task join failed: {error}"))?
}

fn resolve_storage_file_references(
    paths: &StoragePaths,
    icons: &Path,
    references: StorageFileReferences,
) -> HashSet<PathBuf> {
    let mut resolved = HashSet::new();
    extend_cleanup_references(
        &mut resolved,
        references.resource_paths,
        &[&paths.storage, &paths.images, &paths.files],
    );
    extend_cleanup_references(
        &mut resolved,
        references.preview_paths,
        &[&paths.storage, &paths.images, &paths.previews],
    );
    extend_cleanup_references(
        &mut resolved,
        references.icon_paths,
        &[&paths.storage, icons],
    );
    resolved
}

fn extend_cleanup_references(
    resolved: &mut HashSet<PathBuf>,
    references: Vec<String>,
    relative_bases: &[&Path],
) {
    for reference in references {
        if reference.trim().is_empty() {
            continue;
        }
        let path = Path::new(&reference);
        if path.is_absolute() {
            resolved.insert(normalized_cleanup_path(path));
        } else {
            resolved.extend(
                relative_bases
                    .iter()
                    .map(|base| normalized_cleanup_path(&base.join(&reference))),
            );
        }
    }
}

fn normalized_cleanup_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
