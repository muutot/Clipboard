//! Coordinates resource reuse/publication with orphan deletion across the
//! application's SQLite connections and processes, without locking writers
//! during a scan. File locks require Rust 1.89 or newer.
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, Weak},
};

#[derive(Default)]
struct State {
    writers: usize,
    generation: u64,
}

#[derive(Default)]
pub(crate) struct ResourcePublication {
    state: Mutex<State>,
    coordination: Option<CoordinationPaths>,
}

pub(crate) struct ResourceWriteGuard {
    publication: Arc<ResourcePublication>,
    _active_file: Option<File>,
}

pub(crate) struct ResourceCleanupSnapshot {
    publication: Arc<ResourcePublication>,
    generation: u64,
    external_generation: Option<u64>,
}

/// Writers share the active lock until DB publication finishes. A separate
/// counter detects writers that started and completed while cleanup scanned.
/// All locks use independently opened handles and release on close/crash.
struct CoordinationPaths {
    active: PathBuf,
    generation: PathBuf,
}

impl CoordinationPaths {
    fn new(database: &Path) -> io::Result<Self> {
        let sibling = |suffix: &str| {
            let mut name = database.as_os_str().to_os_string();
            name.push(suffix);
            PathBuf::from(name)
        };
        let paths = Self {
            active: sibling(".resource-writers.lock"),
            generation: sibling(".resource-generation"),
        };
        let active = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&paths.active)?;
        active.lock_shared()?;
        let mut generation = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&paths.generation)?;
        generation.lock()?;
        if generation.metadata()?.len() == 0 {
            generation.write_all(&0u64.to_le_bytes())?;
        } else {
            Self::read_counter(&mut generation)?;
        }
        Ok(paths)
    }

    fn read_counter(file: &mut File) -> io::Result<u64> {
        if file.metadata()?.len() != 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid resource publication counter",
            ));
        }
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = [0; 8];
        file.read_exact(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn begin_write(&self) -> io::Result<File> {
        let active = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.active)?;
        active.lock_shared()?;
        let mut counter = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.generation)?;
        counter.lock()?;
        let next = Self::read_counter(&mut counter)?
            .checked_add(1)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "resource publication counter exhausted",
                )
            })?;
        counter.seek(SeekFrom::Start(0))?;
        counter.write_all(&next.to_le_bytes())?;
        // Visibility between processes is required, not durability across a
        // reboot: no cleanup snapshot survives reboot. Avoid per-record fsync.
        Ok(active)
    }

    fn exclusive(&self) -> Option<File> {
        let result = OpenOptions::new().read(true).write(true).open(&self.active);
        let active = match result {
            Ok(active) => active,
            Err(error) => {
                crate::log_error!(
                    "[cleanup] resource coordination unavailable; deferring cleanup: {error}"
                );
                return None;
            }
        };
        match active.try_lock() {
            Ok(()) => Some(active),
            Err(std::fs::TryLockError::WouldBlock) => None,
            Err(error) => {
                crate::log_error!(
                    "[cleanup] resource coordination unavailable; deferring cleanup: {error}"
                );
                None
            }
        }
    }

    fn generation(&self) -> Option<u64> {
        let result = (|| {
            let mut counter = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.generation)?;
            counter.lock_shared()?;
            Self::read_counter(&mut counter)
        })();
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                crate::log_error!(
                    "[cleanup] resource coordination unavailable; deferring cleanup: {error}"
                );
                None
            }
        }
    }
}

impl ResourcePublication {
    pub(crate) fn for_database(path: Option<&str>) -> std::io::Result<Arc<Self>> {
        static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Weak<ResourcePublication>>>> =
            OnceLock::new();
        let Some(path) = path.filter(|path| !path.is_empty()) else {
            return Ok(Arc::default());
        };
        let path = std::fs::canonicalize(path)?;
        let mut registry = REGISTRY
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        registry.retain(|_, value| value.strong_count() > 0);
        if let Some(publication) = registry.get(&path).and_then(Weak::upgrade) {
            return Ok(publication);
        }
        let publication = Arc::new(Self {
            state: Mutex::default(),
            coordination: Some(CoordinationPaths::new(&path)?),
        });
        registry.insert(path, Arc::downgrade(&publication));
        Ok(publication)
    }

    pub(crate) fn begin_write(self: &Arc<Self>) -> io::Result<ResourceWriteGuard> {
        let active_file = self
            .coordination
            .as_ref()
            .map(CoordinationPaths::begin_write)
            .transpose()?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.writers += 1;
        state.generation = state.generation.wrapping_add(1);
        Ok(ResourceWriteGuard {
            publication: self.clone(),
            _active_file: active_file,
        })
    }

