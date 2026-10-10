//! Data migration: copy helpers, size accounting, and the database swap that
//! backs `migrate_storage_data`.

use std::path::{Path, PathBuf};

use crate::storage::{Database, StoragePaths};

use super::paths::{rewrite_database_storage_paths, storage_path_mappings};

pub fn copy_dir_contents(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| format!("create migration directory: {error}"))?;
    let source = std::fs::canonicalize(from).map_err(|error| error.to_string())?;
    let destination = std::fs::canonicalize(to).map_err(|error| error.to_string())?;
    if source == destination {
        return Ok(());
    }
    if source.starts_with(&destination) || destination.starts_with(&source) {
        return Err("migration source and destination directories must not overlap".to_owned());
    }
    let mut ancestors = std::collections::HashSet::new();
    copy_dir_contents_inner(from, to, &mut ancestors)
}

fn copy_dir_contents_inner(
    from: &Path,
    to: &Path,
    ancestors: &mut std::collections::HashSet<PathBuf>,
) -> Result<(), String> {
    // A junction/reparse point reports as a directory and can point at an
    // ancestor, so a self-referential link would recurse until the stack
    // overflows. Track the canonical path of the current chain; re-entering an
    // ancestor is a cycle, not another directory to copy.
    let canonical = std::fs::canonicalize(from).unwrap_or_else(|_| from.to_path_buf());
    if !ancestors.insert(canonical.clone()) {
        return Err(format!(
            "refusing to copy a directory cycle at {}",
            from.display()
        ));
    }
    let result = copy_dir_contents_children(from, to, ancestors);
    ancestors.remove(&canonical);
    result
}

fn copy_dir_contents_children(
    from: &Path,
    to: &Path,
    ancestors: &mut std::collections::HashSet<PathBuf>,
) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("create dir: {}", e))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("read dir: {}", e))? {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let dest = to.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|e| format!("read file type for {}: {e}", entry.path().display()))?
            .is_dir()
        {
            copy_dir_contents_inner(&entry.path(), &dest, ancestors)?;
        } else {
            copy_migration_file(&entry.path(), &dest)?;
        }
    }
    Ok(())
}

fn identical_file_contents(from: &Path, to: &Path) -> std::io::Result<bool> {
    use std::io::{BufRead, BufReader};
    let mut source = BufReader::new(std::fs::File::open(from)?);
    let mut target = BufReader::new(std::fs::File::open(to)?);
    if source.get_ref().metadata()?.len() != target.get_ref().metadata()?.len() {
        return Ok(false);
    }
    loop {
        let left = source.fill_buf()?;
        let right = target.fill_buf()?;
        let count = left.len().min(right.len());
        if count == 0 {
            return Ok(left.is_empty() && right.is_empty());
        }
        if left[..count] != right[..count] {
            return Ok(false);
        }
        source.consume(count);
        target.consume(count);
    }
}

fn copy_migration_file(from: &Path, to: &Path) -> Result<(), String> {
    let mut source = std::fs::File::open(from).map_err(|error| error.to_string())?;
    let permissions = source
        .metadata()
        .map_err(|error| error.to_string())?
        .permissions();
    let mut target = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
    {
        Ok(target) => target,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return if identical_file_contents(from, to).map_err(|error| error.to_string())? {
                Ok(())
            } else {
                Err(format!(
                    "migration destination already contains different content: {}",
                    to.display()
                ))
            };
        }
        Err(error) => return Err(format!("create {}: {error}", to.display())),
    };
    // Only a file created by this attempt may be removed on failure. Existing
    // resources are either byte-identical retry results or explicit conflicts.
    let copied = std::io::copy(&mut source, &mut target)
        .and_then(|_| target.sync_all())
        .and_then(|_| target.set_permissions(permissions));
    drop(target);
    if let Err(error) = copied {
        return match std::fs::remove_file(to) {
            Ok(()) => Err(format!("copy {} to {}: {error}", from.display(), to.display())),
            Err(cleanup) => Err(format!("copy {} failed: {error}; incomplete destination {} could not be removed: {cleanup}", from.display(), to.display())),
        };
    }
    Ok(())
}

pub fn file_or_dir_size(path: &PathBuf) -> u64 {
    if path.is_file() {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let wal = path.with_extension("sqlite3-wal");
        let shm = path.with_extension("sqlite3-shm");
        let wal_size = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        let shm_size = std::fs::metadata(&shm).map(|m| m.len()).unwrap_or(0);
        return size + wal_size + shm_size;
    }
    dir_size(path)
}

pub fn dir_size(path: &Path) -> u64 {
    let mut ancestors = std::collections::HashSet::new();
    dir_size_inner(path, &mut ancestors)
}

fn dir_size_inner(path: &Path, ancestors: &mut std::collections::HashSet<PathBuf>) -> u64 {
    // Same cycle guard as the copy walk: never descend back into an ancestor.
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !ancestors.insert(canonical.clone()) {
        return 0;
    }
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                total += dir_size_inner(&entry.path(), ancestors);
            } else {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    ancestors.remove(&canonical);
    total
}

