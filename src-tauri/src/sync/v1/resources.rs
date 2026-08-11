use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::{
    layout::{parse_resource_key, resource_object_key, ResourceCategory},
    remote::{ObjectMetadata, ObjectStore, PutCondition, PutOutcome},
    MutationBatch,
};
use crate::{domain::ClipboardKind, storage::StoragePaths};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub image_bytes: u64,
    pub file_bytes: u64,
    pub icon_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceTransferStats {
    pub referenced_resources: u64,
    pub transferred_resources: u64,
    pub transferred_bytes: u64,
    pub skipped_resources: u64,
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
    destination_root: &Path,
    max_bytes: u64,
) -> Result<MaterializedResource, String> {
    let parsed = parse_resource_key(object_key)?;
    let category_root = prepare_destination_root(destination_root)?;
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

/// Rewrites local managed paths in a mutation batch to canonical v1 resource
/// keys and uploads every distinct referenced object before the caller may
/// publish the enclosing snapshot or segment. Missing, external, or oversized
/// local files are cleared from portable storage-path fields rather than
/// leaking an unusable machine-local path to other devices.
pub fn prepare_mutation_resources(
    store: &impl ObjectStore,
    mutations: &mut MutationBatch,
    paths: &StoragePaths,
    limits: ResourceLimits,
) -> Result<ResourceTransferStats, String> {
    let mut descriptors = BTreeMap::<String, ResourceDescriptor>::new();
    let mut skipped_resources = 0u64;

    for replicated in &mut mutations.upserts {
        let item = &mut replicated.item;
        let mut path_map = BTreeMap::<String, Option<String>>::new();
        match item.kind {
            ClipboardKind::Image => {
                let original_resource = item.resource_path.clone();
                item.resource_path = rewrite_outgoing_path(
                    original_resource.as_deref(),
                    &paths.images,
                    ResourceCategory::Image,
                    limits.image_bytes,
                    &mut descriptors,
                    &mut path_map,
                    &mut skipped_resources,
                );
                if item.preview_path == original_resource && item.resource_path.is_some() {
                    if let (Some(original), Some(portable)) =
                        (item.preview_path.clone(), item.resource_path.clone())
                    {
                        path_map.insert(original, Some(portable.clone()));
                        item.preview_path = Some(portable);
                    }
                } else {
                    item.preview_path = rewrite_outgoing_path(
                        item.preview_path.as_deref(),
                        &paths.previews,
                        ResourceCategory::Preview,
                        limits.image_bytes,
                        &mut descriptors,
                        &mut path_map,
                        &mut skipped_resources,
                    );
                }
            }
            ClipboardKind::File => {
                item.resource_path = rewrite_outgoing_path(
                    item.resource_path.as_deref(),
                    &paths.files,
                    ResourceCategory::File,
                    limits.file_bytes,
                    &mut descriptors,
                    &mut path_map,
                    &mut skipped_resources,
                );
                if let Some(json) = item.text_content.as_deref() {
                    if let Ok(local_paths) = serde_json::from_str::<Vec<String>>(json) {
                        let portable_paths = local_paths
                            .iter()
                            .filter_map(|local_path| {
                                rewrite_outgoing_path(
                                    Some(local_path),
                                    &paths.files,
                                    ResourceCategory::File,
                                    limits.file_bytes,
                                    &mut descriptors,
                                    &mut path_map,
                                    &mut skipped_resources,
                                )
                            })
                            .collect::<Vec<_>>();
                        item.text_content =
                            Some(serde_json::to_string(&portable_paths).map_err(|error| {
                                format!("failed to encode portable file paths: {error}")
                            })?);
                    }
                }
            }
            ClipboardKind::Text | ClipboardKind::Link => {}
        }

        item.icon_path = rewrite_outgoing_icon(
            item.icon_path.as_deref(),
            paths,
            limits.icon_bytes,
            &mut descriptors,
            &mut path_map,
            &mut skipped_resources,
        );
        rewrite_metadata_paths(item.metadata_json.as_mut(), &path_map, true)?;
    }

    let mut stats = ResourceTransferStats {
        referenced_resources: descriptors.len() as u64,
        skipped_resources,
        ..ResourceTransferStats::default()
    };
    for descriptor in descriptors.values() {
        let result = ensure_resource_uploaded(store, descriptor)?;
        if result.uploaded {
            stats.transferred_resources += 1;
            stats.transferred_bytes = stats
                .transferred_bytes
                .checked_add(result.size_bytes)
                .ok_or_else(|| "uploaded resource byte count overflowed".to_string())?;
        }
    }
    Ok(stats)
}

/// Downloads every canonical resource referenced by a remote mutation batch,
/// verifies it, and rewrites portable keys to paths owned by this device before
/// the database transaction begins.
pub fn materialize_mutation_resources(
    store: &impl ObjectStore,
    mutations: &mut MutationBatch,
    paths: &StoragePaths,
    limits: ResourceLimits,
) -> Result<ResourceTransferStats, String> {
    let mut materialized = BTreeMap::<String, String>::new();
    let mut stats = ResourceTransferStats::default();

    for replicated in &mut mutations.upserts {
        let item = &mut replicated.item;
        match item.kind {
            ClipboardKind::Image => {
                item.resource_path = rewrite_incoming_path(
                    store,
                    item.resource_path.as_deref(),
                    &[ResourceCategory::Image],
                    paths,
                    limits,
                    false,
                    &mut materialized,
                    &mut stats,
                )?;
                item.preview_path = rewrite_incoming_path(
                    store,
                    item.preview_path.as_deref(),
                    &[ResourceCategory::Image, ResourceCategory::Preview],
                    paths,
                    limits,
                    false,
                    &mut materialized,
                    &mut stats,
                )?;
            }
            ClipboardKind::File => {
                item.resource_path = rewrite_incoming_path(
                    store,
                    item.resource_path.as_deref(),
                    &[ResourceCategory::File],
                    paths,
                    limits,
                    false,
                    &mut materialized,
                    &mut stats,
                )?;
                if let Some(json) = item.text_content.as_deref() {
                    if let Ok(portable_paths) = serde_json::from_str::<Vec<String>>(json) {
                        let local_paths = portable_paths
                            .iter()
                            .map(|portable_path| {
                                rewrite_incoming_path(
                                    store,
                                    Some(portable_path),
                                    &[ResourceCategory::File],
                                    paths,
                                    limits,
                                    false,
                                    &mut materialized,
                                    &mut stats,
                                )?
                                .ok_or_else(|| {
                                    "portable file path unexpectedly disappeared".to_string()
                                })
                            })
                            .collect::<Result<Vec<_>, String>>()?;
                        item.text_content =
                            Some(serde_json::to_string(&local_paths).map_err(|error| {
                                format!("failed to encode local file paths: {error}")
                            })?);
                    }
                }
            }
            ClipboardKind::Text | ClipboardKind::Link => {}
        }

        item.icon_path = rewrite_incoming_path(
            store,
            item.icon_path.as_deref(),
            &[ResourceCategory::Icon],
            paths,
            limits,
            true,
            &mut materialized,
            &mut stats,
        )?;
        rewrite_metadata_paths(
            item.metadata_json.as_mut(),
            &materialized_map(&materialized),
            false,
        )?;
    }
    stats.referenced_resources = materialized.len() as u64;
    Ok(stats)
}

fn rewrite_outgoing_path(
    value: Option<&str>,
    managed_root: &Path,
    category: ResourceCategory,
    max_bytes: u64,
    descriptors: &mut BTreeMap<String, ResourceDescriptor>,
    path_map: &mut BTreeMap<String, Option<String>>,
    skipped_resources: &mut u64,
) -> Option<String> {
    let value = value?.to_string();
    if let Some(existing) = path_map.get(&value) {
        return existing.clone();
    }
    let descriptor = fingerprint_resource(managed_root, Path::new(&value), category, max_bytes);
    match descriptor {
        Ok(descriptor) => {
            let object_key = descriptor.object_key.clone();
            descriptors.entry(object_key.clone()).or_insert(descriptor);
            path_map.insert(value, Some(object_key.clone()));
            Some(object_key)
        }
        Err(_) => {
            *skipped_resources = skipped_resources.saturating_add(1);
            path_map.insert(value, None);
            None
        }
    }
}

fn rewrite_outgoing_icon(
    value: Option<&str>,
    paths: &StoragePaths,
    max_bytes: u64,
    descriptors: &mut BTreeMap<String, ResourceDescriptor>,
    path_map: &mut BTreeMap<String, Option<String>>,
    skipped_resources: &mut u64,
) -> Option<String> {
    let value = value?;
    let icon_root = paths.storage.join("icons");
    let source = if Path::new(value).is_absolute() {
        PathBuf::from(value)
    } else if is_safe_file_name(value) {
        icon_root.join(value)
    } else {
        path_map.insert(value.to_string(), None);
        *skipped_resources = skipped_resources.saturating_add(1);
        return None;
    };
    rewrite_outgoing_path(
        source.to_str(),
        &icon_root,
        ResourceCategory::Icon,
        max_bytes,
        descriptors,
        path_map,
        skipped_resources,
    )
}

#[allow(clippy::too_many_arguments)]
fn rewrite_incoming_path(
    store: &impl ObjectStore,
    value: Option<&str>,
    allowed_categories: &[ResourceCategory],
    paths: &StoragePaths,
    limits: ResourceLimits,
    bare_file_name: bool,
    materialized: &mut BTreeMap<String, String>,
    stats: &mut ResourceTransferStats,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if let Some(existing) = materialized.get(value) {
        return Ok(Some(if bare_file_name {
            Path::new(existing)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "materialized icon has no portable file name".to_string())?
                .to_string()
        } else {
            existing.clone()
        }));
    }
    let parsed = parse_resource_key(value)?;
    if !allowed_categories.contains(&parsed.category) {
        return Err(format!(
            "resource {value:?} has category {:?}, expected one of {allowed_categories:?}",
            parsed.category
        ));
    }
    let (destination_root, max_bytes) = resource_destination(paths, parsed.category, limits);
    let result = materialize_resource(store, value, &destination_root, max_bytes)?;
    let local = result.path.to_string_lossy().to_string();
    if !result.reused_local_file {
        stats.transferred_resources += 1;
        stats.transferred_bytes = stats
            .transferred_bytes
            .checked_add(result.size_bytes)
            .ok_or_else(|| "downloaded resource byte count overflowed".to_string())?;
    }
    materialized.insert(value.to_string(), local.clone());
    Ok(Some(if bare_file_name {
        result
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "materialized icon has no portable file name".to_string())?
            .to_string()
    } else {
        local
    }))
}

