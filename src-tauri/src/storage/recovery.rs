use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags};

use super::{Database, StorageError};

#[derive(Debug, Clone, Default)]
pub struct DatabaseRecoveryReport {
    pub restored_from: PathBuf,
    pub quarantined_database: Option<PathBuf>,
}

pub fn backup_path(database_path: &Path) -> PathBuf {
    sibling_with_suffix(database_path, ".backup")
}

pub fn previous_backup_path(database_path: &Path) -> PathBuf {
    sibling_with_suffix(database_path, ".backup.prev")
}

/// A schema reset deliberately discards historical rows, so old recovery
/// copies must not survive and reintroduce them on a later startup.
pub fn discard_database_backups(database_path: &Path) -> Result<(), StorageError> {
    for path in [
        backup_path(database_path),
        previous_backup_path(database_path),
        sibling_with_suffix(database_path, ".backup.tmp"),
    ] {
        remove_file_if_present(&path).map_err(|error| StorageError::DatabaseRecoveryFailed {
            database: database_path.to_path_buf(),
            reason: format!(
                "discard obsolete database backup {}: {error}",
                path.display()
            ),
        })?;
    }
    Ok(())
}

pub fn discard_database_quarantine(quarantined_database: &Path) -> Result<(), StorageError> {
    for path in [
        quarantined_database.to_path_buf(),
        sibling_with_suffix(quarantined_database, "-wal"),
        sibling_with_suffix(quarantined_database, "-shm"),
    ] {
        remove_file_if_present(&path).map_err(|error| StorageError::DatabaseRecoveryFailed {
            database: quarantined_database.to_path_buf(),
            reason: format!(
                "discard obsolete database quarantine {}: {error}",
                path.display()
            ),
        })?;
    }
    Ok(())
}

pub fn recover_database_if_needed(
    database_path: &Path,
) -> Result<Option<DatabaseRecoveryReport>, StorageError> {
    let current_error = if database_path.exists() {
        match validate_database_file(database_path) {
            Ok(()) => return Ok(None),
            Err(error) => error,
        }
    } else if !backup_path(database_path).exists() && !previous_backup_path(database_path).exists()
    {
        return Ok(None);
    } else {
        "database file is missing".to_owned()
    };

    let candidates = [
        backup_path(database_path),
        previous_backup_path(database_path),
    ];
    let mut candidate_errors = Vec::new();
    let backup = candidates.iter().find(|candidate| {
        if !candidate.exists() {
            return false;
        }
        match validate_database_file(candidate) {
            Ok(()) => true,
            Err(error) => {
                candidate_errors.push(format!("{}: {error}", candidate.display()));
                false
            }
        }
    });

    let Some(backup) = backup else {
        let details = if candidate_errors.is_empty() {
            "no valid database backup was found".to_owned()
        } else {
            candidate_errors.join("; ")
        };
        return Err(StorageError::DatabaseRecoveryUnavailable {
            database: database_path.to_path_buf(),
            reason: format!("current database is invalid ({current_error}); {details}"),
        });
    };

    let quarantined_database = restore_from_backup(database_path, backup)?;
    Ok(Some(DatabaseRecoveryReport {
        restored_from: backup.to_path_buf(),
        quarantined_database,
    }))
}

pub fn refresh_database_backup(
    database: &Database,
    database_path: &Path,
) -> Result<PathBuf, StorageError> {
    refresh_database_backup_with_hook(database, database_path, || {})
}

