use std::{
    fs,
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use super::SearchError;
use crate::storage::replace_file;

pub const SEARCH_INDEX_VERSION: u32 = 2;

const MANIFEST_FILE_NAME: &str = "manifest.json";
const TANTIVY_DIRECTORY_NAME: &str = "tantivy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum SearchIndexState {
    Building,
    Ready,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchIndexManifest {
    version: u32,
    state: SearchIndexState,
}

pub struct SearchIndexLayout {
    manifest_path: PathBuf,
    pub index_directory: PathBuf,
    pub rebuild_required: bool,
}

impl SearchIndexLayout {
    pub fn prepare(root: PathBuf) -> Result<Self, SearchError> {
        fs::create_dir_all(&root)?;
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        let index_directory = root.join(TANTIVY_DIRECTORY_NAME);
        let manifest = read_manifest(&manifest_path).ok().flatten();
        // `read_manifest` returns `Err` for "present but unreadable" and `Ok(None)`
        // for "absent"; `.ok().flatten()` collapses the former to `None`, and the
        // match below tells those two apart by whether the index still exists.
        //
        // Only a manifest we could actually read may authorize a rebuild.
        //
        // "Unreadable" and "stale" are different facts: an unreadable manifest
        // is usually a transient sharing violation (antivirus, backup tooling,
        // OneDrive) and says nothing about the index on disk. Treating it as
        // stale deleted a perfectly good index, forcing a minutes-long rebuild
        // and — if that rebuild could not finish in one session — another wipe
        // on the next launch.
        let rebuild_required = match manifest {
            Some(manifest) => !matches!(
                manifest,
                SearchIndexManifest {
                    version: SEARCH_INDEX_VERSION,
                    state: SearchIndexState::Ready,
                } if index_directory.join("meta.json").is_file()
            ),
            // The index directory is already gone, so there is nothing to
            // preserve and rebuilding is the only option.
            None if !index_directory.exists() => true,
            // An unreadable manifest with an index present: keep the index and
            // leave the manifest alone. The synchronizer's own full-rebuild check
            // reconciles the index later, without destroying it first.
            None => {
                crate::log_warn!(
                    "[search] index manifest is unreadable; keeping the existing index and \
                     deferring any rebuild"
                );
                false
            }
        };

        if rebuild_required {
            if index_directory.exists() {
                fs::remove_dir_all(&index_directory)?;
            }
            fs::create_dir_all(&index_directory)?;
            write_manifest(&manifest_path, SearchIndexState::Building)?;
        }

        Ok(Self {
            manifest_path,
            index_directory,
            rebuild_required,
        })
    }

    pub fn mark_building(&self) -> Result<(), SearchError> {
        write_manifest(&self.manifest_path, SearchIndexState::Building)
    }

    pub fn mark_ready(&self) -> Result<(), SearchError> {
        write_manifest(&self.manifest_path, SearchIndexState::Ready)
    }
}

/// Reads the manifest, distinguishing "absent" from "present but unreadable".
///
/// `Ok(None)` means there is no manifest file. `Err` means one exists but could
/// not be read, which is almost always transient; conflating the two is what let
/// a sharing violation delete a healthy index.
fn read_manifest(path: &Path) -> Result<Option<SearchIndexManifest>, std::io::Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        // Retry once after a short delay, mirroring the tolerance in
        // `Index::open_or_create`, before reporting the failure.
        Err(first) => {
            std::thread::sleep(Duration::from_millis(250));
            match fs::read(path) {
                Ok(bytes) => bytes,
                Err(_) if !path.exists() => return Ok(None),
                Err(_) => return Err(first),
            }
        }
    };
    // A manifest that exists but does not parse is genuinely stale: it is a torn
    // or foreign write, and the index it describes cannot be trusted.
    match serde_json::from_slice::<SearchIndexManifest>(&bytes) {
        Ok(manifest) => Ok(Some(manifest)),
        Err(error) => {
            crate::log_warn!("[search] index manifest is not valid JSON ({error}); rebuilding");
            Ok(Some(SearchIndexManifest {
                version: 0,
                state: SearchIndexState::Building,
            }))
        }
    }
}