fn resource_destination(
    paths: &StoragePaths,
    category: ResourceCategory,
    limits: ResourceLimits,
) -> (PathBuf, u64) {
    match category {
        ResourceCategory::Image => (paths.images.clone(), limits.image_bytes),
        ResourceCategory::Preview => (paths.previews.clone(), limits.image_bytes),
        ResourceCategory::File => (paths.files.clone(), limits.file_bytes),
        ResourceCategory::Icon => (paths.storage.join("icons"), limits.icon_bytes),
    }
}

fn materialized_map(materialized: &BTreeMap<String, String>) -> BTreeMap<String, Option<String>> {
    materialized
        .iter()
        .map(|(portable, local)| (portable.clone(), Some(local.clone())))
        .collect()
}

fn rewrite_metadata_paths(
    metadata_json: Option<&mut String>,
    path_map: &BTreeMap<String, Option<String>>,
    clear_unavailable: bool,
) -> Result<(), String> {
    let Some(metadata_json) = metadata_json else {
        return Ok(());
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(metadata_json) else {
        return Ok(());
    };
    rewrite_json_value(&mut value, path_map, clear_unavailable, None);
    *metadata_json = serde_json::to_string(&value)
        .map_err(|error| format!("failed to encode rewritten resource metadata: {error}"))?;
    Ok(())
}

fn rewrite_json_value(
    value: &mut serde_json::Value,
    path_map: &BTreeMap<String, Option<String>>,
    clear_unavailable: bool,
    parent_key: Option<&str>,
) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                rewrite_json_value(child, path_map, clear_unavailable, Some(key));
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                rewrite_json_value(child, path_map, clear_unavailable, parent_key);
            }
        }
        serde_json::Value::String(path)
            if is_managed_metadata_key(parent_key) && path_map.contains_key(path) =>
        {
            match path_map.get(path).and_then(Clone::clone) {
                Some(replacement) => *path = replacement,
                None if clear_unavailable => *value = serde_json::Value::Null,
                None => {}
            }
        }
        _ => {}
    }
}