fn refresh_database_backup_with_hook(
    database: &Database,
    database_path: &Path,
    after_validation: impl FnOnce(),
) -> Result<PathBuf, StorageError> {
    // The connection mutex protects VACUUM only. Keep the temporary file and
    // both generations exclusive through validation, rotation and rollback,
    // including other connections/processes. Never unlink this lock file.
    let canonical = fs::canonicalize(database_path)
        .or_else(|error| {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error);
            }
            // An in-memory source may use a backup base without a live DB file.
            let parent = database_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            Ok(fs::canonicalize(parent)?.join(database_path.file_name().ok_or(error)?))
        })
        .map_err(|error| backup_error(database_path, error))?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(sibling_with_suffix(&canonical, ".backup.lock"))
        .map_err(|error| backup_error(database_path, error))?;
    lock.try_lock().map_err(|error| {
        backup_error(
            database_path,
            match error {
                fs::TryLockError::WouldBlock => {
                    "database backup refresh is already in progress".to_owned()
                }
                fs::TryLockError::Error(error) => format!("lock database backup refresh: {error}"),
            },
        )
    })?;
    let backup = backup_path(database_path);
    let previous = previous_backup_path(database_path);
    let temporary = sibling_with_suffix(database_path, ".backup.tmp");

    remove_file_if_present(&temporary).map_err(|error| backup_error(database_path, error))?;
    database
        .vacuum_into(&temporary)
        .map_err(|error| backup_error(database_path, error))?;

    if let Err(error) = validate_database_file(&temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(backup_error(
            database_path,
            format!("generated backup is invalid: {error}"),
        ));
    }

    after_validation();

    if previous.exists() {
        fs::remove_file(&previous).map_err(|error| backup_error(database_path, error))?;
    }
    if backup.exists() {
        fs::rename(&backup, &previous).map_err(|error| backup_error(database_path, error))?;
    }

    if let Err(error) = fs::rename(&temporary, &backup) {
        let rollback = if previous.exists() && !backup.exists() {
            fs::rename(&previous, &backup).err()
        } else {
            None
        };
        let reason = match rollback {
            Some(rollback) => format!(
                "installing backup failed: {error}; restoring previous backup failed: {rollback}"
            ),
            None => format!("installing backup failed: {error}"),
        };
        return Err(backup_error(database_path, reason));
    }

    Ok(backup)
}

pub fn quarantine_search_index(search_index: &Path) -> Result<Option<PathBuf>, StorageError> {
    if !search_index.exists() {
        return Ok(None);
    }

    let quarantined = unique_sibling(search_index, ".pre-recovery");
    fs::rename(search_index, &quarantined).map_err(|error| {
        StorageError::DatabaseRecoveryFailed {
            database: search_index.to_path_buf(),
            reason: format!("quarantine search index: {error}"),
        }
    })?;
    fs::create_dir_all(search_index).map_err(|error| StorageError::DatabaseRecoveryFailed {
        database: search_index.to_path_buf(),
        reason: format!("recreate search index directory: {error}"),
    })?;

    Ok(Some(quarantined))
}

/// Search data is derived. When the SQLite schema is deliberately reset, the
/// old index must be removed instead of quarantined or reused.
pub fn reset_search_index(search_index: &Path) -> Result<(), StorageError> {
    if let Ok(metadata) = fs::symlink_metadata(search_index) {
        let result = if metadata.file_type().is_symlink() {
            fs::remove_file(search_index)
                .or_else(|file_error| fs::remove_dir(search_index).map_err(|_| file_error))
        } else if metadata.is_file() {
            fs::remove_file(search_index)
        } else {
            fs::remove_dir_all(search_index)
        };
        result.map_err(|error| StorageError::DatabaseRecoveryFailed {
            database: search_index.to_path_buf(),
            reason: format!("remove derived search index: {error}"),
        })?;
    }
    fs::create_dir_all(search_index).map_err(|error| StorageError::DatabaseRecoveryFailed {
        database: search_index.to_path_buf(),
        reason: format!("recreate search index directory: {error}"),
    })
}

fn validate_database_file(path: &Path) -> Result<(), String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| error.to_string())?;

    let mut statement = connection
        .prepare("PRAGMA integrity_check")
        .map_err(|error| error.to_string())?;
    let messages = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if messages.len() != 1 || messages.first().map(String::as_str) != Some("ok") {
        return Err(messages.join("; "));
    }

    let core_table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'table' AND name = 'clipboard_items'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if core_table_count != 1 {
        return Err("clipboard_items table is missing".to_owned());
    }

    Ok(())
}

