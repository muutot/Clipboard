//! Content-addressed v1 image/file/icon resources, split by concern.
//!
//! `store.rs` transfers one managed resource (fingerprint, upload, download,
//! verify) with bounded streaming and temporary files, `mutations.rs` plans and
//! rewrites the resource references of a mutation batch, and this module keeps
//! the shared imports, limits, and DTOs so every submodule can reach them with
//! `use super::*`.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::{
    layout::{parse_resource_key, resource_object_key, ResourceCategory},
    remote::{ObjectMetadata, ObjectStore, PutCondition, PutOutcome},
    wire::{
        resource_header, validate_resource_header, MutationBatch, SessionKey, SyncItemKind,
        RESOURCE_AUTH_TAG_LEN, RESOURCE_HEADER_LEN,
    },
};

const HASH_BUFFER_BYTES: usize = 256 * 1024;
const RESOURCE_CHUNK_BYTES: usize = 1024 * 1024;

mod mutations;
mod store;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDescriptor {
    pub category: ResourceCategory,
    pub source_path: PathBuf,
    pub object_key: String,
    pub plaintext_sha256: String,
    pub extension: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceUploadResult {
    pub object_key: String,
    pub size_bytes: u64,
    pub uploaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedResource {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub transferred_bytes: u64,
    pub reused_local_file: bool,
}

struct EncryptedResourceTemp {
    path: PathBuf,
    sha256: String,
    size_bytes: u64,
}

impl Drop for EncryptedResourceTemp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub image_bytes: u64,
    pub file_bytes: u64,
    pub icon_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRoots {
    pub images: PathBuf,
    pub files: PathBuf,
    pub icons: PathBuf,
}

impl ResourceRoots {
    pub fn new(images: PathBuf, files: PathBuf, icons: PathBuf) -> Self {
        Self {
            images,
            files,
            icons,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceTransferStats {
    pub referenced_resources: u64,
    pub transferred_resources: u64,
    pub transferred_bytes: u64,
    pub skipped_resources: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncResourceRef {
    pub slot: String,
    pub ordinal: u32,
    pub object_key: String,
}

pub use mutations::{
    collect_mutation_resource_refs, defer_mutation_resources, materialize_mutation_resources,
    prepare_mutation_resources,
};
pub use store::{
    ensure_resource_uploaded, fingerprint_resource, materialize_resource, verify_local_resource,
};
