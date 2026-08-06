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
    fs::copy(backup, &temporary).map_err(|error| recovery_error(database_path, error))?;
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
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
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
        backup_path, previous_backup_path, quarantine_search_index, recover_database_if_needed,
        refresh_database_backup,
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
}