pub fn migrate_storage_data(
    old: &StoragePaths,
    new: &StoragePaths,
    database: &Database,
) -> Result<(), String> {
    let dirs_to_migrate: &[(PathBuf, PathBuf, &str)] = &[
        (old.images.clone(), new.images.clone(), "images"),
        (old.files.clone(), new.files.clone(), "files"),
    ];

    for (old_dir, new_dir, label) in dirs_to_migrate {
        if old_dir == new_dir {
            continue;
        }
        if old_dir.exists() {
            copy_dir_contents(old_dir, new_dir)
                .map_err(|e| format!("failed to migrate {}: {}", label, e))?;
        }
    }

    if old.search_index != new.search_index {
        std::fs::create_dir_all(&new.search_index)
            .map_err(|e| format!("failed to create search index directory: {}", e))?;
    }

    let icons_old = old.storage.join("icons");
    let icons_new = new.storage.join("icons");
    if icons_old != icons_new && icons_old.exists() {
        copy_dir_contents(&icons_old, &icons_new)
            .map_err(|e| format!("failed to migrate icons: {}", e))?;
    }

    if old.database != new.database && old.database.exists() {
        // `VACUUM INTO` refuses an existing destination, so a failed earlier
        // attempt (or a pre-existing database at the chosen directory) would
        // make every retry fail. Quarantine whatever is there first, restore it
        // if snapshot creation or validation fails, and leave the copy behind
        // otherwise instead of destroying a pre-existing database.
        replace_migration_database(&new.database, || {
            database
                .snapshot_into(&new.database)
                .map_err(|error| format!("failed to migrate database: {error}"))?;
            let migrated_database = Database::open(&new.database)
                .map_err(|e| format!("failed to open migrated database: {e}"))?;
            rewrite_database_storage_paths(&migrated_database, &storage_path_mappings(old, new))
                .map_err(|e| format!("failed to update migrated resource paths: {e}"))?;
            Ok(())
        })?;
    }

    Ok(())
}

pub(super) fn migrate_and_save_storage(
    old: &StoragePaths,
    new: &StoragePaths,
    database: &Database,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let guard = database
        .begin_storage_migration()
        .map_err(|error| error.to_string())?;
    migrate_storage_data(old, new, database)?;
    save()?;
    guard.keep_until_restart();
    Ok(())
}

/// Keep the old database until the snapshot is opened and its paths rewritten.
/// Close any SQLite handle in `replace` before returning so rollback can move it.
pub(super) fn replace_migration_database(
    path: &Path,
    replace: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let quarantined = quarantine_database_for_migration(path)?;
    if let Err(error) = replace() {
        return match restore_quarantined_database(path, &quarantined) {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}; migration rollback failed: {rollback}")),
        };
    }
    Ok(())
}

/// A newly reserved directory keeps database/WAL/SHM backups together and
/// prevents a rapid retry from replacing a previous migration's backup.
pub(super) fn quarantine_database_for_migration(
    path: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let candidates = [
        path.to_path_buf(),
        migration_sibling(path, "-wal"),
        migration_sibling(path, "-shm"),
    ];
    let mut present = Vec::new();
    for candidate in candidates {
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => present.push(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to inspect {}: {error}",
                    candidate.display()
                ))
            }
        }
    }
    if present.is_empty() {
        return Ok(Vec::new());
    }
    let directory = loop {
        let directory = migration_sibling(path, &format!(".pre-migrate-{}", uuid::Uuid::new_v4()));
        match std::fs::create_dir(&directory) {
            Ok(()) => break directory,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "failed to reserve {}: {error}",
                    directory.display()
                ))
            }
        }
    };
    let mut moved = Vec::new();
    for candidate in present {
        let backup = directory.join(
            candidate
                .file_name()
                .ok_or_else(|| format!("invalid database path: {}", candidate.display()))?,
        );
        if let Err(error) = std::fs::rename(&candidate, &backup) {
            let rollback = restore_moved_files(&moved);
            let _ = std::fs::remove_dir(&directory);
            return Err(format!(
                "failed to quarantine {}: {error}; rollback: {}",
                candidate.display(),
                rollback.err().unwrap_or_else(|| "completed".to_owned())
            ));
        }
        moved.push((backup, candidate));
    }
    Ok(moved)
}

fn migration_sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

pub(super) fn restore_quarantined_database(
    path: &Path,
    moved: &[(PathBuf, PathBuf)],
) -> Result<(), String> {
    // A failed vacuum/open/rewrite may have left a DB and new WAL sidecars.
    // Preserve the whole failed bundle before restoring any old file.
    quarantine_database_for_migration(path).map_err(|error| {
        format!("could not preserve failed snapshot: {error}; original backups: {moved:?}")
    })?;
    restore_moved_files(moved)
}

pub(super) fn restore_moved_files(moved: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    for (backup, original) in moved {
        // Refuse a conflicting destination and stop before restoring sidecars
        // beside a database that could not itself be restored.
        let result = match std::fs::symlink_metadata(original) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::rename(backup, original)
            }
            Err(error) => Err(error),
            Ok(_) => Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "rollback destination already exists",
            )),
        };
        if let Err(error) = result {
            return Err(format!(
                "restore {} from {}: {error}; remaining backups: {moved:?}",
                original.display(),
                backup.display()
            ));
        }
    }
    if let Some(directory) = moved.first().and_then(|(backup, _)| backup.parent()) {
        let _ = std::fs::remove_dir(directory);
    }
    Ok(())
}
