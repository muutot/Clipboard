use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::{
    layout::{parse_resource_key, resource_object_key, ResourceCategory},
    remote::{ObjectMetadata, ObjectStore, PutCondition, PutOutcome},
};

const HASH_BUFFER_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDescriptor {
    pub category: ResourceCategory,
    pub source_path: PathBuf,
    pub object_key: String,
    pub sha256: String,
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
    pub reused_local_file: bool,
}

/// Fingerprints one managed resource with a bounded streaming read. The
/// canonical source must be a regular non-symlink file below `managed_root`.
pub fn fingerprint_resource(
    managed_root: &Path,
    source_path: &Path,
    category: ResourceCategory,
    max_bytes: u64,
) -> Result<ResourceDescriptor, String> {
    let canonical_root = fs::canonicalize(managed_root)
        .map_err(|error| format!("failed to resolve managed resource root: {error}"))?;
    if !canonical_root.is_dir() {
        return Err("managed resource root is not a directory".to_string());
    }
    let link_metadata = fs::symlink_metadata(source_path)
        .map_err(|error| format!("failed to inspect managed resource: {error}"))?;
    if link_metadata.file_type().is_symlink() {
        return Err("managed resource cannot be a symlink".to_string());
    }
    let canonical_source = fs::canonicalize(source_path)
        .map_err(|error| format!("failed to resolve managed resource: {error}"))?;
    canonical_source
        .strip_prefix(&canonical_root)
        .map_err(|_| "managed resource is outside its configured root".to_string())?;

    let (sha256, size_bytes) = hash_regular_file(&canonical_source, max_bytes)?;
    let extension = portable_extension(&canonical_source);
    let object_key = resource_object_key(category, &sha256, &extension)?;
    Ok(ResourceDescriptor {
        category,
        source_path: canonical_source,
        object_key,
        sha256,
        extension,
        size_bytes,
    })
}

/// Avoids uploading an already-present content-addressed object. The HEAD +
/// create-only PUT pair minimizes traffic while remaining safe under races.
pub fn ensure_resource_uploaded(
    store: &impl ObjectStore,
    resource: &ResourceDescriptor,
) -> Result<ResourceUploadResult, String> {
    if let Some(metadata) = store.head(&resource.object_key)? {
        validate_remote_size(&resource.object_key, resource.size_bytes, &metadata)?;
        return Ok(ResourceUploadResult {
            object_key: resource.object_key.clone(),
            size_bytes: resource.size_bytes,
            uploaded: false,
        });
    }

    let uploaded = match store.put_file(
        &resource.object_key,
        &resource.source_path,
        &resource.sha256,
        resource.size_bytes,
        PutCondition::IfAbsent,
    )? {
        PutOutcome::Stored { .. } => true,
        PutOutcome::PreconditionFailed => {
            let metadata = store.head(&resource.object_key)?.ok_or_else(|| {
                format!(
                    "resource create-only write lost its race but {:?} is still absent",
                    resource.object_key
                )
            })?;
            validate_remote_size(&resource.object_key, resource.size_bytes, &metadata)?;
            false
        }
    };

    Ok(ResourceUploadResult {
        object_key: resource.object_key.clone(),
        size_bytes: resource.size_bytes,
        uploaded,
    })
}

/// Materializes a canonical resource object into a local content-addressed
/// cache. Downloads are written to a unique temporary file, hash-checked, and
/// renamed only after complete verification.
pub fn materialize_resource(
    store: &impl ObjectStore,
    object_key: &str,
    cache_root: &Path,
    max_bytes: u64,
) -> Result<MaterializedResource, String> {
    let parsed = parse_resource_key(object_key)?;
    let category_root = prepare_category_root(cache_root, parsed.category)?;
    let file_name = format!("sha256-{}.{}", parsed.sha256, parsed.extension);
    let final_path = category_root.join(file_name);

    if let Some(metadata) = symlink_metadata_if_exists(&final_path)? {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("cached resource path is not a regular file".to_string());
        }
        if metadata.len() <= max_bytes {
            let (sha256, size_bytes) = hash_regular_file(&final_path, max_bytes)?;
            if sha256 == parsed.sha256 {
                return Ok(MaterializedResource {
                    path: final_path,
                    size_bytes,
                    reused_local_file: true,
                });
            }
        }
    }

    let temp_path = category_root.join(format!(
        ".download-{}-{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    let download = match store.get_to_file(object_key, &temp_path, max_bytes) {
        Ok(Some(download)) => download,
        Ok(None) => {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("remote resource {object_key:?} does not exist"));
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
    };
    if download.sha256 != parsed.sha256 {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "remote resource digest mismatch for {object_key:?}: expected {}, got {}",
            parsed.sha256, download.sha256
        ));
    }
    if download.size_bytes > max_bytes {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "remote resource {object_key:?} exceeds the {max_bytes}-byte limit"
        ));
    }

    if let Err(error) = publish_verified_file(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(MaterializedResource {
        path: final_path,
        size_bytes: download.size_bytes,
        reused_local_file: false,
    })
}