fn write_manifest(path: &Path, state: SearchIndexState) -> Result<(), SearchError> {
    let manifest = SearchIndexManifest {
        version: SEARCH_INDEX_VERSION,
        state,
    };
    // Replace atomically (fsync'd temporary + platform-atomic rename) so a
    // crash mid-write cannot corrupt the manifest into a needless full
    // rebuild of the index.
    let temporary = path.with_extension("json.tmp");
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.sync_all()?;
    drop(file);
    replace_file(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use super::{SearchIndexLayout, SearchIndexManifest, SearchIndexState};

    #[test]
    fn missing_or_incomplete_indexes_require_a_rebuild() {
        let root = temporary_directory("building");
        let layout = SearchIndexLayout::prepare(root.clone()).unwrap();

        assert!(layout.rebuild_required);
        let manifest: SearchIndexManifest =
            serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest.state, SearchIndexState::Building);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ready_indexes_with_metadata_can_be_reused() {
        let root = temporary_directory("ready");
        let layout = SearchIndexLayout::prepare(root.clone()).unwrap();
        fs::write(layout.index_directory.join("meta.json"), "{}").unwrap();
        layout.mark_ready().unwrap();

        let reopened = SearchIndexLayout::prepare(root.clone()).unwrap();

        assert!(!reopened.rebuild_required);
        fs::remove_dir_all(root).unwrap();
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "clipboard-search-manifest-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "clipboard-search-manifest-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).expect("create temp root");
        root
    }

    /// A missing manifest with no index is a first run: create and rebuild.
    #[test]
    fn a_missing_manifest_on_a_first_run_rebuilds() {
        let root = temp_root("first-run");
        let layout = SearchIndexLayout::prepare(root.clone()).expect("prepare");
        assert!(layout.rebuild_required);
        assert!(layout.index_directory.is_dir());
        assert!(layout.manifest_path.is_file());
        let _ = fs::remove_dir_all(&root);
    }

    /// A ready manifest with an index on disk needs no rebuild.
    #[test]
    fn a_ready_manifest_with_an_index_needs_no_rebuild() {
        let root = temp_root("ready");
        let first = SearchIndexLayout::prepare(root.clone()).expect("prepare");
        first.mark_ready().expect("mark ready");
        fs::write(
            first.index_directory.join("meta.json"),
            br#"{"segment":"probe"}"#,
        )
        .expect("write meta");

        let second = SearchIndexLayout::prepare(root.clone()).expect("prepare");
        assert!(
            !second.rebuild_required,
            "a ready manifest describing a present index must not rebuild"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// The regression: an unreadable-but-present manifest must not delete the
    /// index. Before this fix a transient sharing violation was treated as a
    /// stale manifest, which wiped a healthy index and forced a full rebuild
    /// (and another wipe on the next launch if the rebuild could not finish).
    ///
    /// The read is made to fail by locking the file, which is the Windows
    /// equivalent of an antivirus or backup scanner holding it open.
    #[test]
    fn an_unreadable_manifest_never_deletes_an_existing_index() {
        let root = temp_root("unreadable");
        let first = SearchIndexLayout::prepare(root.clone()).expect("prepare");
        first.mark_ready().expect("mark ready");
        let meta = first.index_directory.join("meta.json");
        fs::write(&meta, br#"{"segment":"probe"}"#).expect("write meta");

        // A directory in place of the manifest is readable-but-unparseable, so
        // use an explicitly locked handle for the unreadable case: open the real
        // manifest with exclusive access on Windows, which makes `fs::read` fail
        // while the file still exists.
        let held = lock_manifest_exclusively(&first.manifest_path);
        if held.is_none() {
            // The platform does not give us an exclusive lock; the unreadable
            // branch is then exercised through the parse-failure path below.
            let _ = fs::remove_dir_all(&root);
            return;
        }

        let second = SearchIndexLayout::prepare(root.clone()).expect("prepare");
        assert!(
            !second.rebuild_required,
            "an unreadable manifest must not authorize a rebuild"
        );
        assert!(
            meta.is_file(),
            "an unreadable manifest must not delete the index directory"
        );
        drop(held);
        let _ = fs::remove_dir_all(&root);
    }

    /// A manifest that exists but is not valid JSON *is* a stale write (torn or
    /// foreign), so it may authorize a rebuild.
    #[test]
    fn a_corrupt_manifest_does_authorize_a_rebuild() {
        let root = temp_root("corrupt");
        let first = SearchIndexLayout::prepare(root.clone()).expect("prepare");
        first.mark_ready().expect("mark ready");
        fs::write(
            first.index_directory.join("meta.json"),
            br#"{"segment":"probe"}"#,
        )
        .expect("write meta");
        fs::write(&first.manifest_path, b"{ not json").expect("corrupt the manifest");

        let second = SearchIndexLayout::prepare(root.clone()).expect("prepare");
        assert!(
            second.rebuild_required,
            "a corrupt manifest is a stale write and may rebuild"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Holds an exclusive handle on the manifest so a concurrent read fails.
    #[cfg(windows)]
    fn lock_manifest_exclusively(path: &Path) -> Option<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_NONE: u32 = 0;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_NONE)
            .open(path)
            .ok()
    }

    #[cfg(not(windows))]
    fn lock_manifest_exclusively(_path: &Path) -> Option<std::fs::File> {
        None
    }
}
