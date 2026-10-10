//! Mutation-batch resource planning, rewriting and reference collection.

use super::*;

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
        crate::cancellation::check()?;
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
        crate::cancellation::check()?;
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
        crate::cancellation::check()?;
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
        crate::cancellation::check()?;
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
                    // An empty portable-path list must fall back to the
                    // primary `resource_path`; otherwise the file resource is
                    // dropped and peers cannot materialize it.
                    let portable_paths = serde_json::from_str::<Vec<String>>(json)
                        .ok()
                        .filter(|paths| !paths.is_empty());
                    if let Some(portable_paths) = portable_paths {
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
        crate::cancellation::check()?;
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
                // Empty portable-path lists fall back to `resource_path`.
                let portable_paths = item
                    .text_content
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
                    .filter(|paths| !paths.is_empty());
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