fn validate_remote_size(
    object_key: &str,
    expected_size: u64,
    metadata: &ObjectMetadata,
) -> Result<(), String> {
    if metadata
        .size_bytes
        .is_some_and(|actual_size| actual_size != expected_size)
    {
        return Err(format!(
            "remote resource size mismatch for {object_key:?}: expected {expected_size}, got {}",
            metadata.size_bytes.unwrap_or_default()
        ));
    }
    Ok(())
}

fn symlink_metadata_if_exists(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to inspect cached resource: {error}")),
    }
}

fn publish_verified_file(temp_path: &Path, final_path: &Path) -> Result<(), String> {
    if let Some(metadata) = symlink_metadata_if_exists(final_path)? {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("cached resource path changed to a non-regular file".to_string());
        }
        fs::remove_file(final_path)
            .map_err(|error| format!("failed to replace corrupted cached resource: {error}"))?;
    }
    fs::rename(temp_path, final_path)
        .map_err(|error| format!("failed to publish cached resource: {error}"))
}

fn portable_extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 16
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| "bin".to_string())
}

fn hash_regular_file(path: &Path, max_bytes: u64) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|error| format!("failed to open resource: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect resource: {error}"))?;
    if !metadata.is_file() {
        return Err("resource is not a regular file".to_string());
    }
    if metadata.len() > max_bytes {
        return Err(format!("resource exceeds the {max_bytes}-byte limit"));
    }

    let mut hasher = Sha256::new();
    let mut size_bytes = 0u64;
    let mut buffer = vec![0u8; HASH_BUFFER_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash resource: {error}"))?;
        if read == 0 {
            break;
        }
        size_bytes = size_bytes
            .checked_add(read as u64)
            .ok_or_else(|| "resource size overflowed".to_string())?;
        if size_bytes > max_bytes {
            return Err(format!("resource exceeds the {max_bytes}-byte limit"));
        }
        hasher.update(&buffer[..read]);
    }
    if size_bytes != metadata.len() {
        return Err(format!(
            "resource size changed while hashing: expected {}, got {size_bytes}",
            metadata.len()
        ));
    }
    Ok((hex::encode(hasher.finalize()), size_bytes))
}

