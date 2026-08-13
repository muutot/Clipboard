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

/// Fingerprints one managed resource with a bounded streaming read. The
/// canonical source must be a regular non-symlink file below `managed_root`.
pub fn fingerprint_resource(
    managed_root: &Path,
    source_path: &Path,
    category: ResourceCategory,
    max_bytes: u64,
    session_key: Option<&SessionKey>,
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

    let (plaintext_sha256, size_bytes) = hash_regular_file(&canonical_source, max_bytes)?;
    let extension = portable_extension(&canonical_source);
    let object_digest = resource_object_digest(&plaintext_sha256, session_key)?;
    let object_key = resource_object_key(category, &object_digest, &extension)?;
    Ok(ResourceDescriptor {
        category,
        source_path: canonical_source,
        object_key,
        plaintext_sha256,
        extension,
        size_bytes,
    })
}

/// Avoids uploading an already-present content-addressed object. The HEAD +
/// create-only PUT pair minimizes traffic while remaining safe under races.
pub fn ensure_resource_uploaded(
    store: &impl ObjectStore,
    resource: &ResourceDescriptor,
    session_key: Option<&SessionKey>,
) -> Result<ResourceUploadResult, String> {
    let stored_size_bytes = resource_stored_size(resource.size_bytes, session_key.is_some())?;
    if let Some(metadata) = store.head(&resource.object_key)? {
        validate_remote_size(&resource.object_key, stored_size_bytes, &metadata)?;
        return Ok(ResourceUploadResult {
            object_key: resource.object_key.clone(),
            size_bytes: stored_size_bytes,
            uploaded: false,
        });
    }

    let encrypted_upload = session_key
        .map(|key| encrypt_resource_to_temp(resource, key))
        .transpose()?;
    let (upload_path, upload_sha256, upload_size) = encrypted_upload.as_ref().map_or_else(
        || {
            (
                resource.source_path.as_path(),
                resource.plaintext_sha256.as_str(),
                resource.size_bytes,
            )
        },
        |upload| {
            (
                upload.path.as_path(),
                upload.sha256.as_str(),
                upload.size_bytes,
            )
        },
    );
    let uploaded = match store.put_file(
        &resource.object_key,
        upload_path,
        upload_sha256,
        upload_size,
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
            validate_remote_size(&resource.object_key, stored_size_bytes, &metadata)?;
            false
        }
    };

    Ok(ResourceUploadResult {
        object_key: resource.object_key.clone(),
        size_bytes: stored_size_bytes,
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
    session_key: Option<&SessionKey>,
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
            if resource_object_digest(&sha256, session_key)? == parsed.sha256 {
                return Ok(MaterializedResource {
                    path: final_path,
                    size_bytes,
                    transferred_bytes: 0,
                    reused_local_file: true,
                });
            }
        }
    }

    let download_path = category_root.join(format!(
        ".download-{}-{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    let stored_limit = resource_stored_size(max_bytes, session_key.is_some())?;
    let download = match store.get_to_file(object_key, &download_path, stored_limit) {
        Ok(Some(download)) => download,
        Ok(None) => {
            let _ = fs::remove_file(&download_path);
            return Err(format!("remote resource {object_key:?} does not exist"));
        }
        Err(error) => {
            let _ = fs::remove_file(&download_path);
            return Err(error);
        }
    };
    let (verified_path, plaintext_size) = if let Some(key) = session_key {
        let plaintext_path = category_root.join(format!(
            ".plaintext-{}-{:016x}.tmp",
            std::process::id(),
            rand::random::<u64>()
        ));
        match decrypt_resource_to_file(&download_path, &plaintext_path, object_key, key, max_bytes)
        {
            Ok(size_bytes) => {
                let _ = fs::remove_file(&download_path);
                (plaintext_path, size_bytes)
            }
            Err(error) => {
                let _ = fs::remove_file(&download_path);
                let _ = fs::remove_file(&plaintext_path);
                return Err(error);
            }
        }
    } else {
        if download.sha256 != parsed.sha256 {
            let _ = fs::remove_file(&download_path);
            return Err(format!(
                "remote resource digest mismatch for {object_key:?}: expected {}, got {}",
                parsed.sha256, download.sha256
            ));
        }
        if download.size_bytes > max_bytes {
            let _ = fs::remove_file(&download_path);
            return Err(format!(
                "remote resource {object_key:?} exceeds the {max_bytes}-byte limit"
            ));
        }
        (download_path, download.size_bytes)
    };

    if let Err(error) = publish_verified_file(&verified_path, &final_path) {
        let _ = fs::remove_file(&verified_path);
        return Err(error);
    }

    Ok(MaterializedResource {
        path: final_path,
        size_bytes: plaintext_size,
        transferred_bytes: download.size_bytes,
        reused_local_file: false,
    })
}

/// Verifies an already materialized local file against a canonical resource
/// key without touching the object store. This lets records captured locally
/// (or materialized during an earlier request) reuse their existing path while
/// retaining the remote reference as the stable cache identity.
pub fn verify_local_resource(
    path: &Path,
    object_key: &str,
    max_bytes: u64,
    session_key: Option<&SessionKey>,
) -> Result<bool, String> {
    let parsed = parse_resource_key(object_key)?;
    let metadata = match symlink_metadata_if_exists(path)? {
        Some(metadata) => metadata,
        None => return Ok(false),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max_bytes {
        return Ok(false);
    }
    let (sha256, _) = hash_regular_file(path, max_bytes)?;
    Ok(resource_object_digest(&sha256, session_key)? == parsed.sha256)
}

/// Rewrites local managed paths in a mutation batch to canonical v1 resource
/// keys and uploads every distinct referenced object before the caller may
/// publish the enclosing snapshot or segment. Missing, external, or oversized
/// local files are cleared from portable storage-path fields rather than
/// leaking an unusable machine-local path to other devices.
pub fn prepare_mutation_resources(
    store: &impl ObjectStore,
    mutations: &mut MutationBatch,
    roots: &ResourceRoots,
    limits: ResourceLimits,
    session_key: Option<&SessionKey>,
) -> Result<ResourceTransferStats, String> {
    let mut descriptors = BTreeMap::<String, ResourceDescriptor>::new();
    let mut skipped_resources = 0u64;

    for replicated in &mut mutations.upserts {
        let item = &mut replicated.item;
        let mut path_map = BTreeMap::<String, Option<String>>::new();
        match item.kind {
            SyncItemKind::Image => {
                item.resource_path = rewrite_outgoing_path(
                    item.resource_path.as_deref(),
                    &roots.images,
                    ResourceCategory::Image,
                    limits.image_bytes,
                    &mut descriptors,
                    &mut path_map,
                    &mut skipped_resources,
                    session_key,
                );
                item.preview_path = None;
            }
            SyncItemKind::File => {
                item.resource_path = rewrite_outgoing_path(
                    item.resource_path.as_deref(),
                    &roots.files,
                    ResourceCategory::File,
                    limits.file_bytes,
                    &mut descriptors,
                    &mut path_map,
                    &mut skipped_resources,
                    session_key,
                );
                if let Some(json) = item.text_content.as_deref() {
                    if let Ok(local_paths) = serde_json::from_str::<Vec<String>>(json) {
                        let portable_paths = local_paths
                            .iter()
                            .filter_map(|local_path| {
                                rewrite_outgoing_path(
                                    Some(local_path),
                                    &roots.files,
                                    ResourceCategory::File,
                                    limits.file_bytes,
                                    &mut descriptors,
                                    &mut path_map,
                                    &mut skipped_resources,
                                    session_key,
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
            SyncItemKind::Text | SyncItemKind::Link => {}
        }

        item.icon_path = rewrite_outgoing_icon(
            item.icon_path.as_deref(),
            roots,
            limits.icon_bytes,
            &mut descriptors,
            &mut path_map,
            &mut skipped_resources,
            session_key,
        );
        rewrite_metadata_paths(item.metadata_json.as_mut(), &path_map, true)?;
        remove_preview_metadata(item.metadata_json.as_mut())?;
    }

    let mut stats = ResourceTransferStats {
        referenced_resources: descriptors.len() as u64,
        skipped_resources,
        ..ResourceTransferStats::default()
    };
    for descriptor in descriptors.values() {
        let result = ensure_resource_uploaded(store, descriptor, session_key)?;
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
    roots: &ResourceRoots,
    limits: ResourceLimits,
    session_key: Option<&SessionKey>,
) -> Result<ResourceTransferStats, String> {
    let mut materialized = BTreeMap::<String, String>::new();
    let mut stats = ResourceTransferStats::default();

    for replicated in &mut mutations.upserts {
        let item = &mut replicated.item;
        match item.kind {
            SyncItemKind::Image => {
                item.resource_path = rewrite_incoming_path(
                    store,
                    item.resource_path.as_deref(),
                    &[ResourceCategory::Image],
                    roots,
                    limits,
                    false,
                    &mut materialized,
                    &mut stats,
                    session_key,
                )?;
                item.preview_path = None;
            }
            SyncItemKind::File => {
                item.resource_path = rewrite_incoming_path(
                    store,
                    item.resource_path.as_deref(),
                    &[ResourceCategory::File],
                    roots,
                    limits,
                    false,
                    &mut materialized,
                    &mut stats,
                    session_key,
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
                                    roots,
                                    limits,
                                    false,
                                    &mut materialized,
                                    &mut stats,
                                    session_key,
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
            SyncItemKind::Text | SyncItemKind::Link => {}
        }

        item.icon_path = rewrite_incoming_path(
            store,
            item.icon_path.as_deref(),
            &[ResourceCategory::Icon],
            roots,
            limits,
            true,
            &mut materialized,
            &mut stats,
            session_key,
        )?;
        rewrite_metadata_paths(
            item.metadata_json.as_mut(),
            &materialized_map(&materialized),
            false,
        )?;
        remove_preview_metadata(item.metadata_json.as_mut())?;
    }
    stats.referenced_resources = materialized.len() as u64;
    Ok(stats)
}

/// Replaces untrusted portable resource keys with absent local paths and
/// returns the compact references that must be committed beside the item.
/// This performs no object-store I/O.
pub fn defer_mutation_resources(
    mutations: &mut MutationBatch,
) -> Result<BTreeMap<String, Vec<SyncResourceRef>>, String> {
    let mut pending = BTreeMap::new();
    for replicated in &mut mutations.upserts {
        let item = &mut replicated.item;
        let mut references = Vec::new();
        let mut path_map = BTreeMap::<String, Option<String>>::new();

        match item.kind {
            SyncItemKind::Image => {
                if let Some(object_key) = take_portable_resource(
                    item.resource_path.take(),
                    &[ResourceCategory::Image],
                    "image",
                    0,
                    &mut references,
                    &mut path_map,
                )? {
                    path_map.insert(object_key, None);
                }
                item.resource_path = None;
                item.preview_path = None;
            }
            SyncItemKind::File => {
                let primary = item.resource_path.take();
                item.resource_path = None;

                if let Some(json) = item.text_content.as_deref() {
                    if let Ok(portable_paths) = serde_json::from_str::<Vec<String>>(json) {
                        if primary.as_deref().is_some_and(|primary| {
                            portable_paths.first().is_some_and(|first| first != primary)
                        }) {
                            return Err(
                                "file resource_path does not match the first portable path"
                                    .to_string(),
                            );
                        }
                        let mut local_paths = Vec::new();
                        for (index, portable_path) in portable_paths.into_iter().enumerate() {
                            let ordinal = u32::try_from(index)
                                .map_err(|_| "file resource ordinal overflowed".to_string())?;
                            if let Some(object_key) = take_portable_resource(
                                Some(portable_path),
                                &[ResourceCategory::File],
                                "file",
                                ordinal,
                                &mut references,
                                &mut path_map,
                            )? {
                                path_map.insert(object_key, None);
                            }
                            local_paths.push(String::new());
                        }
                        item.text_content =
                            Some(serde_json::to_string(&local_paths).map_err(|error| {
                                format!("failed to encode deferred file paths: {error}")
                            })?);
                    } else if let Some(object_key) = take_portable_resource(
                        primary,
                        &[ResourceCategory::File],
                        "file",
                        0,
                        &mut references,
                        &mut path_map,
                    )? {
                        path_map.insert(object_key, None);
                    }
                } else if let Some(object_key) = take_portable_resource(
                    primary,
                    &[ResourceCategory::File],
                    "file",
                    0,
                    &mut references,
                    &mut path_map,
                )? {
                    path_map.insert(object_key, None);
                }
            }
            SyncItemKind::Text | SyncItemKind::Link => {}
        }

        if let Some(object_key) = take_portable_resource(
            item.icon_path.take(),
            &[ResourceCategory::Icon],
            "icon",
            0,
            &mut references,
            &mut path_map,
        )? {
            path_map.insert(object_key, None);
        }
        item.icon_path = None;
        rewrite_metadata_paths(item.metadata_json.as_mut(), &path_map, true)?;
        remove_preview_metadata(item.metadata_json.as_mut())?;
        if !references.is_empty() {
            pending.insert(item.id.clone(), references);
        }
    }
    Ok(pending)
}

/// Collects canonical resource references from a portable mutation batch
/// without changing its wire fields. Call this after outgoing preparation so
/// publication state can retain the remote keys even if local files disappear.
pub fn collect_mutation_resource_refs(
    mutations: &MutationBatch,
) -> Result<BTreeMap<String, Vec<SyncResourceRef>>, String> {
    let mut references_by_item = BTreeMap::new();
    for replicated in &mutations.upserts {
        let item = &replicated.item;
        let mut references = Vec::new();
        match item.kind {
            SyncItemKind::Image => collect_resource_ref(
                item.resource_path.as_deref(),
                ResourceCategory::Image,
                "image",
                0,
                &mut references,
            )?,
            SyncItemKind::File => {
                let portable_paths = item
                    .text_content
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok());
                if let Some(paths) = portable_paths {
                    for (index, path) in paths.iter().enumerate() {
                        let ordinal = u32::try_from(index)
                            .map_err(|_| "file resource ordinal overflowed".to_string())?;
                        collect_resource_ref(
                            Some(path),
                            ResourceCategory::File,
                            "file",
                            ordinal,
                            &mut references,
                        )?;
                    }
                } else {
                    collect_resource_ref(
                        item.resource_path.as_deref(),
                        ResourceCategory::File,
                        "file",
                        0,
                        &mut references,
                    )?;
                }
            }
            SyncItemKind::Text | SyncItemKind::Link => {}
        }
        collect_resource_ref(
            item.icon_path.as_deref(),
            ResourceCategory::Icon,
            "icon",
            0,
            &mut references,
        )?;
        if !references.is_empty() {
            references_by_item.insert(item.id.clone(), references);
        }
    }
    Ok(references_by_item)
}

fn collect_resource_ref(
    value: Option<&str>,
    expected: ResourceCategory,
    slot: &str,
    ordinal: u32,
    references: &mut Vec<SyncResourceRef>,
) -> Result<(), String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let parsed = parse_resource_key(value)?;
    if parsed.category != expected {
        return Err(format!(
            "resource {value:?} has category {:?}, expected {expected:?}",
            parsed.category
        ));
    }
    references.push(SyncResourceRef {
        slot: slot.to_string(),
        ordinal,
        object_key: value.to_string(),
    });
    Ok(())
}

fn take_portable_resource(
    value: Option<String>,
    allowed_categories: &[ResourceCategory],
    slot: &str,
    ordinal: u32,
    references: &mut Vec<SyncResourceRef>,
    path_map: &mut BTreeMap<String, Option<String>>,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let parsed = parse_resource_key(&value)?;
    if !allowed_categories.contains(&parsed.category) {
        return Err(format!(
            "resource {value:?} has category {:?}, expected one of {allowed_categories:?}",
            parsed.category
        ));
    }
    if !references
        .iter()
        .any(|pending| pending.slot == slot && pending.ordinal == ordinal)
    {
        references.push(SyncResourceRef {
            slot: slot.to_string(),
            ordinal,
            object_key: value.clone(),
        });
    }
    path_map.insert(value.clone(), None);
    Ok(Some(value))
}

#[allow(clippy::too_many_arguments)]
fn rewrite_outgoing_path(
    value: Option<&str>,
    managed_root: &Path,
    category: ResourceCategory,
    max_bytes: u64,
    descriptors: &mut BTreeMap<String, ResourceDescriptor>,
    path_map: &mut BTreeMap<String, Option<String>>,
    skipped_resources: &mut u64,
    session_key: Option<&SessionKey>,
) -> Option<String> {
    let value = value?.to_string();
    if let Some(existing) = path_map.get(&value) {
        return existing.clone();
    }
    if let Ok(parsed) = parse_resource_key(&value) {
        if parsed.category == category {
            path_map.insert(value.clone(), Some(value.clone()));
            return Some(value);
        }
        *skipped_resources = skipped_resources.saturating_add(1);
        path_map.insert(value, None);
        return None;
    }
    let descriptor = fingerprint_resource(
        managed_root,
        Path::new(&value),
        category,
        max_bytes,
        session_key,
    );
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
    roots: &ResourceRoots,
    max_bytes: u64,
    descriptors: &mut BTreeMap<String, ResourceDescriptor>,
    path_map: &mut BTreeMap<String, Option<String>>,
    skipped_resources: &mut u64,
    session_key: Option<&SessionKey>,
) -> Option<String> {
    let value = value?;
    if let Ok(parsed) = parse_resource_key(value) {
        if parsed.category == ResourceCategory::Icon {
            path_map.insert(value.to_string(), Some(value.to_string()));
            return Some(value.to_string());
        }
        path_map.insert(value.to_string(), None);
        *skipped_resources = skipped_resources.saturating_add(1);
        return None;
    }
    let icon_root = &roots.icons;
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
        icon_root,
        ResourceCategory::Icon,
        max_bytes,
        descriptors,
        path_map,
        skipped_resources,
        session_key,
    )
}

#[allow(clippy::too_many_arguments)]
fn rewrite_incoming_path(
    store: &impl ObjectStore,
    value: Option<&str>,
    allowed_categories: &[ResourceCategory],
    roots: &ResourceRoots,
    limits: ResourceLimits,
    bare_file_name: bool,
    materialized: &mut BTreeMap<String, String>,
    stats: &mut ResourceTransferStats,
    session_key: Option<&SessionKey>,
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
    let (destination_root, max_bytes) = resource_destination(roots, parsed.category, limits);
    let result = materialize_resource(store, value, &destination_root, max_bytes, session_key)?;
    let local = result.path.to_string_lossy().to_string();
    if !result.reused_local_file {
        stats.transferred_resources += 1;
        stats.transferred_bytes = stats
            .transferred_bytes
            .checked_add(result.transferred_bytes)
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
    roots: &ResourceRoots,
    category: ResourceCategory,
    limits: ResourceLimits,
) -> (PathBuf, u64) {
    match category {
        ResourceCategory::Image => (roots.images.clone(), limits.image_bytes),
        ResourceCategory::File => (roots.files.clone(), limits.file_bytes),
        ResourceCategory::Icon => (roots.icons.clone(), limits.icon_bytes),
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

fn remove_preview_metadata(metadata_json: Option<&mut String>) -> Result<(), String> {
    let Some(metadata_json) = metadata_json else {
        return Ok(());
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(metadata_json) else {
        return Ok(());
    };
    remove_preview_keys(&mut value);
    *metadata_json = serde_json::to_string(&value)
        .map_err(|error| format!("failed to remove preview metadata: {error}"))?;
    Ok(())
}

fn remove_preview_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.remove("previewPath");
            for child in object.values_mut() {
                remove_preview_keys(child);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                remove_preview_keys(child);
            }
        }
        _ => {}
    }
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

fn resource_object_digest(
    plaintext_sha256: &str,
    session_key: Option<&SessionKey>,
) -> Result<String, String> {
    let digest = hex::decode(plaintext_sha256)
        .map_err(|error| format!("resource SHA-256 is not hexadecimal: {error}"))?;
    let digest: [u8; 32] = digest
        .try_into()
        .map_err(|_| "resource SHA-256 must contain 32 bytes".to_string())?;
    Ok(session_key.map_or_else(
        || plaintext_sha256.to_string(),
        |key| key.resource_digest(&digest),
    ))
}

fn resource_stored_size(plaintext_size: u64, encrypted: bool) -> Result<u64, String> {
    if !encrypted {
        return Ok(plaintext_size);
    }
    let chunk_size = RESOURCE_CHUNK_BYTES as u64;
    let chunks = plaintext_size
        .checked_add(chunk_size.saturating_sub(1))
        .ok_or_else(|| "resource chunk count overflowed".to_string())?
        / chunk_size;
    (RESOURCE_HEADER_LEN as u64)
        .checked_add(plaintext_size)
        .and_then(|size| size.checked_add(chunks.saturating_mul(RESOURCE_AUTH_TAG_LEN as u64)))
        .ok_or_else(|| "encrypted resource size overflowed".to_string())
}

fn encrypt_resource_to_temp(
    resource: &ResourceDescriptor,
    session_key: &SessionKey,
) -> Result<EncryptedResourceTemp, String> {
    let temp_path = std::env::temp_dir().join(format!(
        ".sync-encrypted-{}-{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    let result = (|| {
        let mut source = File::open(&resource.source_path)
            .map_err(|error| format!("failed to open resource for encryption: {error}"))?;
        let metadata = source
            .metadata()
            .map_err(|error| format!("failed to inspect resource for encryption: {error}"))?;
        if !metadata.is_file() || metadata.len() != resource.size_bytes {
            return Err("resource changed after fingerprinting".to_string());
        }
        let mut destination = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|error| format!("failed to create encrypted resource: {error}"))?;
        let header = resource_header(resource.size_bytes)?;
        destination
            .write_all(&header)
            .map_err(|error| format!("failed to write encrypted resource header: {error}"))?;
        let mut stored_hasher = Sha256::new();
        stored_hasher.update(header);
        let mut plaintext_hasher = Sha256::new();
        let mut plaintext_size = 0u64;
        let mut chunk_index = 0u64;
        let mut buffer = vec![0u8; RESOURCE_CHUNK_BYTES];
        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|error| format!("failed to read resource for encryption: {error}"))?;
            if read == 0 {
                break;
            }
            plaintext_size = plaintext_size
                .checked_add(read as u64)
                .ok_or_else(|| "resource plaintext size overflowed".to_string())?;
            if plaintext_size > resource.size_bytes {
                return Err("resource grew while it was encrypted".to_string());
            }
            plaintext_hasher.update(&buffer[..read]);
            let encrypted = session_key.encrypt_resource_chunk(
                &header,
                &resource.object_key,
                chunk_index,
                &buffer[..read],
            )?;
            destination
                .write_all(&encrypted)
                .map_err(|error| format!("failed to write encrypted resource: {error}"))?;
            stored_hasher.update(&encrypted);
            chunk_index = chunk_index
                .checked_add(1)
                .ok_or_else(|| "resource chunk index overflowed".to_string())?;
        }
        destination
            .flush()
            .map_err(|error| format!("failed to flush encrypted resource: {error}"))?;
        if plaintext_size != resource.size_bytes
            || hex::encode(plaintext_hasher.finalize()) != resource.plaintext_sha256
        {
            return Err("resource changed while it was encrypted".to_string());
        }
        let size_bytes = resource_stored_size(resource.size_bytes, true)?;
        let actual_size = destination
            .metadata()
            .map_err(|error| format!("failed to inspect encrypted resource: {error}"))?
            .len();
        if actual_size != size_bytes {
            return Err("encrypted resource size does not match its format".to_string());
        }
        Ok(EncryptedResourceTemp {
            path: temp_path.clone(),
            sha256: hex::encode(stored_hasher.finalize()),
            size_bytes,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn decrypt_resource_to_file(
    encrypted_path: &Path,
    plaintext_path: &Path,
    object_key: &str,
    session_key: &SessionKey,
    max_bytes: u64,
) -> Result<u64, String> {
    let parsed = parse_resource_key(object_key)?;
    let mut source = File::open(encrypted_path)
        .map_err(|error| format!("failed to open encrypted resource: {error}"))?;
    let source_size = source
        .metadata()
        .map_err(|error| format!("failed to inspect encrypted resource: {error}"))?
        .len();
    let mut header = [0u8; RESOURCE_HEADER_LEN];
    source
        .read_exact(&mut header)
        .map_err(|error| format!("failed to read encrypted resource header: {error}"))?;
    let plaintext_size = validate_resource_header(&header)?;
    if plaintext_size > max_bytes {
        return Err(format!(
            "remote resource {object_key:?} exceeds the {max_bytes}-byte limit"
        ));
    }
    if source_size != resource_stored_size(plaintext_size, true)? {
        return Err("encrypted resource size does not match its header".to_string());
    }

    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(plaintext_path)
        .map_err(|error| format!("failed to create decrypted resource: {error}"))?;
    let mut plaintext_hasher = Sha256::new();
    let mut remaining = plaintext_size;
    let mut chunk_index = 0u64;
    while remaining > 0 {
        let chunk_plaintext_size = remaining.min(RESOURCE_CHUNK_BYTES as u64) as usize;
        let mut ciphertext = vec![0u8; chunk_plaintext_size + RESOURCE_AUTH_TAG_LEN];
        source
            .read_exact(&mut ciphertext)
            .map_err(|error| format!("failed to read encrypted resource chunk: {error}"))?;
        let plaintext =
            session_key.decrypt_resource_chunk(&header, object_key, chunk_index, &ciphertext)?;
        if plaintext.len() != chunk_plaintext_size {
            return Err("decrypted resource chunk has an invalid size".to_string());
        }
        destination
            .write_all(&plaintext)
            .map_err(|error| format!("failed to write decrypted resource: {error}"))?;
        plaintext_hasher.update(&plaintext);
        remaining -= chunk_plaintext_size as u64;
        chunk_index = chunk_index
            .checked_add(1)
            .ok_or_else(|| "resource chunk index overflowed".to_string())?;
    }
    destination
        .flush()
        .map_err(|error| format!("failed to flush decrypted resource: {error}"))?;
    let plaintext_sha256 = hex::encode(plaintext_hasher.finalize());
    if resource_object_digest(&plaintext_sha256, Some(session_key))? != parsed.sha256 {
        return Err("decrypted resource digest does not match its object key".to_string());
    }
    Ok(plaintext_size)
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
    use crate::v1::{
        remote::{DownloadedFile, DownloadedObject, ObjectInfo},
        MutationBatch, RecordVersion, ReplicatedItem, SyncItem, SyncItemKind,
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
                    etag: Some(format!("\"{}\"", hex::encode(Sha256::digest(bytes)))),
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

    fn sample_item(id: &str, kind: SyncItemKind) -> ReplicatedItem {
        ReplicatedItem {
            item: SyncItem {
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

        let resource = fingerprint_resource(
            &managed,
            &source,
            ResourceCategory::File,
            3 * 1024 * 1024,
            None,
        )
        .unwrap();
        assert_eq!(resource.extension, "data");
        assert_eq!(resource.size_bytes, bytes.len() as u64);

        let store = MemoryStore::default();
        assert!(
            ensure_resource_uploaded(&store, &resource, None)
                .unwrap()
                .uploaded
        );
        assert!(
            !ensure_resource_uploaded(&store, &resource, None)
                .unwrap()
                .uploaded
        );
        assert_eq!(store.file_puts.get(), 1);
        assert_eq!(store.objects.borrow().len(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn encrypted_resources_are_deterministic_private_and_password_scoped() {
        let root = temporary_directory("encrypted-upload");
        let managed = root.join("managed");
        fs::create_dir_all(&managed).unwrap();
        let source = managed.join("private.bin");
        let plaintext = b"clipboard secret body".repeat(96 * 1024);
        fs::write(&source, &plaintext).unwrap();
        let first_key = SessionKey::derive("password", "scope-a").unwrap();
        let same_key = SessionKey::derive("password", "scope-a").unwrap();
        let other_key = SessionKey::derive("different", "scope-a").unwrap();

        let first = fingerprint_resource(
            &managed,
            &source,
            ResourceCategory::File,
            plaintext.len() as u64,
            Some(&first_key),
        )
        .unwrap();
        let same = fingerprint_resource(
            &managed,
            &source,
            ResourceCategory::File,
            plaintext.len() as u64,
            Some(&same_key),
        )
        .unwrap();
        let other = fingerprint_resource(
            &managed,
            &source,
            ResourceCategory::File,
            plaintext.len() as u64,
            Some(&other_key),
        )
        .unwrap();
        assert_eq!(first.object_key, same.object_key);
        assert_ne!(first.object_key, other.object_key);

        let store = MemoryStore::default();
        let uploaded = ensure_resource_uploaded(&store, &first, Some(&first_key)).unwrap();
        assert!(uploaded.uploaded);
        let stored = store
            .objects
            .borrow()
            .get(&first.object_key)
            .cloned()
            .unwrap();
        assert_eq!(stored.len() as u64, uploaded.size_bytes);
        assert_ne!(stored, plaintext);
        assert!(!stored
            .windows(b"clipboard secret body".len())
            .any(|window| window == b"clipboard secret body"));

        let retry_store = MemoryStore::default();
        ensure_resource_uploaded(&retry_store, &same, Some(&same_key)).unwrap();
        assert_eq!(
            retry_store.objects.borrow().get(&same.object_key),
            Some(&stored)
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn encrypted_resource_round_trip_rejects_wrong_password_and_corruption() {
        let root = temporary_directory("encrypted-round-trip");
        let managed = root.join("managed");
        let cache = root.join("cache");
        let wrong_cache = root.join("wrong-cache");
        let corrupt_cache = root.join("corrupt-cache");
        fs::create_dir_all(&managed).unwrap();
        let source = managed.join("private.bin");
        let plaintext = b"authenticated remote resource".repeat(80 * 1024);
        fs::write(&source, &plaintext).unwrap();
        let key = SessionKey::derive("password", "scope-a").unwrap();
        let wrong = SessionKey::derive("wrong", "scope-a").unwrap();
        let descriptor = fingerprint_resource(
            &managed,
            &source,
            ResourceCategory::Image,
            plaintext.len() as u64,
            Some(&key),
        )
        .unwrap();
        let store = MemoryStore::default();
        ensure_resource_uploaded(&store, &descriptor, Some(&key)).unwrap();

        let materialized = materialize_resource(
            &store,
            &descriptor.object_key,
            &cache,
            plaintext.len() as u64,
            Some(&key),
        )
        .unwrap();
        assert_eq!(fs::read(&materialized.path).unwrap(), plaintext);
        assert!(verify_local_resource(
            &materialized.path,
            &descriptor.object_key,
            plaintext.len() as u64,
            Some(&key),
        )
        .unwrap());
        assert!(!verify_local_resource(
            &materialized.path,
            &descriptor.object_key,
            plaintext.len() as u64,
            Some(&wrong),
        )
        .unwrap());

        assert!(materialize_resource(
            &store,
            &descriptor.object_key,
            &wrong_cache,
            plaintext.len() as u64,
            Some(&wrong),
        )
        .is_err());
        assert!(fs::read_dir(&wrong_cache).unwrap().next().is_none());

        let corrupt_store = MemoryStore::default();
        let mut corrupted = store
            .objects
            .borrow()
            .get(&descriptor.object_key)
            .cloned()
            .unwrap();
        let middle = corrupted.len() / 2;
        corrupted[middle] ^= 0x80;
        corrupt_store
            .objects
            .borrow_mut()
            .insert(descriptor.object_key.clone(), corrupted);
        assert!(materialize_resource(
            &corrupt_store,
            &descriptor.object_key,
            &corrupt_cache,
            plaintext.len() as u64,
            Some(&key),
        )
        .is_err());
        assert!(fs::read_dir(&corrupt_cache).unwrap().next().is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn encrypted_empty_resource_round_trips_without_phantom_chunks() {
        let root = temporary_directory("encrypted-empty");
        let managed = root.join("managed");
        let cache = root.join("cache");
        fs::create_dir_all(&managed).unwrap();
        let source = managed.join("empty.bin");
        fs::write(&source, []).unwrap();
        let key = SessionKey::derive("password", "scope-a").unwrap();
        let descriptor =
            fingerprint_resource(&managed, &source, ResourceCategory::File, 0, Some(&key)).unwrap();
        let store = MemoryStore::default();
        let uploaded = ensure_resource_uploaded(&store, &descriptor, Some(&key)).unwrap();
        assert_eq!(uploaded.size_bytes, RESOURCE_HEADER_LEN as u64);
        let materialized =
            materialize_resource(&store, &descriptor.object_key, &cache, 0, Some(&key)).unwrap();
        assert_eq!(materialized.size_bytes, 0);
        assert_eq!(materialized.transferred_bytes, RESOURCE_HEADER_LEN as u64);
        assert!(fs::read(materialized.path).unwrap().is_empty());

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

        let first = materialize_resource(&store, &key, &cache, 1024 * 1024, None).unwrap();
        assert!(!first.reused_local_file);
        assert_eq!(fs::read(&first.path).unwrap(), bytes);
        let second = materialize_resource(&store, &key, &cache, 1024 * 1024, None).unwrap();
        assert!(second.reused_local_file);
        assert_eq!(store.file_gets.get(), 1);

        fs::write(&first.path, b"corrupt").unwrap();
        let repaired = materialize_resource(&store, &key, &cache, 1024 * 1024, None).unwrap();
        assert!(!repaired.reused_local_file);
        assert_eq!(fs::read(&repaired.path).unwrap(), bytes);
        assert_eq!(store.file_gets.get(), 2);

        fs::write(&first.path, vec![0u8; 2 * 1024 * 1024]).unwrap();
        let repaired_oversized =
            materialize_resource(&store, &key, &cache, 1024 * 1024, None).unwrap();
        assert!(!repaired_oversized.reused_local_file);
        assert_eq!(fs::read(&repaired_oversized.path).unwrap(), bytes);
        assert_eq!(store.file_gets.get(), 3);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_verification_rejects_missing_corrupt_and_oversized_files() {
        let root = temporary_directory("verify-local");
        let path = root.join("cached.bin");
        fs::write(&path, b"expected").unwrap();
        let key = resource_object_key(
            ResourceCategory::File,
            &hex::encode(Sha256::digest(b"expected")),
            "bin",
        )
        .unwrap();

        assert!(verify_local_resource(&path, &key, 1024, None).unwrap());
        fs::write(&path, b"corrupt").unwrap();
        assert!(!verify_local_resource(&path, &key, 1024, None).unwrap());
        assert!(!verify_local_resource(&path, &key, 3, None).unwrap());
        fs::remove_file(&path).unwrap();
        assert!(!verify_local_resource(&path, &key, 1024, None).unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fingerprint_rejects_outside_and_oversized_files() {
        let root = temporary_directory("bounds");
        let managed = root.join("managed");
        fs::create_dir_all(&managed).unwrap();
        let outside = root.join("outside.bin");
        fs::write(&outside, vec![0u8; 32]).unwrap();
        assert!(
            fingerprint_resource(&managed, &outside, ResourceCategory::File, 1024, None).is_err()
        );

        let oversized = managed.join("oversized.bin");
        fs::write(&oversized, vec![0u8; 2048]).unwrap();
        assert!(
            fingerprint_resource(&managed, &oversized, ResourceCategory::File, 1024, None).is_err()
        );

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

        assert!(materialize_resource(&store, &key, &cache, 1024, None).is_err());
        let parsed = parse_resource_key(&key).unwrap();
        let expected_path = cache.join(format!("sha256-{}.{}", parsed.sha256, parsed.extension));
        assert!(!expected_path.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mutation_resources_round_trip_without_machine_local_paths() {
        let root = temporary_directory("mutation-round-trip");
        let source_root = root.join("source");
        let target_root = root.join("target");
        let source_roots = ResourceRoots::new(
            source_root.join("images"),
            source_root.join("files"),
            source_root.join("icons"),
        );
        let target_roots = ResourceRoots::new(
            target_root.join("images"),
            target_root.join("files"),
            target_root.join("icons"),
        );
        for directory in [
            &source_roots.images,
            &source_roots.files,
            &source_roots.icons,
            &target_roots.images,
            &target_roots.files,
            &target_roots.icons,
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        let image_path = source_roots.images.join("image.png");
        let preview_path = source_root.join("previews/image.jpg");
        let file_path = source_roots.files.join("document.txt");
        fs::create_dir_all(preview_path.parent().unwrap()).unwrap();
        fs::write(&image_path, b"image-bytes").unwrap();
        fs::write(&preview_path, b"preview-bytes").unwrap();
        fs::write(&file_path, b"file-bytes").unwrap();
        fs::write(source_roots.icons.join("app.png"), b"icon-bytes").unwrap();

        let mut image = sample_item("image", SyncItemKind::Image);
        image.item.resource_path = Some(image_path.to_string_lossy().to_string());
        image.item.preview_path = Some(preview_path.to_string_lossy().to_string());
        image.item.icon_path = Some("app.png".to_string());
        image.item.metadata_json = Some(
            serde_json::json!({
                "resourcePath": image_path,
                "storagePath": image_path,
                "previewPath": preview_path,
            })
            .to_string(),
        );

        let mut file = sample_item("file", SyncItemKind::File);
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
            prepare_mutation_resources(&store, &mut batch, &source_roots, limits, None).unwrap();
        assert_eq!(uploaded.referenced_resources, 3);
        assert_eq!(uploaded.transferred_resources, 3);
        assert!(batch.upserts.iter().all(|item| {
            item.item
                .resource_path
                .as_deref()
                .is_none_or(|path| path.starts_with("v1/resources/"))
        }));
        assert!(batch.upserts[0].item.preview_path.is_none());
        assert!(!store
            .objects
            .borrow()
            .keys()
            .any(|key| key.starts_with("v1/resources/preview/")));
        assert!(batch.upserts[0]
            .item
            .icon_path
            .as_deref()
            .unwrap()
            .starts_with("v1/resources/icon/"));

        let downloaded =
            materialize_mutation_resources(&store, &mut batch, &target_roots, limits, None)
                .unwrap();
        assert_eq!(downloaded.referenced_resources, 3);
        assert_eq!(downloaded.transferred_resources, 3);
        assert!(batch.upserts[0].item.preview_path.is_none());
        let target_images = fs::canonicalize(&target_roots.images).unwrap();
        let target_files = fs::canonicalize(&target_roots.files).unwrap();
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
        let image_metadata: serde_json::Value =
            serde_json::from_str(batch.upserts[0].item.metadata_json.as_deref().unwrap()).unwrap();
        assert!(image_metadata.get("previewPath").is_none());

        fs::remove_dir_all(root).unwrap();
    }
}
