//! Serialize a database's complete sync pass across connections and processes.
//! Persistent OS lock files release on close/crash and are never removed by a run.

use std::{fs, io, path::Path, sync::MutexGuard};

use super::{Database, StorageError};

pub(crate) struct StorageSyncRunLease<'a> {
    _local: MutexGuard<'a, ()>,
    _file: Option<fs::File>,
}

impl crate::sync::v1::SyncRunLease for StorageSyncRunLease<'_> {}

fn busy() -> StorageError {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "sync or storage migration is already in progress for this database",
    )
    .into()
}

pub(super) fn lock_database(database: &Path) -> Result<fs::File, StorageError> {
    let database = fs::canonicalize(database)?;
    let mut name = database.as_os_str().to_os_string();
    name.push(".sync-run.lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(Path::new(&name))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(busy()),
        Err(fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

impl Database {
    pub(crate) fn begin_sync_run(&self) -> Result<StorageSyncRunLease<'_>, StorageError> {
        let local = match self.sync_run.try_lock() {
            Ok(local) => local,
            Err(std::sync::TryLockError::WouldBlock) => return Err(busy()),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(StorageError::ConnectionPoisoned)
            }
        };
        let file = self.with_connection(|connection| {
            connection
                .path()
                .filter(|path| !path.is_empty())
                .map(|path| lock_database(Path::new(path)))
                .transpose()
        })?;
        Ok(StorageSyncRunLease {
            _local: local,
            _file: file,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        process::Command,
        time::{Duration, Instant},
    };

    fn root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("clipboard-sync-run-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("fixture.marker"), b"isolated sync-run fixture").unwrap();
        root
    }

    #[test]
    fn run_ownership_is_exclusive_for_connections_and_memory_databases() {
        let root = root();
        let first = Database::open(root.join("history.db")).unwrap();
        let second = Database::open(root.join("./history.db")).unwrap();
        let other = Database::open(root.join("other.db")).unwrap();
        let memory = Database::open_in_memory().unwrap();
        let guard = first.begin_sync_run().unwrap();
        assert!(first.begin_sync_run().is_err());
        assert!(second.begin_sync_run().is_err());
        drop(other.begin_sync_run().unwrap());
        let memory_guard = memory.begin_sync_run().unwrap();
        assert!(memory.begin_sync_run().is_err());
        drop(memory_guard);
        drop(memory.begin_sync_run().unwrap());
        drop(guard);
        drop(second.begin_sync_run().unwrap());
        drop(first);
        drop(second);
        drop(other);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_and_sync_share_ownership_until_failure_or_restart() {
        let root = root();
        let first = Database::open(root.join("history.db")).unwrap();
        let second = Database::open(root.join("history.db")).unwrap();
        let sync = first.begin_sync_run().unwrap();
        assert!(second.begin_storage_migration().is_err());
        drop(sync);
        let migration = first.begin_storage_migration().unwrap();
        assert!(second.begin_sync_run().is_err());
        drop(migration); // Failed migration releases both SQLite and OS reservations.
        drop(second.begin_sync_run().unwrap());
        first
            .begin_storage_migration()
            .unwrap()
            .keep_until_restart();
        assert!(second.begin_sync_run().is_err());
        drop(first);
        drop(second.begin_sync_run().unwrap());
        drop(second);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unavailable_coordination_rejects_run_ownership_and_recovers() {
        use crate::sync::v1::SyncRepository;
        let root = root();
        let path = root.join("history.db");
        let database = Database::open(&path).unwrap();
        fs::create_dir(root.join("history.db.sync-run.lock")).unwrap();
        assert!(database.acquire_sync_run().is_err());
        fs::remove_dir(root.join("history.db.sync-run.lock")).unwrap();
        drop(database.acquire_sync_run().unwrap());
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "child fixture invoked by the normal cross-process sync ownership test"]
    fn sync_run_child_fixture() {
        let root = PathBuf::from(std::env::var_os("CLIPBOARD_SYNC_RUN_FIXTURE").unwrap());
        assert_eq!(
            root.parent().unwrap().canonicalize().unwrap(),
            std::env::temp_dir().canonicalize().unwrap()
        );
        assert_eq!(
            fs::read(root.join("fixture.marker")).unwrap(),
            b"isolated sync-run fixture"
        );
        let database = Database::open(root.join("history.db")).unwrap();
        let _lease = database.begin_sync_run().unwrap();
        fs::write(root.join("child.ready"), b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(15));
    }

    struct ChildFixture(std::process::Child);
    impl Drop for ChildFixture {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn another_process_run_blocks_sync_and_migration_and_releases_on_crash() {
        let root = root();
        let database = Database::open(root.join("history.db")).unwrap();
        let mut child = ChildFixture(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage::sync_run::tests::sync_run_child_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("CLIPBOARD_SYNC_RUN_FIXTURE", &root)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("child.ready").is_file() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before acquiring ownership"
            );
            assert!(Instant::now() < deadline, "child did not acquire ownership");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(database.begin_sync_run().is_err());
        assert!(database.begin_storage_migration().is_err());
        drop(child); // Kill only this test fixture, releasing OS locks without Drop.
        drop(database.begin_sync_run().unwrap());
        drop(database.begin_storage_migration().unwrap());
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }
}