fn prepare_category_root(cache_root: &Path, category: ResourceCategory) -> Result<PathBuf, String> {
    fs::create_dir_all(cache_root)
        .map_err(|error| format!("failed to create resource cache root: {error}"))?;
    let canonical_root = fs::canonicalize(cache_root)
        .map_err(|error| format!("failed to resolve resource cache root: {error}"))?;
    if !canonical_root.is_dir() {
        return Err("resource cache root is not a directory".to_string());
    }
    let category_root = canonical_root.join(category.as_str());
    if category_root.exists() {
        let metadata = fs::symlink_metadata(&category_root)
            .map_err(|error| format!("failed to inspect resource cache category: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("resource cache category is not a regular directory".to_string());
        }
    } else {
        fs::create_dir(&category_root)
            .map_err(|error| format!("failed to create resource cache category: {error}"))?;
    }
    let canonical_category = fs::canonicalize(&category_root)
        .map_err(|error| format!("failed to resolve resource cache category: {error}"))?;
    canonical_category
        .strip_prefix(&canonical_root)
        .map_err(|_| "resource cache category escapes its root".to_string())?;
    Ok(canonical_category)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        io::Write,
    };

    use super::*;
    use crate::sync::v1::remote::{DownloadedFile, DownloadedObject, ObjectInfo};

    #[derive(Default)]
    struct MemoryStore {
        objects: RefCell<BTreeMap<String, Vec<u8>>>,
        file_gets: Cell<u64>,
        file_puts: Cell<u64>,
    }

    impl ObjectStore for MemoryStore {
        fn list(&self, prefix: &str, start_after: Option<&str>) -> Result<Vec<ObjectInfo>, String> {
            Ok(self
                .objects
                .borrow()
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .filter(|(key, _)| start_after.is_none_or(|cursor| key.as_str() > cursor))
                .map(|(key, bytes)| ObjectInfo {
                    key: key.clone(),
                    size_bytes: Some(bytes.len() as u64),
                    modified_ms: None,
                })
                .collect())
        }

        fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
            Ok(self
                .objects
                .borrow()
                .get(key)
                .cloned()
                .map(|bytes| DownloadedObject { bytes, etag: None }))
        }

        fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String> {
            Ok(self.objects.borrow().get(key).map(|bytes| ObjectMetadata {
                size_bytes: Some(bytes.len() as u64),
                etag: None,
            }))
        }

        fn get_to_file(
            &self,
            key: &str,
            destination: &Path,
            max_bytes: u64,
        ) -> Result<Option<DownloadedFile>, String> {
            let Some(bytes) = self.objects.borrow().get(key).cloned() else {
                return Ok(None);
            };
            if bytes.len() as u64 > max_bytes {
                return Err("memory resource exceeds download limit".to_string());
            }
            self.file_gets.set(self.file_gets.get() + 1);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|error| error.to_string())?;
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            Ok(Some(DownloadedFile {
                size_bytes: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
                etag: None,
            }))
        }

        fn put(
            &self,
            key: &str,
            bytes: Vec<u8>,
            condition: PutCondition,
        ) -> Result<PutOutcome, String> {
            let mut objects = self.objects.borrow_mut();
            if matches!(condition, PutCondition::IfAbsent) && objects.contains_key(key) {
                return Ok(PutOutcome::PreconditionFailed);
            }
            objects.insert(key.to_string(), bytes);
            Ok(PutOutcome::Stored { etag: None })
        }

        fn put_file(
            &self,
            key: &str,
            path: &Path,
            sha256: &str,
            size_bytes: u64,
            condition: PutCondition,
        ) -> Result<PutOutcome, String> {
            self.file_puts.set(self.file_puts.get() + 1);
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            if bytes.len() as u64 != size_bytes || hex::encode(Sha256::digest(&bytes)) != sha256 {
                return Err("memory resource fingerprint mismatch".to_string());
            }
            self.put(key, bytes, condition)
        }

        fn delete(&self, key: &str) -> Result<(), String> {
            self.objects.borrow_mut().remove(key);
            Ok(())
        }
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "clipboard-v1-resource-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn fingerprint_and_upload_stream_only_one_new_resource() {
        let root = temporary_directory("upload");
        let managed = root.join("managed");
        fs::create_dir_all(&managed).unwrap();
        let source = managed.join("payload.DATA");
        let bytes = vec![0x4d; 2 * 1024 * 1024 + 3];
        fs::write(&source, &bytes).unwrap();

        let resource =
            fingerprint_resource(&managed, &source, ResourceCategory::File, 3 * 1024 * 1024)
                .unwrap();
        assert_eq!(resource.extension, "data");
        assert_eq!(resource.size_bytes, bytes.len() as u64);

        let store = MemoryStore::default();
        assert!(
            ensure_resource_uploaded(&store, &resource)
                .unwrap()
                .uploaded
        );
        assert!(
            !ensure_resource_uploaded(&store, &resource)
                .unwrap()
                .uploaded
        );
        assert_eq!(store.file_puts.get(), 1);
        assert_eq!(store.objects.borrow().len(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn materialization_reuses_valid_cache_and_repairs_corruption() {
        let root = temporary_directory("download");
        let cache = root.join("cache");
        let bytes = b"resource bytes".repeat(4096);
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let key = resource_object_key(ResourceCategory::Image, &sha256, "png").unwrap();
        let store = MemoryStore::default();
        store
            .objects
            .borrow_mut()
            .insert(key.clone(), bytes.clone());

        let first = materialize_resource(&store, &key, &cache, 1024 * 1024).unwrap();
        assert!(!first.reused_local_file);
        assert_eq!(fs::read(&first.path).unwrap(), bytes);
        let second = materialize_resource(&store, &key, &cache, 1024 * 1024).unwrap();
        assert!(second.reused_local_file);
        assert_eq!(store.file_gets.get(), 1);

        fs::write(&first.path, b"corrupt").unwrap();
        let repaired = materialize_resource(&store, &key, &cache, 1024 * 1024).unwrap();
        assert!(!repaired.reused_local_file);
        assert_eq!(fs::read(&repaired.path).unwrap(), bytes);
        assert_eq!(store.file_gets.get(), 2);

        fs::write(&first.path, vec![0u8; 2 * 1024 * 1024]).unwrap();
        let repaired_oversized = materialize_resource(&store, &key, &cache, 1024 * 1024).unwrap();
        assert!(!repaired_oversized.reused_local_file);
        assert_eq!(fs::read(&repaired_oversized.path).unwrap(), bytes);
        assert_eq!(store.file_gets.get(), 3);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fingerprint_rejects_outside_and_oversized_files() {
        let root = temporary_directory("bounds");
        let managed = root.join("managed");
        fs::create_dir_all(&managed).unwrap();
        let outside = root.join("outside.bin");
        fs::write(&outside, vec![0u8; 32]).unwrap();
        assert!(fingerprint_resource(&managed, &outside, ResourceCategory::File, 1024).is_err());

        let oversized = managed.join("oversized.bin");
        fs::write(&oversized, vec![0u8; 2048]).unwrap();
        assert!(fingerprint_resource(&managed, &oversized, ResourceCategory::File, 1024).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_remote_resource_never_replaces_the_cache() {
        let root = temporary_directory("corrupt-remote");
        let cache = root.join("cache");
        let expected = b"expected";
        let sha256 = hex::encode(Sha256::digest(expected));
        let key = resource_object_key(ResourceCategory::File, &sha256, "bin").unwrap();
        let store = MemoryStore::default();
        store
            .objects
            .borrow_mut()
            .insert(key.clone(), b"corrupt".to_vec());

        assert!(materialize_resource(&store, &key, &cache, 1024).is_err());
        let parsed = parse_resource_key(&key).unwrap();
        let expected_path = cache
            .join(parsed.category.as_str())
            .join(format!("sha256-{}.{}", parsed.sha256, parsed.extension));
        assert!(!expected_path.exists());

        fs::remove_dir_all(root).unwrap();
    }
}
