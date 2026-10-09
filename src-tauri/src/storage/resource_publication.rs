//! Coordinates resource reuse/publication with orphan deletion across the
//! application's SQLite connections, without locking writers during a scan.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, Weak},
};

#[derive(Default)]
struct State {
    writers: usize,
    generation: u64,
}

#[derive(Default)]
pub(crate) struct ResourcePublication(Mutex<State>);

pub(crate) struct ResourceWriteGuard(Arc<ResourcePublication>);

pub(crate) struct ResourceCleanupSnapshot {
    publication: Arc<ResourcePublication>,
    generation: u64,
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
        let publication = Arc::new(Self::default());
        registry.insert(path, Arc::downgrade(&publication));
        Ok(publication)
    }

    pub(crate) fn begin_write(self: &Arc<Self>) -> ResourceWriteGuard {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.writers += 1;
        state.generation = state.generation.wrapping_add(1);
        ResourceWriteGuard(self.clone())
    }

    pub(crate) fn snapshot(self: &Arc<Self>) -> Option<ResourceCleanupSnapshot> {
        let state = self.0.lock().ok()?;
        (state.writers == 0).then(|| ResourceCleanupSnapshot {
            publication: self.clone(),
            generation: state.generation,
        })
    }
}

impl Drop for ResourceWriteGuard {
    fn drop(&mut self) {
        self.0 .0.lock().unwrap_or_else(|e| e.into_inner()).writers -= 1;
    }
}

impl ResourceCleanupSnapshot {
    /// Serialize just one unlink against the start of publication. No database
    /// query, reference resolution, or directory scan may run inside `remove`.
    pub(crate) fn remove_if_current<T>(&self, remove: impl FnOnce() -> T) -> Option<T> {
        let state = self.publication.0.lock().ok()?;
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

    #[test]
    fn overlapping_writers_invalidate_snapshots_even_after_they_finish() {
        let publication = Arc::new(ResourcePublication::default());
        let snapshot = publication.snapshot().unwrap();
        let first = publication.begin_write();
        let second = publication.begin_write();
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
}
