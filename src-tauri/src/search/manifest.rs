use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::SearchError;

pub const SEARCH_INDEX_VERSION: u32 = 1;

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
        let manifest = read_manifest(&manifest_path);
        let rebuild_required = !matches!(
            manifest,
            Some(SearchIndexManifest {
                version: SEARCH_INDEX_VERSION,
                state: SearchIndexState::Ready,
            }) if index_directory.join("meta.json").is_file()
        );

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

fn read_manifest(path: &PathBuf) -> Option<SearchIndexManifest> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_manifest(path: &PathBuf, state: SearchIndexState) -> Result<(), SearchError> {
    let manifest = SearchIndexManifest {
        version: SEARCH_INDEX_VERSION,
        state,
    };
    fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
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