fn restore_from_backup(
    database_path: &Path,
    backup: &Path,
) -> Result<Option<PathBuf>, StorageError> {
    let temporary = unique_sibling(database_path, ".restore-tmp");
    // A recovery copy is a new writable database, even when the retained
    // backup is read-only. fs::copy would propagate that attribute and let
    // integrity validation succeed while the subsequent database open fails.
    let mut source =
        fs::File::open(backup).map_err(|error| recovery_error(database_path, error))?;
    let mut staged = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| recovery_error(database_path, error))?;
    let copied = std::io::copy(&mut source, &mut staged).and_then(|_| staged.sync_all());
    drop(staged);
    drop(source);
    if let Err(error) = copied {
        // Creation succeeded, so this operation owns the partial stage.
        let _ = fs::remove_file(&temporary);
        return Err(recovery_error(database_path, error));
    }
    if let Err(error) = validate_database_file(&temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(recovery_error(
            database_path,
            format!("backup became invalid while copying: {error}"),
        ));
    }

    let quarantined = unique_sibling(database_path, ".corrupt");
    let sidecars = [
        (
            sibling_with_suffix(database_path, "-wal"),
            sibling_with_suffix(&quarantined, "-wal"),
        ),
        (
            sibling_with_suffix(database_path, "-shm"),
            sibling_with_suffix(&quarantined, "-shm"),
        ),
    ];
    let mut moved = Vec::new();

    for (original, target) in std::iter::once((database_path.to_path_buf(), quarantined.clone()))
        .chain(sidecars.iter().cloned())
    {
        if !original.exists() {
            continue;
        }
        if let Err(error) = fs::rename(&original, &target) {
            rollback_moves(&moved);
            let _ = fs::remove_file(&temporary);
            return Err(recovery_error(
                database_path,
                format!("quarantine {}: {error}", original.display()),
            ));
        }
        moved.push((original, target));
    }

    if let Err(error) = fs::rename(&temporary, database_path) {
        rollback_moves(&moved);
        let _ = fs::remove_file(&temporary);
        return Err(recovery_error(
            database_path,
            format!("install recovered database: {error}"),
        ));
    }

    if let Err(error) = validate_database_file(database_path) {
        let _ = fs::remove_file(database_path);
        rollback_moves(&moved);
        return Err(recovery_error(
            database_path,
            format!("recovered database is invalid: {error}"),
        ));
    }

    Ok(moved
        .iter()
        .any(|(original, _)| original == database_path)
        .then_some(quarantined))
}

fn rollback_moves(moved: &[(PathBuf, PathBuf)]) {
    for (original, target) in moved.iter().rev() {
        if target.exists() && !original.exists() {
            let _ = fs::rename(target, original);
        }
    }
}