fn is_managed_metadata_key(key: Option<&str>) -> bool {
    matches!(
        key,
        Some("resourcePath" | "storagePath" | "previewPath" | "path")
    )
}

fn is_safe_file_name(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\', ':'])
        && !value.bytes().any(|byte| byte.is_ascii_control())
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

fn prepare_destination_root(destination_root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(destination_root)
        .map_err(|error| format!("failed to create resource cache root: {error}"))?;
    let canonical_root = fs::canonicalize(destination_root)
        .map_err(|error| format!("failed to resolve resource cache root: {error}"))?;
    if !canonical_root.is_dir() {
        return Err("resource cache root is not a directory".to_string());
    }
    let metadata = fs::symlink_metadata(&canonical_root)
        .map_err(|error| format!("failed to inspect resource cache root: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("resource cache root is not a regular directory".to_string());
    }
    Ok(canonical_root)
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
    use crate::{
        domain::{ClipboardItem, ClipboardKind},
        sync::v1::{MutationBatch, RecordVersion, ReplicatedItem},
    };

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

    fn sample_item(id: &str, kind: ClipboardKind) -> ReplicatedItem {
        ReplicatedItem {
            item: ClipboardItem {
                id: id.to_string(),
                kind,
                title: id.to_string(),
                text_content: None,
                html_content: None,
                rtf_content: None,
                resource_path: None,
                preview_path: None,
                content_hash: format!("hash-{id}"),
                source_app: None,
                icon_path: None,
                size_bytes: 0,
                created_at_ms: 1,
                last_used_at_ms: None,
                is_favorite: false,
                metadata_json: None,
            },
            version: RecordVersion {
                modified_at_ms: 1,
                writer_device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
            },
        }
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
        let expected_path = cache.join(format!("sha256-{}.{}", parsed.sha256, parsed.extension));
        assert!(!expected_path.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mutation_resources_round_trip_without_machine_local_paths() {
        let root = temporary_directory("mutation-round-trip");
        let source_paths = StoragePaths::initialize(root.join("source")).unwrap();
        let target_paths = StoragePaths::initialize(root.join("target")).unwrap();
        let image_path = source_paths.images.join("image.png");
        let file_path = source_paths.files.join("document.txt");
        let icon_dir = source_paths.storage.join("icons");
        fs::create_dir_all(&icon_dir).unwrap();
        fs::write(&image_path, b"image-bytes").unwrap();
        fs::write(&file_path, b"file-bytes").unwrap();
        fs::write(icon_dir.join("app.png"), b"icon-bytes").unwrap();

        let mut image = sample_item("image", ClipboardKind::Image);
        image.item.resource_path = Some(image_path.to_string_lossy().to_string());
        image.item.preview_path = image.item.resource_path.clone();
        image.item.icon_path = Some("app.png".to_string());
        image.item.metadata_json = Some(
            serde_json::json!({
                "resourcePath": image_path,
                "storagePath": image_path,
                "previewPath": image_path,
            })
            .to_string(),
        );

        let mut file = sample_item("file", ClipboardKind::File);
        let source_file = file_path.to_string_lossy().to_string();
        file.item.resource_path = Some(source_file.clone());
        file.item.text_content = Some(serde_json::to_string(&[&source_file]).unwrap());
        file.item.metadata_json = Some(
            serde_json::json!({
                "resourcePath": source_file,
                "files": [{
                    "storagePath": source_file,
                    "originalPath": "C:/original/document.txt",
                }],
            })
            .to_string(),
        );

        let store = MemoryStore::default();
        let limits = ResourceLimits {
            image_bytes: 1024,
            file_bytes: 1024,
            icon_bytes: 1024,
        };
        let mut batch = MutationBatch {
            upserts: vec![image, file],
            tombstones: Vec::new(),
        };
        let uploaded =
            prepare_mutation_resources(&store, &mut batch, &source_paths, limits).unwrap();
        assert_eq!(uploaded.referenced_resources, 3);
        assert_eq!(uploaded.transferred_resources, 3);
        assert!(batch.upserts.iter().all(|item| {
            item.item
                .resource_path
                .as_deref()
                .is_none_or(|path| path.starts_with("v1/resources/"))
        }));
        assert_eq!(
            batch.upserts[0].item.preview_path,
            batch.upserts[0].item.resource_path
        );
        assert!(batch.upserts[0]
            .item
            .icon_path
            .as_deref()
            .unwrap()
            .starts_with("v1/resources/icon/"));

        let downloaded =
            materialize_mutation_resources(&store, &mut batch, &target_paths, limits).unwrap();
        assert_eq!(downloaded.referenced_resources, 3);
        assert_eq!(downloaded.transferred_resources, 3);
        let target_images = fs::canonicalize(&target_paths.images).unwrap();
        let target_files = fs::canonicalize(&target_paths.files).unwrap();
        assert!(
            Path::new(batch.upserts[0].item.resource_path.as_deref().unwrap())
                .starts_with(&target_images)
        );
        assert!(
            Path::new(batch.upserts[1].item.resource_path.as_deref().unwrap())
                .starts_with(&target_files)
        );
        let local_icon = batch.upserts[0].item.icon_path.as_deref().unwrap();
        assert!(local_icon.starts_with("sha256-"));
        assert!(local_icon.ends_with(".png"));
        let metadata: serde_json::Value =
            serde_json::from_str(batch.upserts[1].item.metadata_json.as_deref().unwrap()).unwrap();
        assert!(
            Path::new(metadata["files"][0]["storagePath"].as_str().unwrap())
                .starts_with(&target_files)
        );
        assert_eq!(
            metadata["files"][0]["originalPath"],
            "C:/original/document.txt"
        );

        fs::remove_dir_all(root).unwrap();
    }
}