    pub(crate) fn snapshot(self: &Arc<Self>) -> Option<ResourceCleanupSnapshot> {
        let (external_generation, _exclusive) = match &self.coordination {
            Some(paths) => {
                let exclusive = paths.exclusive()?;
                (Some(paths.generation()?), Some(exclusive))
            }
            None => (None, None),
        };
        let state = self.state.lock().ok()?;
        (state.writers == 0).then(|| ResourceCleanupSnapshot {
            publication: self.clone(),
            generation: state.generation,
            external_generation,
        })
    }
}

impl Drop for ResourceWriteGuard {
    fn drop(&mut self) {
        self.publication
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .writers -= 1;
    }
}

impl ResourceCleanupSnapshot {
    /// Serialize just one unlink against the start of publication. No database
    /// query, reference resolution, or directory scan may run inside `remove`.
    pub(crate) fn remove_if_current<T>(&self, remove: impl FnOnce() -> T) -> Option<T> {
        let _exclusive = match &self.publication.coordination {
            Some(paths) => {
                let exclusive = paths.exclusive()?;
                if Some(paths.generation()?) != self.external_generation {
                    return None;
                }
                Some(exclusive)
            }
            None => None,
        };
        let state = self.publication.state.lock().ok()?;
        if state.writers != 0 || state.generation != self.generation {
            return None;
        }
        let result = remove();
        drop(state);
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "clipboard-publication-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn overlapping_writers_invalidate_snapshots_even_after_they_finish() {
        let publication = Arc::new(ResourcePublication::default());
        let snapshot = publication.snapshot().unwrap();
        let first = publication.begin_write().unwrap();
        let second = publication.begin_write().unwrap();
        assert!(publication.snapshot().is_none());
        assert!(snapshot
            .remove_if_current(|| panic!("must not unlink"))
            .is_none());
        drop(first);
        assert!(publication.snapshot().is_none());
        drop(second);
        assert!(snapshot
            .remove_if_current(|| panic!("stale snapshot"))
            .is_none());
        assert_eq!(
            publication.snapshot().unwrap().remove_if_current(|| 42),
            Some(42)
        );
    }

    #[test]
    fn independent_file_coordinators_allow_nested_writes_and_fence_old_snapshots() {
        let temporary = temporary_directory();
        let database = temporary.join("history.db");
        let coordinator = || {
            Arc::new(ResourcePublication {
                state: Mutex::default(),
                coordination: Some(CoordinationPaths::new(&database).unwrap()),
            })
        };
        // Deliberately bypass the process-local registry: protection must come
        // from the file locks/counter, not the shared Arc or local generation.
        let reader = coordinator();
        let writer = coordinator();
        let snapshot = reader.snapshot().unwrap();
        let first = writer.begin_write().unwrap();
        let second = writer.begin_write().unwrap();
        assert!(reader.snapshot().is_none());
        drop(first);
        assert!(reader.snapshot().is_none());
        drop(second);
        assert!(snapshot
            .remove_if_current(|| panic!("stale snapshot"))
            .is_none());
        assert_eq!(
            reader.snapshot().unwrap().remove_if_current(|| 42),
            Some(42)
        );
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn broken_coordination_rejects_publication_and_defers_cleanup() {
        let temporary = temporary_directory();
        let publication = Arc::new(ResourcePublication {
            state: Mutex::default(),
            coordination: Some(CoordinationPaths::new(&temporary.join("history.db")).unwrap()),
        });
        let paths = publication.coordination.as_ref().unwrap();
        let snapshot = publication.snapshot().unwrap();
        std::fs::write(&paths.generation, b"bad").unwrap();
        assert_eq!(
            publication.begin_write().err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(publication.snapshot().is_none());
        assert!(snapshot
            .remove_if_current(|| panic!("invalid counter"))
            .is_none());
        assert_eq!(std::fs::read(&paths.generation).unwrap(), b"bad");
        std::fs::write(&paths.generation, u64::MAX.to_le_bytes()).unwrap();
        assert!(publication.begin_write().is_err());
        std::fs::remove_file(&paths.active).unwrap();
        assert!(publication.begin_write().is_err());
        assert!(publication.snapshot().is_none());
        assert!(snapshot
            .remove_if_current(|| panic!("missing coordination"))
            .is_none());
        std::fs::remove_dir_all(temporary).unwrap();
    }
}
