//! Captured file references and the JSON metadata block written with them.

use std::path::Path;

use crate::content::{
    created_at_ms, extension_for_path, mime_type_for_path, modified_at_ms, FileStore,
    RESOURCE_METADATA_SCHEMA_VERSION,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFileReference {
    pub(crate) original_path: String,
    pub(crate) storage_path: String,
    pub(crate) original_name: String,
    pub(crate) size_bytes: u64,
    pub(crate) content_hash: Option<String>,
    pub(crate) extension: Option<String>,
    pub(crate) mime_type: String,
    pub(crate) created_at_ms: Option<i64>,
    pub(crate) modified_at_ms: Option<i64>,
    pub(crate) copied: bool,
}

/// Persists CF_HDROP file references and materializes metadata for a file capture.
pub fn store_captured_file_references(
    file_paths: &[String],
    file_storage_dir: &Path,
    max_copy_size_bytes: u64,
) -> Vec<CapturedFileReference> {
    file_paths
        .iter()
        .map(|file_path| {
            let source_path = Path::new(file_path);
            match FileStore::save_file(source_path, file_storage_dir, max_copy_size_bytes) {
                Ok(info) => CapturedFileReference {
                    original_path: file_path.clone(),
                    copied: Path::new(&info.storage_path) != source_path,
                    storage_path: info.storage_path,
                    original_name: info.original_name,
                    size_bytes: info.size_bytes,
                    content_hash: Some(info.content_hash),
                    extension: info.extension,
                    mime_type: info.mime_type,
                    created_at_ms: info.created_at_ms,
                    modified_at_ms: info.modified_at_ms,
                },
                Err(error) => {
                    crate::log_error!(
                        "[clipboard-worker] failed to store file {}: {error}",
                        source_path.display()
                    );
                    let metadata = std::fs::metadata(source_path).ok();
                    CapturedFileReference {
                        original_path: file_path.clone(),
                        storage_path: file_path.clone(),
                        original_name: source_path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| file_path.clone()),
                        size_bytes: metadata.as_ref().map_or(0, std::fs::Metadata::len),
                        content_hash: None,
                        extension: extension_for_path(source_path),
                        mime_type: mime_type_for_path(source_path),
                        created_at_ms: metadata.as_ref().and_then(created_at_ms),
                        modified_at_ms: metadata.as_ref().and_then(modified_at_ms),
                        copied: false,
                    }
                }
            }
        })
        .collect()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileMetadata {
    schema_version: u8,
    size_bytes: u64,
    resource_path: Option<String>,
    original_path: Option<String>,
    files: Vec<FileEntry>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileEntry {
    name: String,
    extension: Option<String>,
    mime_type: String,
    size_bytes: u64,
    storage_path: String,
    original_path: String,
    content_hash: Option<String>,
    copied: bool,
    created_at_ms: Option<i64>,
    modified_at_ms: Option<i64>,
}

/// Serializes captured file references into the record `metadata_json` blob.
pub fn captured_file_metadata(files: &[CapturedFileReference]) -> String {
    let first = files.first();
    serde_json::to_string(&FileMetadata {
        schema_version: RESOURCE_METADATA_SCHEMA_VERSION,
        size_bytes: files.iter().map(|file| file.size_bytes).sum::<u64>(),
        resource_path: first.map(|file| file.storage_path.clone()),
        original_path: if files.len() == 1 {
            first.map(|file| file.original_path.clone())
        } else {
            None
        },
        files: files
            .iter()
            .map(|file| FileEntry {
                name: file.original_name.clone(),
                extension: file.extension.clone(),
                mime_type: file.mime_type.clone(),
                size_bytes: file.size_bytes,
                storage_path: file.storage_path.clone(),
                original_path: file.original_path.clone(),
                content_hash: file.content_hash.clone(),
                copied: file.copied,
                created_at_ms: file.created_at_ms,
                modified_at_ms: file.modified_at_ms,
            })
            .collect(),
    })
    .unwrap_or_default()
}
