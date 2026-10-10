use super::{
    quarantine_database_for_migration, replace_migration_database, restore_moved_files,
    restore_quarantined_database,
};
use std::time::SystemTime;

#[test]
fn migration_copy_preserves_existing_destination_files() {
    let root =
        std::env::temp_dir().join(format!("clipboard-copy-conflict-{}", uuid::Uuid::new_v4()));
    let source = root.join("source");
    let destination = root.join("destination");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(source.join("renamed.txt"), b"incoming content").unwrap();
    std::fs::write(destination.join("renamed.txt"), b"existing content").unwrap();
    let result = super::copy_dir_contents(&source, &destination);
    let contents = std::fs::read(destination.join("renamed.txt")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(contents, b"existing content");
    assert!(result.is_err());
}

#[test]
fn migration_copy_allows_identical_retries_and_rejects_nested_directories() {
    let root = std::env::temp_dir().join(format!("clipboard-copy-retry-{}", uuid::Uuid::new_v4()));
    let source = root.join("source");
    let target = root.join("target");
    std::fs::create_dir_all(source.join("child")).unwrap();
    let bytes = vec![37; 32_001];
    std::fs::write(source.join("child/item.bin"), &bytes).unwrap();
    super::copy_dir_contents(&source, &target).unwrap();
    super::copy_dir_contents(&source, &target).unwrap();
    assert_eq!(std::fs::read(target.join("child/item.bin")).unwrap(), bytes);
    assert!(super::copy_dir_contents(&source, &source.join("nested")).is_err());
    assert!(!source.join("nested/nested").exists());
    assert!(super::copy_dir_contents(&source.join("child"), &source).is_err());
    super::copy_dir_contents(&source, &source.join(".")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn migration_blocks_independent_writers_through_save_and_until_restart() {
    use crate::storage::{Database, StoragePaths};
    let root = std::env::temp_dir().join(format!(
        "clipboard-migration-writers-{}",
        uuid::Uuid::new_v4()
    ));
    let old = StoragePaths::initialize(root.join("old")).unwrap();
    let new = StoragePaths::initialize(root.join("new")).unwrap();
    let database = Database::open(&old.database).unwrap();
    let writer = rusqlite::Connection::open(&old.database).unwrap();
    writer.busy_timeout(std::time::Duration::ZERO).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE audit_migration (id INTEGER); INSERT INTO audit_migration VALUES (1)",
        )
        .unwrap();

    let result = super::migration::migrate_and_save_storage(&old, &new, &database, || {
        assert!(
            writer
                .execute("INSERT INTO audit_migration VALUES (2)", [])
                .is_err(),
            "writes after the snapshot would be absent from the migrated database"
        );
        Ok(())
    });
    result.unwrap();
    assert!(writer
        .execute("INSERT INTO audit_migration VALUES (3)", [])
        .is_err());
    let snapshot = rusqlite::Connection::open(&new.database).unwrap();
    let count: i64 = snapshot
        .query_row("SELECT COUNT(*) FROM audit_migration", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    drop(database);
    writer
        .execute("INSERT INTO audit_migration VALUES (4)", [])
        .unwrap();
    drop(writer);
    drop(snapshot);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_migration_or_config_save_releases_the_writer_reservation() {
    use crate::storage::{Database, StoragePaths};
    for fail_copy in [true, false] {
        let root = std::env::temp_dir().join(format!(
            "clipboard-migration-release-{}",
            uuid::Uuid::new_v4()
        ));
        let old = StoragePaths::initialize(root.join("old")).unwrap();
        let new = StoragePaths::initialize(root.join("new")).unwrap();
        let database = Database::open(&old.database).unwrap();
        let writer = rusqlite::Connection::open(&old.database).unwrap();
        writer.busy_timeout(std::time::Duration::ZERO).unwrap();
        if fail_copy {
            std::fs::write(old.images.join("collision"), b"image bytes").unwrap();
            std::fs::create_dir(new.images.join("collision")).unwrap();
        }
        let mut saved = false;
        let result = super::migration::migrate_and_save_storage(&old, &new, &database, || {
            saved = true;
            Err("injected config save failure".to_owned())
        });
        assert!(result.is_err());
        assert_eq!(saved, !fail_copy);
        writer
            .execute(
                "INSERT INTO sync_metadata(key, value) VALUES ('audit-after-failure', '1')",
                [],
            )
            .unwrap();
        // Failure must not leave the app permanently in migration state.
        drop(database.begin_storage_migration().unwrap());
        drop(writer);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn quarantine_moves_existing_database_and_sidecars_aside() {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "clipboard-migration-quarantine-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("clipboard.sqlite3");
    let wal = directory.join("clipboard.sqlite3-wal");
    std::fs::write(&database, b"db").unwrap();
    std::fs::write(&wal, b"wal").unwrap();

    let moved = quarantine_database_for_migration(&database).unwrap();
    assert_eq!(moved.len(), 2);
    assert!(!database.exists());
    assert!(!wal.exists());

    restore_quarantined_database(&database, &moved).unwrap();
    assert_eq!(std::fs::read(&database).unwrap(), b"db");
    assert_eq!(std::fs::read(&wal).unwrap(), b"wal");

    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(target_os = "windows")]
#[test]
fn quarantine_failure_restores_the_already_moved_database() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = std::env::temp_dir().join(format!(
        "clipboard-migration-locked-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("clipboard.sqlite3");
    let wal = directory.join("clipboard.sqlite3-wal");
    std::fs::write(&database, b"original-db").unwrap();
    std::fs::write(&wal, b"original-wal").unwrap();
    let locked_wal = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&wal)
        .unwrap();

    let result = quarantine_database_for_migration(&database);
    drop(locked_wal);
    let restored = std::fs::read(&database);
    std::fs::remove_dir_all(directory).unwrap();
    assert!(result.is_err());
    assert_eq!(restored.unwrap(), b"original-db");
}

#[test]
fn restore_preserves_an_incomplete_snapshot_and_recovers_originals() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-migration-restore-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("clipboard.sqlite3");
    std::fs::write(&database, b"original-db").unwrap();
    let moved = quarantine_database_for_migration(&database).unwrap();
    // VACUUM INTO can leave an incomplete destination on I/O failure.
    std::fs::write(&database, b"incomplete-snapshot").unwrap();
    restore_quarantined_database(&database, &moved).unwrap();
    let restored = std::fs::read(&database).unwrap();
    let incomplete_preserved = std::fs::read_dir(&directory).unwrap().any(|entry| {
        entry.ok().is_some_and(|entry| {
            std::fs::read(entry.path().join("clipboard.sqlite3"))
                .is_ok_and(|bytes| bytes == b"incomplete-snapshot")
        })
    });
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(restored, b"original-db");
    assert!(incomplete_preserved);
}

#[test]
fn failed_replacement_restores_database_without_mixing_new_sidecars() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-migration-failed-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("clipboard.sqlite3");
    let wal = directory.join("clipboard.sqlite3-wal");
    std::fs::write(&database, b"original-db").unwrap();
    let result = replace_migration_database(&database, || {
        std::fs::write(&database, b"new-db").unwrap();
        std::fs::write(&wal, b"new-wal").unwrap();
        Err("path rewrite failed".to_owned())
    });
    assert_eq!(result.unwrap_err(), "path rewrite failed");
    assert_eq!(std::fs::read(&database).unwrap(), b"original-db");
    assert!(!wal.exists());
    let backups = std::fs::read_dir(&directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read(backups[0].path().join("clipboard.sqlite3")).unwrap(),
        b"new-db"
    );
    assert_eq!(
        std::fs::read(backups[0].path().join("clipboard.sqlite3-wal")).unwrap(),
        b"new-wal"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rollback_reports_conflict_and_retains_both_original_backups() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-migration-conflict-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("clipboard.sqlite3");
    let wal = directory.join("clipboard.sqlite3-wal");
    std::fs::write(&database, b"original-db").unwrap();
    std::fs::write(&wal, b"original-wal").unwrap();
    let moved = quarantine_database_for_migration(&database).unwrap();
    std::fs::write(&database, b"conflicting-db").unwrap();
    let error = restore_moved_files(&moved).unwrap_err();
    assert!(error.contains("rollback destination already exists"));
    assert!(error.contains(&moved[0].0.display().to_string()));
    assert_eq!(std::fs::read(&database).unwrap(), b"conflicting-db");
    assert!(!wal.exists());
    assert_eq!(std::fs::read(&moved[0].0).unwrap(), b"original-db");
    assert_eq!(std::fs::read(&moved[1].0).unwrap(), b"original-wal");
    std::fs::remove_dir_all(directory).unwrap();
}