fn remove_file_if_present(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

fn unique_sibling(path: &Path, suffix: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let base = sibling_with_suffix(path, &format!("{suffix}-{timestamp}"));
    if !base.exists() {
        return base;
    }

    for index in 1..1000u32 {
        let candidate = sibling_with_suffix(&base, &format!("-{index}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

fn recovery_error(database: &Path, error: impl std::fmt::Display) -> StorageError {
    StorageError::DatabaseRecoveryFailed {
        database: database.to_path_buf(),
        reason: error.to_string(),
    }
}

fn backup_error(database: &Path, error: impl std::fmt::Display) -> StorageError {
    StorageError::DatabaseBackupFailed {
        database: database.to_path_buf(),
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use crate::{
        domain::{ClipboardItem, ClipboardKind},
        storage::ClipboardRepository,
    };

    use super::{
        backup_path, discard_database_backups, discard_database_quarantine, previous_backup_path,
        quarantine_search_index, recover_database_if_needed, refresh_database_backup,
        reset_search_index,
    };
    use crate::storage::Database;

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "clipboard-database-recovery-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn item(id: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: id.to_owned(),
            text_content: Some(format!("content-{id}")),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: None,
            icon_path: None,
            size_bytes: 1,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    #[test]
    fn rotates_and_validates_database_backups() {
        let root = temporary_path("rotate");
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        database.save_item(&item("first")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        database.save_item(&item("second")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();

        assert!(backup_path(&database_path).is_file());
        assert!(previous_backup_path(&database_path).is_file());
        let backup = Database::open(backup_path(&database_path)).unwrap();
        assert!(backup.get_item("second").unwrap().is_some());
        let previous = Database::open(previous_backup_path(&database_path)).unwrap();
        assert!(previous.get_item("first").unwrap().is_some());

        drop(previous);
        drop(backup);
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    fn overlapping_refresh_preserves_generations(separate_connection: bool) {
        use std::sync::mpsc;
        use std::time::Duration;

        let root = temporary_path("overlapping-refresh");
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        database.save_item(&item("first")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        database.save_item(&item("second")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        database.save_item(&item("third")).unwrap();
        let other = separate_connection.then(|| Database::open(&database_path).unwrap());
        let (validated_tx, validated_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let first_result = std::thread::scope(|scope| {
            let database_ref = &database;
            let path_ref = &database_path;
            let first = scope.spawn(move || {
                super::refresh_database_backup_with_hook(database_ref, path_ref, || {
                    validated_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                })
            });
            validated_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let second_database = other.as_ref().unwrap_or(&database);
            second_database.save_item(&item("fourth")).unwrap();
            let second_result =
                refresh_database_backup(second_database, &root.join("./clipboard.sqlite3"));
            assert!(
                second_result.is_err(),
                "overlapping refresh should be refused before touching generations"
            );
            resume_tx.send(()).unwrap();
            first.join().unwrap()
        });
        assert!(
            previous_backup_path(&database_path).is_file(),
            "overlapping refresh discarded the previous recovery generation"
        );
        first_result.unwrap();
        let previous = Database::open(previous_backup_path(&database_path)).unwrap();
        assert!(previous.get_item("second").unwrap().is_some());
        assert!(previous.get_item("third").unwrap().is_none());
        let backup = Database::open(backup_path(&database_path)).unwrap();
        assert!(backup.get_item("third").unwrap().is_some());
        assert!(backup.get_item("fourth").unwrap().is_none());
        drop(backup);
        drop(previous);
        // Successful completion released ownership; a later refresh can install
        // the new row and still rotate the third generation into previous.
        refresh_database_backup(&database, &database_path).unwrap();
        let backup = Database::open(backup_path(&database_path)).unwrap();
        assert!(backup.get_item("fourth").unwrap().is_some());
        drop(backup);
        drop(other);
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn overlapping_refresh_on_one_database_preserves_both_generations() {
        overlapping_refresh_preserves_generations(false);
    }

    #[test]
    fn overlapping_refresh_across_connections_preserves_both_generations() {
        overlapping_refresh_preserves_generations(true);
    }

    #[test]
    fn backup_lock_errors_and_failed_rotation_preserve_generations_and_release_ownership() {
        let root = temporary_path("backup-lock-failures");
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        database.save_item(&item("first")).unwrap();
        let lock_path = super::sibling_with_suffix(&database_path, ".backup.lock");
        fs::create_dir(&lock_path).unwrap();
        assert!(refresh_database_backup(&database, &database_path).is_err());
        assert!(!super::sibling_with_suffix(&database_path, ".backup.tmp").exists());
        fs::remove_dir(&lock_path).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        let backup_before = fs::read(backup_path(&database_path)).unwrap();
        let previous = previous_backup_path(&database_path);
        fs::create_dir(&previous).unwrap();
        fs::write(previous.join("keep"), b"preserve").unwrap();
        assert!(refresh_database_backup(&database, &database_path).is_err());
        assert_eq!(
            fs::read(backup_path(&database_path)).unwrap(),
            backup_before
        );
        assert_eq!(fs::read(previous.join("keep")).unwrap(), b"preserve");
        // The failed refresh must release the file lock too.
        fs::remove_file(previous.join("keep")).unwrap();
        fs::remove_dir(previous).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        let other_path = root.join("other.sqlite3");
        let other = Database::open(&other_path).unwrap();
        super::refresh_database_backup_with_hook(&database, &database_path, || {
            refresh_database_backup(&other, &other_path).unwrap();
        })
        .unwrap();
        let detached_base = root.join("detached.sqlite3");
        refresh_database_backup(&database, &detached_base).unwrap();
        assert!(!detached_base.exists());
        let detached = Database::open(backup_path(&detached_base)).unwrap();
        assert!(detached.get_item("first").unwrap().is_some());
        drop(detached);
        drop(other);
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "child fixture invoked by the cross-process backup refresh test"]
    fn backup_refresh_child_fixture() {
        use std::time::{Duration, Instant};
        let root = PathBuf::from(std::env::var_os("CLIPBOARD_BACKUP_REFRESH_FIXTURE").unwrap());
        assert_eq!(
            root.parent().unwrap().canonicalize().unwrap(),
            std::env::temp_dir().canonicalize().unwrap()
        );
        assert_eq!(
            fs::read(root.join("fixture.marker")).unwrap(),
            b"isolated backup refresh fixture"
        );
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        super::refresh_database_backup_with_hook(&database, &database_path, || {
            fs::write(root.join("child.ready"), b"ready").unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !root.join("child.release").is_file() {
                assert!(
                    Instant::now() < deadline,
                    "parent did not release backup fixture"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        })
        .unwrap();
    }

    #[test]
    fn another_process_refresh_is_refused_without_touching_backup_generations() {
        use std::time::{Duration, Instant};
        let root = temporary_path("backup-process");
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        fs::write(
            root.join("fixture.marker"),
            b"isolated backup refresh fixture",
        )
        .unwrap();
        database.save_item(&item("first")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        let backup_before = fs::read(backup_path(&database_path)).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage::recovery::tests::backup_refresh_child_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("CLIPBOARD_BACKUP_REFRESH_FIXTURE", &root)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("child.ready").is_file() {
            assert!(
                child.try_wait().unwrap().is_none(),
                "backup fixture exited before validation"
            );
            assert!(Instant::now() < deadline, "backup fixture did not validate");
            std::thread::sleep(Duration::from_millis(10));
        }
        let overlapping = refresh_database_backup(&database, &database_path);
        let backup_during = fs::read(backup_path(&database_path)).unwrap();
        let previous_during = previous_backup_path(&database_path).exists();
        fs::write(root.join("child.release"), b"release").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(overlapping.is_err());
        assert_eq!(backup_during, backup_before);
        assert!(!previous_during);
        assert!(previous_backup_path(&database_path).is_file());
        refresh_database_backup(&database, &database_path).unwrap();
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema_reset_discards_all_database_backup_generations() {
        let root = temporary_path("discard-backups");
        fs::create_dir_all(&root).unwrap();
        let database = root.join("clipboard.sqlite3");
        let backup = backup_path(&database);
        let previous = previous_backup_path(&database);
        let temporary = super::sibling_with_suffix(&database, ".backup.tmp");
        for path in [&backup, &previous, &temporary] {
            fs::write(path, b"obsolete").unwrap();
        }

        discard_database_backups(&database).unwrap();

        assert!(!backup.exists());
        assert!(!previous.exists());
        assert!(!temporary.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema_reset_discards_the_obsolete_database_quarantine() {
        let root = temporary_path("discard-quarantine");
        fs::create_dir_all(&root).unwrap();
        let quarantine = root.join("clipboard.sqlite3.corrupt-1");
        let wal = super::sibling_with_suffix(&quarantine, "-wal");
        let shm = super::sibling_with_suffix(&quarantine, "-shm");
        for path in [&quarantine, &wal, &shm] {
            fs::write(path, b"obsolete").unwrap();
        }

        discard_database_quarantine(&quarantine).unwrap();

        assert!(!quarantine.exists());
        assert!(!wal.exists());
        assert!(!shm.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn read_only_backups_restore_to_a_writable_database_without_changing_the_backup() {
        for missing in [true, false] {
            let root = temporary_path("read-only-backup");
            let database_path = root.join("clipboard.sqlite3");
            let database = Database::open(&database_path).unwrap();
            database.save_item(&item("safe")).unwrap();
            refresh_database_backup(&database, &database_path).unwrap();
            drop(database);
            let backup = backup_path(&database_path);
            let backup_bytes = fs::read(&backup).unwrap();
            let permissions = fs::metadata(&backup).unwrap().permissions();
            let mut read_only = permissions.clone();
            read_only.set_readonly(true);
            fs::set_permissions(&backup, read_only).unwrap();
            if missing {
                fs::remove_file(&database_path).unwrap();
            } else {
                fs::write(&database_path, b"corrupt current database").unwrap();
            }
            let result = (|| -> Result<(), crate::storage::StorageError> {
                let report = recover_database_if_needed(&database_path)?;
                assert!(report.is_some());
                let restored = Database::open(&database_path)?;
                assert!(restored.get_item("safe")?.is_some());
                restored.save_item(&item("after-recovery"))?;
                Ok(())
            })();
            let unchanged = fs::read(&backup).unwrap() == backup_bytes;
            let still_read_only = fs::metadata(&backup).unwrap().permissions().readonly();
            // Clear only our fixture's attributes so even the pre-fix failure
            // removes read-only copied stages on Windows before asserting.
            for entry in fs::read_dir(&root).unwrap() {
                let path = entry.unwrap().path();
                if path.is_file() {
                    fs::set_permissions(path, permissions.clone()).unwrap();
                }
            }
            fs::remove_dir_all(&root).unwrap();
            assert!(
                result.is_ok(),
                "read-only backup recovery failed: {result:?}"
            );
            assert!(unchanged && still_read_only);
        }
    }

    #[test]
    fn restores_a_corrupt_database_and_keeps_a_quarantine_copy() {
        let root = temporary_path("restore");
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        database.save_item(&item("safe")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        drop(database);

        fs::write(&database_path, b"not a sqlite database").unwrap();
        let report = recover_database_if_needed(&database_path)
            .unwrap()
            .expect("corrupt database should be restored");
        assert!(report.restored_from.ends_with("clipboard.sqlite3.backup"));
        let quarantined_database = report.quarantined_database.unwrap();
        assert!(quarantined_database.is_file());
        assert_eq!(
            fs::read(quarantined_database).unwrap(),
            b"not a sqlite database"
        );

        let restored = Database::open(&database_path).unwrap();
        assert!(restored.get_item("safe").unwrap().is_some());
        drop(restored);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restores_when_the_database_file_is_missing() {
        let root = temporary_path("missing");
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        database.save_item(&item("safe")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        drop(database);
        fs::remove_file(&database_path).unwrap();

        let report = recover_database_if_needed(&database_path)
            .unwrap()
            .expect("missing database should be restored from backup");
        assert!(report.quarantined_database.is_none());
        let restored = Database::open(&database_path).unwrap();
        assert!(restored.get_item("safe").unwrap().is_some());
        drop(restored);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepts_a_valid_wal_database_without_sidecar_files() {
        let root = temporary_path("wal-sidecars");
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        database.save_item(&item("safe")).unwrap();
        drop(database);
        let _ = fs::remove_file(super::sibling_with_suffix(&database_path, "-wal"));
        let _ = fs::remove_file(super::sibling_with_suffix(&database_path, "-shm"));

        assert!(recover_database_if_needed(&database_path)
            .unwrap()
            .is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn falls_back_to_the_previous_backup_when_the_latest_is_invalid() {
        let root = temporary_path("fallback");
        let database_path = root.join("clipboard.sqlite3");
        let database = Database::open(&database_path).unwrap();
        database.save_item(&item("first")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        database.save_item(&item("second")).unwrap();
        refresh_database_backup(&database, &database_path).unwrap();
        drop(database);

        fs::write(backup_path(&database_path), b"invalid latest backup").unwrap();
        fs::write(&database_path, b"invalid current database").unwrap();
        let report = recover_database_if_needed(&database_path)
            .unwrap()
            .expect("previous backup should be used");
        assert!(report
            .restored_from
            .ends_with("clipboard.sqlite3.backup.prev"));
        let restored = Database::open(&database_path).unwrap();
        assert!(restored.get_item("first").unwrap().is_some());
        assert!(restored.get_item("second").unwrap().is_none());
        drop(restored);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quarantines_stale_search_index_after_database_restore() {
        let root = temporary_path("index");
        let index = root.join("search-index");
        fs::create_dir_all(&index).unwrap();
        fs::write(index.join("meta.json"), b"stale").unwrap();
        let quarantined = quarantine_search_index(&index).unwrap().unwrap();
        assert!(!index.join("meta.json").exists());
        assert_eq!(fs::read(quarantined.join("meta.json")).unwrap(), b"stale");
        assert!(index.is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema_reset_removes_derived_search_index_contents() {
        let root = temporary_path("reset-index");
        let index = root.join("search-index");
        fs::create_dir_all(&index).unwrap();
        fs::write(index.join("meta.json"), b"stale").unwrap();

        reset_search_index(&index).unwrap();

        assert!(index.is_dir());
        assert!(!index.join("meta.json").exists());
        assert_eq!(fs::read_dir(&index).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema_reset_does_not_follow_a_search_index_symlink() {
        let root = temporary_path("reset-index-symlink");
        let target = root.join("outside-index");
        let index = root.join("search-index");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), b"keep").unwrap();

        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_dir(&target, &index);
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&target, &index);

        if link_result.is_err() {
            fs::remove_dir_all(root).unwrap();
            return;
        }

        reset_search_index(&index).unwrap();

        assert!(target.join("keep.txt").is_file());
        assert!(index.is_dir());
        assert!(!fs::symlink_metadata(&index)
            .unwrap()
            .file_type()
            .is_symlink());
        fs::remove_dir_all(root).unwrap();
    }
}
