use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::wire::OplogResource;
use crate::storage::{StoragePaths, SyncChangeLogEntry};

/// Fetch callback used to download a pooled resource (`bytes: None`) from the
/// remote pool when materializing a payload on a device that lacks the file.
pub type PoolFetcher<'a> = dyn Fn(&str) -> Result<Vec<u8>, String> + 'a;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourceCategory {
    Image,
    Preview,
    File,
    Icon,
}

impl ResourceCategory {
    fn from_wire(value: &str) -> Option<Self> {
        match value {
            "image" => Some(Self::Image),
            "preview" => Some(Self::Preview),
            "file" => Some(Self::File),
            "icon" => Some(Self::Icon),
            _ => None,
        }
    }

    fn wire_name(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Preview => "preview",
            Self::File => "file",
            Self::Icon => "icon",
        }
    }

    fn base(self, paths: &StoragePaths) -> PathBuf {
        match self {
            Self::Image => paths.images.clone(),
            Self::Preview => paths.previews.clone(),
            Self::File => paths.files.clone(),
            Self::Icon => paths.storage.join("icons"),
        }
    }
}

struct ParsedWirePath<'a> {
    category: ResourceCategory,
    segments: Vec<&'a str>,
    canonical_digest: Option<&'a str>,
}

impl ParsedWirePath<'_> {
    fn relative_path(&self) -> PathBuf {
        self.segments.iter().collect()
    }

    fn file_name(&self) -> &str {
        self.segments
            .last()
            .expect("validated wire paths always have a file name")
    }
}

fn parse_wire_path(wire: &str) -> Result<ParsedWirePath<'_>, String> {
    if wire.is_empty()
        || wire.len() > 1024
        || Path::new(wire).is_absolute()
        || wire.contains('\\')
        || wire.contains(':')
        || wire.chars().any(char::is_control)
    {
        return Err(format!("invalid sync resource path {wire:?}"));
    }

    let mut parts = wire.split('/');
    let category_name = parts.next().unwrap_or_default();
    let category = ResourceCategory::from_wire(category_name)
        .ok_or_else(|| format!("unknown sync resource category in {wire:?}"))?;
    let segments = parts.collect::<Vec<_>>();
    if segments.is_empty() {
        return Err(format!("sync resource path has no file name: {wire:?}"));
    }
    if segments.len() > 32 {
        return Err(format!("sync resource path is too deep: {wire:?}"));
    }
    if category == ResourceCategory::Icon && segments.len() != 1 {
        return Err(format!(
            "icon sync paths must contain one file name: {wire:?}"
        ));
    }
    for segment in &segments {
        validate_wire_segment(segment, wire)?;
    }

    let canonical_digest = canonical_digest_from_name(
        segments
            .last()
            .expect("validated wire paths always have a file name"),
        wire,
    )?;
    Ok(ParsedWirePath {
        category,
        segments,
        canonical_digest,
    })
}

fn validate_wire_segment(segment: &str, wire: &str) -> Result<(), String> {
    if segment.is_empty()
        || segment.len() > 255
        || matches!(segment, "." | "..")
        || segment.trim_matches(' ') != segment
        || segment.ends_with('.')
        || segment.chars().any(|ch| {
            ch.is_control() || matches!(ch, '<' | '>' | '"' | '|' | '?' | '*' | '#' | '%')
        })
        || is_reserved_windows_name(segment)
    {
        return Err(format!("invalid sync resource path component in {wire:?}"));
    }
    Ok(())
}

fn is_reserved_windows_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn canonical_digest_from_name<'a>(name: &'a str, wire: &str) -> Result<Option<&'a str>, String> {
    let bytes = name.as_bytes();
    if bytes.len() < 7 || !bytes[..7].eq_ignore_ascii_case(b"sha256-") {
        return Ok(None);
    }

    let suffix = &name[7..];
    let (digest, extension) = suffix
        .split_once('.')
        .map_or((suffix, None), |(digest, extension)| {
            (digest, Some(extension))
        });
    let valid_extension = extension.is_none_or(|extension| {
        !extension.is_empty()
            && extension.len() <= 16
            && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
    });
    if !is_sha256_hex(digest) || !valid_extension {
        return Err(format!(
            "malformed content-addressed sync resource path {wire:?}"
        ));
    }
    Ok(Some(digest))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Validates a portable resource path before it is used in a local path,
/// remote object name, or pool manifest.
pub(crate) fn validate_resource_wire_path(wire: &str) -> Result<(), String> {
    parse_wire_path(wire).map(|_| ())
}

/// Verifies the integrity claim encoded in content-addressed and legacy
/// hash-named resource paths. Safe legacy names without a content digest are
/// accepted for compatibility.
pub(crate) fn validate_resource_bytes(wire: &str, bytes: &[u8]) -> Result<(), String> {
    let parsed = parse_wire_path(wire)?;
    validate_parsed_resource_bytes(wire, &parsed, bytes)
}

/// Rewrites absolute resource paths on `entries` to portable wire form
/// (`image/<rel>`, `file/<rel>`, `preview/<rel>`, `icon/<name>`) and collects
/// the referenced file bytes into inline `OplogResource`s.
///
/// Files that cannot be read (missing, unreadable) have their reference
/// cleared so the receiver never gets a dangling path. Resources are deduped
/// by wire path so a file shared by several entries is transferred once.
pub fn collect_item_resources(
    items: &[crate::domain::ClipboardItem],
    paths: &StoragePaths,
) -> (Vec<crate::domain::ClipboardItem>, Vec<OplogResource>) {
    use crate::domain::ClipboardKind;

    let mut seen = HashSet::new();
    let mut resources: Vec<OplogResource> = Vec::new();

    let items = items
        .iter()
        .cloned()
        .map(|mut item| {
            match item.kind {
                ClipboardKind::Image => {
                    if let Some(abs) = item.resource_path.take() {
                        if let Some((wire, bytes)) = resource_bytes(&abs, paths) {
                            if seen.insert(wire.clone()) {
                                resources.push(OplogResource {
                                    rel_path: wire.clone(),
                                    bytes: Some(bytes),
                                });
                            }
                            item.resource_path = Some(wire);
                        }
                    }
                    if let Some(abs) = item.preview_path.take() {
                        if let Some((wire, bytes)) = resource_bytes(&abs, paths) {
                            if seen.insert(wire.clone()) {
                                resources.push(OplogResource {
                                    rel_path: wire.clone(),
                                    bytes: Some(bytes),
                                });
                            }
                            item.preview_path = Some(wire);
                        }
                    }
                }
                ClipboardKind::File => {
                    if let Some(abs) = item.resource_path.take() {
                        if let Some((wire, bytes)) = resource_bytes(&abs, paths) {
                            if seen.insert(wire.clone()) {
                                resources.push(OplogResource {
                                    rel_path: wire.clone(),
                                    bytes: Some(bytes),
                                });
                            }
                            item.resource_path = Some(wire);
                        }
                    }
                    if let Some(json) = item.text_content.take() {
                        if let Ok(stored_paths) = serde_json::from_str::<Vec<String>>(&json) {
                            let mut wires = Vec::new();
                            for abs in stored_paths {
                                if let Some((wire, bytes)) = resource_bytes(&abs, paths) {
                                    if seen.insert(wire.clone()) {
                                        resources.push(OplogResource {
                                            rel_path: wire.clone(),
                                            bytes: Some(bytes),
                                        });
                                    }
                                    wires.push(wire);
                                }
                            }
                            item.text_content = serde_json::to_string(&wires).ok();
                        } else {
                            item.text_content = Some(json);
                        }
                    }
                }
                _ => {}
            }
            if let Some(icon) = item.icon_path.take() {
                if let Some((wire, bytes)) = icon_resource_bytes(&icon, paths) {
                    if seen.insert(wire.clone()) {
                        resources.push(OplogResource {
                            rel_path: wire.clone(),
                            bytes: Some(bytes),
                        });
                    }
                    item.icon_path = Some(wire);
                }
            }
            item
        })
        .collect();

    (items, resources)
}

/// Converts wire-form paths on `items` (from a downloaded baseline or merged
/// payload) back to local absolute paths so they point at this device's
/// storage. Malformed, absolute, or category-confused paths are rejected so
/// untrusted remote payloads cannot persist arbitrary local paths.
pub fn rewrite_item_paths_to_local(
    items: &mut [crate::domain::ClipboardItem],
    paths: &StoragePaths,
) -> Result<(), String> {
    use crate::domain::ClipboardKind;

    for item in items {
        match item.kind {
            ClipboardKind::Image => {
                if let Some(wire) = item.resource_path.as_deref() {
                    item.resource_path = Some(
                        wire_to_abs(wire, paths, &[ResourceCategory::Image])
                            .map_err(|error| format!("item {} resource path: {error}", item.id))?,
                    );
                }
                if let Some(wire) = item.preview_path.as_deref() {
                    item.preview_path = Some(
                        wire_to_abs(
                            wire,
                            paths,
                            &[ResourceCategory::Image, ResourceCategory::Preview],
                        )
                        .map_err(|error| format!("item {} preview path: {error}", item.id))?,
                    );
                }
            }
            ClipboardKind::File => {
                if let Some(wire) = item.resource_path.as_deref() {
                    item.resource_path = Some(
                        wire_to_abs(wire, paths, &[ResourceCategory::File])
                            .map_err(|error| format!("item {} resource path: {error}", item.id))?,
                    );
                }
                if let Some(json) = item.text_content.as_deref() {
                    if let Ok(wires) = serde_json::from_str::<Vec<String>>(json) {
                        let abs = wires
                            .into_iter()
                            .map(|wire| {
                                wire_to_abs(&wire, paths, &[ResourceCategory::File]).map_err(
                                    |error| format!("item {} file list path: {error}", item.id),
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        item.text_content =
                            Some(serde_json::to_string(&abs).map_err(|error| {
                                format!("failed to rewrite file paths: {error}")
                            })?);
                    }
                }
            }
            _ => {}
        }
        if let Some(wire) = item.icon_path.as_deref() {
            item.icon_path = Some(
                wire_to_icon(wire, paths)
                    .map_err(|error| format!("item {} icon path: {error}", item.id))?,
            );
        }
    }
    Ok(())
}
pub fn collect_entry_resources(
    entries: Vec<SyncChangeLogEntry>,
    paths: &StoragePaths,
) -> (Vec<SyncChangeLogEntry>, Vec<OplogResource>) {
    let mut seen = HashSet::new();
    let mut resources: Vec<OplogResource> = Vec::new();

    let entries = entries
        .into_iter()
        .map(|mut entry| {
            match entry.kind.as_str() {
                "image" => {
                    if let Some(abs) = entry.resource_path.take() {
                        if let Some((wire, bytes)) = resource_bytes(&abs, paths) {
                            if seen.insert(wire.clone()) {
                                resources.push(OplogResource {
                                    rel_path: wire.clone(),
                                    bytes: Some(bytes),
                                });
                            }
                            entry.resource_path = Some(wire);
                        }
                    }
                    if let Some(abs) = entry.preview_path.take() {
                        if let Some((wire, bytes)) = resource_bytes(&abs, paths) {
                            if seen.insert(wire.clone()) {
                                resources.push(OplogResource {
                                    rel_path: wire.clone(),
                                    bytes: Some(bytes),
                                });
                            }
                            entry.preview_path = Some(wire);
                        }
                    }
                }
                "file" => {
                    if let Some(abs) = entry.resource_path.take() {
                        if let Some((wire, bytes)) = resource_bytes(&abs, paths) {
                            if seen.insert(wire.clone()) {
                                resources.push(OplogResource {
                                    rel_path: wire.clone(),
                                    bytes: Some(bytes),
                                });
                            }
                            entry.resource_path = Some(wire);
                        }
                    }
                    if let Some(json) = entry.text_content.take() {
                        if let Ok(stored_paths) = serde_json::from_str::<Vec<String>>(&json) {
                            let mut wires = Vec::new();
                            for abs in stored_paths {
                                if let Some((wire, bytes)) = resource_bytes(&abs, paths) {
                                    if seen.insert(wire.clone()) {
                                        resources.push(OplogResource {
                                            rel_path: wire.clone(),
                                            bytes: Some(bytes),
                                        });
                                    }
                                    wires.push(wire);
                                }
                            }
                            entry.text_content = serde_json::to_string(&wires).ok();
                        } else {
                            entry.text_content = Some(json);
                        }
                    }
                }
                _ => {}
            }
            if let Some(icon) = entry.icon_path.take() {
                if let Some((wire, bytes)) = icon_resource_bytes(&icon, paths) {
                    if seen.insert(wire.clone()) {
                        resources.push(OplogResource {
                            rel_path: wire.clone(),
                            bytes: Some(bytes),
                        });
                    }
                    entry.icon_path = Some(wire);
                }
            }
            entry
        })
        .collect();

    (entries, resources)
}

/// Writes inline resource bytes back into the local storage subdirectories
/// according to their wire category.
///
/// Resources that reference the remote pool (`bytes: None`) are downloaded
/// through `fetch_pool` when no valid local cache file exists. Portable paths
/// are strictly validated before joining them to storage roots, nested/final
/// symlinks are rejected, and digest-bearing names are checked before bytes
/// reach disk.
pub fn materialize_resources(
    resources: &[OplogResource],
    paths: &StoragePaths,
    fetch_pool: Option<&PoolFetcher>,
) -> Result<(), String> {
    for resource in resources {
        let parsed = parse_wire_path(&resource.rel_path)?;
        if let Some(bytes) = &resource.bytes {
            validate_parsed_resource_bytes(&resource.rel_path, &parsed, bytes)?;
            let target = secure_target_path(&parsed, paths)?;
            fs::write(&target, bytes).map_err(|error| {
                format!(
                    "failed to write sync resource {} to {}: {error}",
                    resource.rel_path,
                    target.display()
                )
            })?;
            continue;
        }

        let target = secure_target_path(&parsed, paths)?;
        match read_existing_resource(&target, &resource.rel_path, &parsed) {
            Ok(Some(())) => continue,
            Ok(None) => {}
            Err(error) if fetch_pool.is_none() => return Err(error),
            Err(_) => {}
        }
        let fetch = fetch_pool.ok_or_else(|| {
            format!(
                "sync resource {} is missing locally and no pool fetcher is available",
                resource.rel_path
            )
        })?;
        let bytes = fetch(&resource.rel_path).map_err(|error| {
            format!(
                "failed to fetch pool resource {}: {error}",
                resource.rel_path
            )
        })?;
        validate_parsed_resource_bytes(&resource.rel_path, &parsed, &bytes)?;
        fs::write(&target, bytes).map_err(|error| {
            format!(
                "failed to write pooled sync resource {} to {}: {error}",
                resource.rel_path,
                target.display()
            )
        })?;
    }
    Ok(())
}

/// Converts wire-form paths on `entries` to local absolute paths (or, for
/// icons, back to the bare file name the frontend resolves via `icons`).
/// Invalid or category-confused paths abort the payload instead of being
/// persisted verbatim.
pub fn rewrite_to_local(
    entries: &mut [SyncChangeLogEntry],
    paths: &StoragePaths,
) -> Result<(), String> {
    for entry in entries {
        match entry.kind.as_str() {
            "image" => {
                if let Some(wire) = entry.resource_path.as_deref() {
                    entry.resource_path = Some(
                        wire_to_abs(wire, paths, &[ResourceCategory::Image]).map_err(|error| {
                            format!("item {} resource path: {error}", entry.item_id)
                        })?,
                    );
                }
                if let Some(wire) = entry.preview_path.as_deref() {
                    entry.preview_path = Some(
                        wire_to_abs(
                            wire,
                            paths,
                            &[ResourceCategory::Image, ResourceCategory::Preview],
                        )
                        .map_err(|error| format!("item {} preview path: {error}", entry.item_id))?,
                    );
                }
            }
            "file" => {
                if let Some(wire) = entry.resource_path.as_deref() {
                    entry.resource_path =
                        Some(wire_to_abs(wire, paths, &[ResourceCategory::File]).map_err(
                            |error| format!("item {} resource path: {error}", entry.item_id),
                        )?);
                }
                if let Some(json) = entry.text_content.as_deref() {
                    if let Ok(wires) = serde_json::from_str::<Vec<String>>(json) {
                        let abs = wires
                            .into_iter()
                            .map(|wire| {
                                wire_to_abs(&wire, paths, &[ResourceCategory::File]).map_err(
                                    |error| {
                                        format!("item {} file list path: {error}", entry.item_id)
                                    },
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        entry.text_content =
                            Some(serde_json::to_string(&abs).map_err(|error| {
                                format!("failed to rewrite file paths: {error}")
                            })?);
                    }
                }
            }
            _ => {}
        }
        if let Some(wire) = entry.icon_path.as_deref() {
            entry.icon_path = Some(
                wire_to_icon(wire, paths)
                    .map_err(|error| format!("item {} icon path: {error}", entry.item_id))?,
            );
        }
    }
    Ok(())
}

/// Reads a referenced file and produces its wire path + bytes.
/// Returns `None` when the path cannot be mapped to storage or read.
fn resource_bytes(abs_or_rel: &str, paths: &StoragePaths) -> Option<(String, Vec<u8>)> {
    let path = resolve_resource_path(abs_or_rel, paths);
    let canonical_path = fs::canonicalize(path).ok()?;
    let category = managed_resource_category(&canonical_path, paths)?;
    let bytes = fs::read(&canonical_path).ok()?;
    validate_managed_source_bytes(category, &canonical_path, &bytes).ok()?;
    Some((
        content_addressed_wire_path(category, &canonical_path, &bytes),
        bytes,
    ))
}

fn icon_resource_bytes(icon: &str, paths: &StoragePaths) -> Option<(String, Vec<u8>)> {
    let candidate_wire = format!("icon/{icon}");
    let parsed = parse_wire_path(&candidate_wire).ok()?;
    if parsed.category != ResourceCategory::Icon {
        return None;
    }

    let icons = paths.storage.join("icons");
    if fs::symlink_metadata(&icons)
        .ok()
        .is_some_and(|metadata| metadata.file_type().is_symlink())
    {
        return None;
    }
    let canonical_base = fs::canonicalize(&icons).ok()?;
    let canonical_path = fs::canonicalize(icons.join(icon)).ok()?;
    canonical_path.strip_prefix(&canonical_base).ok()?;
    let bytes = fs::read(&canonical_path).ok()?;
    validate_parsed_resource_bytes(&candidate_wire, &parsed, &bytes).ok()?;
    Some((
        content_addressed_wire_path(ResourceCategory::Icon, &canonical_path, &bytes),
        bytes,
    ))
}

fn resolve_resource_path(abs_or_rel: &str, paths: &StoragePaths) -> PathBuf {
    let path = Path::new(abs_or_rel);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.storage.join(path)
    }
}

fn managed_resource_category(path: &Path, paths: &StoragePaths) -> Option<ResourceCategory> {
    for category in [
        ResourceCategory::Preview,
        ResourceCategory::Image,
        ResourceCategory::File,
    ] {
        let base = fs::canonicalize(category.base(paths)).ok()?;
        if path.strip_prefix(base).is_ok() {
            return Some(category);
        }
    }
    None
}

fn content_addressed_wire_path(category: ResourceCategory, source: &Path, bytes: &[u8]) -> String {
    let digest = sha256_hex(bytes);
    let extension = safe_extension(source)
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    format!("{}/sha256-{}{}", category.wire_name(), digest, extension)
}

fn safe_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?;
    let extension = extension
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    (!extension.is_empty()).then_some(extension)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn validate_parsed_resource_bytes(
    wire: &str,
    parsed: &ParsedWirePath<'_>,
    bytes: &[u8],
) -> Result<(), String> {
    validate_named_resource_bytes(
        wire,
        parsed.category,
        parsed.file_name(),
        parsed.canonical_digest,
        bytes,
    )
}

fn validate_managed_source_bytes(
    category: ResourceCategory,
    source: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("managed sync resource has no portable file name: {source:?}"))?;
    let canonical_digest = canonical_digest_from_name(name, name)?;
    validate_named_resource_bytes(name, category, name, canonical_digest, bytes)
}

fn validate_named_resource_bytes(
    display_path: &str,
    category: ResourceCategory,
    file_name: &str,
    canonical_digest: Option<&str>,
    bytes: &[u8],
) -> Result<(), String> {
    let raw_digest = sha256_hex(bytes);
    if let Some(expected) = canonical_digest {
        if raw_digest.eq_ignore_ascii_case(expected) {
            return Ok(());
        }
        return Err(format!(
            "content digest mismatch for sync resource {display_path:?}"
        ));
    }

    let legacy_stem = file_name.split('.').next().unwrap_or_default();
    if !is_sha256_hex(legacy_stem) {
        return Ok(());
    }
    let valid = match category {
        ResourceCategory::File => raw_digest.eq_ignore_ascii_case(legacy_stem),
        ResourceCategory::Image => {
            raw_digest.eq_ignore_ascii_case(legacy_stem)
                || crate::content::hash::compute_media_hash("image", bytes)
                    .eq_ignore_ascii_case(legacy_stem)
        }
        ResourceCategory::Preview | ResourceCategory::Icon => true,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "legacy content digest mismatch for sync resource {display_path:?}"
        ))
    }
}

fn secure_target_path(
    parsed: &ParsedWirePath<'_>,
    paths: &StoragePaths,
) -> Result<PathBuf, String> {
    let (_, canonical_base) = canonical_resource_base(parsed.category, paths, true)?;

    let mut parent = canonical_base;
    for segment in &parsed.segments[..parsed.segments.len() - 1] {
        parent.push(segment);
        match fs::symlink_metadata(&parent) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "sync resource path crosses a symlink: {}",
                    parent.display()
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "sync resource parent is not a directory: {}",
                    parent.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {
                fs::create_dir(&parent).map_err(|error| {
                    format!(
                        "failed to create sync resource directory {}: {error}",
                        parent.display()
                    )
                })?;
            }
            Err(error) => {
                return Err(format!(
                    "failed to inspect sync resource directory {}: {error}",
                    parent.display()
                ));
            }
        }
    }

    let target = parent.join(parsed.file_name());
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "sync resource target is a symlink: {}",
            target.display()
        )),
        Ok(metadata) if !metadata.is_file() => Err(format!(
            "sync resource target is not a file: {}",
            target.display()
        )),
        Ok(_) => Ok(target),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(target),
        Err(error) => Err(format!(
            "failed to inspect sync resource target {}: {error}",
            target.display()
        )),
    }
}

fn canonical_resource_base(
    category: ResourceCategory,
    paths: &StoragePaths,
    create_icon: bool,
) -> Result<(PathBuf, PathBuf), String> {
    let base = category.base(paths);
    match fs::symlink_metadata(&base) {
        Ok(metadata)
            if matches!(category, ResourceCategory::Preview | ResourceCategory::Icon)
                && metadata.file_type().is_symlink() =>
        {
            return Err(format!(
                "sync resource base must not be a symlink: {}",
                base.display()
            ));
        }
        Ok(_) => {}
        Err(error)
            if error.kind() == ErrorKind::NotFound
                && category == ResourceCategory::Icon
                && create_icon =>
        {
            fs::create_dir(&base).map_err(|error| {
                format!(
                    "failed to create sync icon directory {}: {error}",
                    base.display()
                )
            })?;
        }
        Err(error) => {
            return Err(format!(
                "failed to inspect sync resource directory {}: {error}",
                base.display()
            ));
        }
    }

    let canonical_base = fs::canonicalize(&base).map_err(|error| {
        format!(
            "failed to resolve sync resource directory {}: {error}",
            base.display()
        )
    })?;
    if !canonical_base.is_dir() {
        return Err(format!(
            "sync resource base is not a directory: {}",
            canonical_base.display()
        ));
    }
    Ok((base, canonical_base))
}

fn existing_safe_target_path(
    parsed: &ParsedWirePath<'_>,
    paths: &StoragePaths,
) -> Result<PathBuf, String> {
    let (base, canonical_base) = canonical_resource_base(parsed.category, paths, false)?;
    let mut current = canonical_base;
    for (index, segment) in parsed.segments.iter().enumerate() {
        current.push(segment);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            format!(
                "sync resource target is unavailable {}: {error}",
                current.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "sync resource path crosses a symlink: {}",
                current.display()
            ));
        }
        let is_last = index + 1 == parsed.segments.len();
        if (is_last && !metadata.is_file()) || (!is_last && !metadata.is_dir()) {
            return Err(format!(
                "sync resource path has an unexpected file type: {}",
                current.display()
            ));
        }
    }
    Ok(base.join(parsed.relative_path()))
}

fn read_existing_resource(
    target: &Path,
    wire: &str,
    parsed: &ParsedWirePath<'_>,
) -> Result<Option<()>, String> {
    match fs::read(target) {
        Ok(bytes) => {
            validate_parsed_resource_bytes(wire, parsed, &bytes).map_err(|error| {
                format!(
                    "invalid local sync resource cache {}: {error}",
                    target.display()
                )
            })?;
            Ok(Some(()))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "failed to read local sync resource cache {}: {error}",
            target.display()
        )),
    }
}

fn wire_to_abs(
    wire: &str,
    paths: &StoragePaths,
    allowed: &[ResourceCategory],
) -> Result<String, String> {
    let parsed = parse_wire_path(wire)?;
    if !allowed.contains(&parsed.category) {
        return Err(format!(
            "unexpected sync resource category in path {wire:?}"
        ));
    }
    Ok(existing_safe_target_path(&parsed, paths)?
        .to_string_lossy()
        .to_string())
}

fn wire_to_icon(wire: &str, paths: &StoragePaths) -> Result<String, String> {
    let parsed = parse_wire_path(wire)?;
    if parsed.category != ResourceCategory::Icon {
        return Err(format!("unexpected icon sync path {wire:?}"));
    }
    existing_safe_target_path(&parsed, paths)?;
    Ok(parsed.file_name().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoragePaths;

    fn temporary_storage(label: &str) -> StoragePaths {
        let project = std::env::temp_dir()
            .join("clipboard-sync-resources")
            .join(label)
            .join("project");
        let _ = fs::remove_dir_all(project.parent().unwrap());
        StoragePaths::initialize(project).unwrap()
    }

    fn canonical_wire(category: &str, bytes: &[u8], extension: &str) -> String {
        format!(
            "{category}/sha256-{}{}",
            sha256_hex(bytes),
            if extension.is_empty() {
                String::new()
            } else {
                format!(".{extension}")
            }
        )
    }

    fn target_for_wire(paths: &StoragePaths, wire: &str) -> PathBuf {
        let parsed = parse_wire_path(wire).unwrap();
        parsed.category.base(paths).join(parsed.relative_path())
    }

    fn sample_image_entry() -> SyncChangeLogEntry {
        SyncChangeLogEntry {
            sequence: 1,
            item_id: "img_1".to_string(),
            operation: "insert".to_string(),
            kind: "image".to_string(),
            title: "shot".to_string(),
            content_hash: "h1".to_string(),
            resource_path: None,
            preview_path: None,
            icon_path: None,
            text_content: None,
            html_content: None,
            rtf_content: None,
            metadata_json: None,
            is_favorite: false,
            source_app: Some("Browser".to_string()),
            size_bytes: 0,
            last_used_at_ms: None,
            created_at_ms: 0,
            modified_at_ms: 0,
            device_id: "dev".to_string(),
        }
    }

    #[test]
    fn image_resources_round_trip() {
        let paths = temporary_storage("image");
        let img = paths.images.join("abc.png");
        std::fs::write(&img, b"image-bytes").unwrap();

        let mut entry = sample_image_entry();
        entry.resource_path = Some(img.to_string_lossy().to_string());
        entry.preview_path = Some(img.to_string_lossy().to_string());

        let (entries, resources) = collect_entry_resources(vec![entry], &paths);
        let wire = canonical_wire("image", b"image-bytes", "png");
        assert_eq!(resources.len(), 1, "preview equals resource: deduped");
        assert_eq!(resources[0].rel_path, wire);
        assert_eq!(resources[0].bytes, Some(b"image-bytes".to_vec()));
        assert_eq!(entries[0].resource_path.as_deref(), Some(wire.as_str()));
        assert_eq!(entries[0].preview_path.as_deref(), Some(wire.as_str()));

        materialize_resources(&resources, &paths, None).unwrap();
        let mut downloaded = entries;
        rewrite_to_local(&mut downloaded, &paths).unwrap();
        assert_eq!(
            downloaded[0].resource_path.as_deref(),
            Some(target_for_wire(&paths, &wire).to_string_lossy().as_ref())
        );
    }

    #[test]
    fn file_multi_resources_rewrite() {
        let paths = temporary_storage("file");
        let a = paths.files.join("a.pdf");
        let b = paths.files.join("b.pdf");
        std::fs::write(&a, b"a-bytes").unwrap();
        std::fs::write(&b, b"b-bytes").unwrap();

        let mut entry = sample_image_entry();
        entry.kind = "file".to_string();
        entry.text_content = Some(
            serde_json::to_string(&[
                a.to_string_lossy().to_string(),
                b.to_string_lossy().to_string(),
            ])
            .unwrap(),
        );

        let (entries, resources) = collect_entry_resources(vec![entry], &paths);
        assert_eq!(resources.len(), 2);
        let wire_a = canonical_wire("file", b"a-bytes", "pdf");
        let wire_b = canonical_wire("file", b"b-bytes", "pdf");
        assert_eq!(resources[0].rel_path, wire_a);
        assert_eq!(resources[1].rel_path, wire_b);

        materialize_resources(&resources, &paths, None).unwrap();
        let mut downloaded = entries;
        rewrite_to_local(&mut downloaded, &paths).unwrap();
        let restored: Vec<String> =
            serde_json::from_str(downloaded[0].text_content.as_deref().unwrap()).unwrap();
        assert_eq!(
            restored[0],
            target_for_wire(&paths, &wire_a).to_string_lossy()
        );
        assert_eq!(
            restored[1],
            target_for_wire(&paths, &wire_b).to_string_lossy()
        );
    }

    #[test]
    fn icon_resources_round_trip() {
        let paths = temporary_storage("icon");
        let icons = paths.storage.join("icons");
        std::fs::create_dir_all(&icons).unwrap();
        std::fs::write(icons.join("browser.png"), b"icon-bytes").unwrap();

        let mut entry = sample_image_entry();
        entry.icon_path = Some("browser.png".to_string());

        let (entries, resources) = collect_entry_resources(vec![entry], &paths);
        let wire = canonical_wire("icon", b"icon-bytes", "png");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].rel_path, wire);
        assert_eq!(entries[0].icon_path.as_deref(), Some(wire.as_str()));

        materialize_resources(&resources, &paths, None).unwrap();
        let mut downloaded = entries;
        rewrite_to_local(&mut downloaded, &paths).unwrap();
        let file_name = parse_wire_path(&wire).unwrap().file_name().to_string();
        assert_eq!(downloaded[0].icon_path.as_deref(), Some(file_name.as_str()));

        assert_eq!(
            std::fs::read(target_for_wire(&paths, &wire)).unwrap(),
            b"icon-bytes"
        );
    }

    #[test]
    fn materialize_writes_into_correct_subdirs() {
        let paths = temporary_storage("materialize");
        let resources = vec![
            OplogResource {
                rel_path: "image/a.png".to_string(),
                bytes: Some(b"i".to_vec()),
            },
            OplogResource {
                rel_path: "preview/a.jpg".to_string(),
                bytes: Some(b"p".to_vec()),
            },
            OplogResource {
                rel_path: "file/a.pdf".to_string(),
                bytes: Some(b"f".to_vec()),
            },
        ];
        materialize_resources(&resources, &paths, None).unwrap();
        assert_eq!(std::fs::read(paths.images.join("a.png")).unwrap(), b"i");
        assert_eq!(std::fs::read(paths.previews.join("a.jpg")).unwrap(), b"p");
        assert_eq!(std::fs::read(paths.files.join("a.pdf")).unwrap(), b"f");
    }

    #[test]
    fn missing_resource_path_is_cleared() {
        let paths = temporary_storage("missing");
        let mut entry = sample_image_entry();
        entry.resource_path = Some(paths.images.join("gone.png").to_string_lossy().to_string());

        let (entries, resources) = collect_entry_resources(vec![entry], &paths);
        assert!(resources.is_empty());
        assert!(entries[0].resource_path.is_none());
    }

    #[test]
    fn pool_reference_is_fetched_through_callback() {
        let paths = temporary_storage("pool-fetch");
        let resources = vec![OplogResource {
            rel_path: "image/from-pool.png".to_string(),
            bytes: None,
        }];

        materialize_resources(
            &resources,
            &paths,
            Some(&|rel: &str| {
                assert_eq!(rel, "image/from-pool.png");
                Ok(b"pool-bytes".to_vec())
            }),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(paths.images.join("from-pool.png")).unwrap(),
            b"pool-bytes"
        );
    }

    #[test]
    fn pool_reference_without_callback_is_rejected() {
        let paths = temporary_storage("pool-skip");
        let resources = vec![OplogResource {
            rel_path: "image/from-pool.png".to_string(),
            bytes: None,
        }];

        assert!(materialize_resources(&resources, &paths, None).is_err());
        assert!(!paths.images.join("from-pool.png").exists());
    }

    #[test]
    fn pool_reference_existing_file_is_not_refetched() {
        let paths = temporary_storage("pool-existing");
        std::fs::write(paths.images.join("already.png"), b"local").unwrap();
        let resources = vec![OplogResource {
            rel_path: "image/already.png".to_string(),
            bytes: None,
        }];

        materialize_resources(
            &resources,
            &paths,
            Some(&|_| panic!("existing file must not be re-downloaded")),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(paths.images.join("already.png")).unwrap(),
            b"local"
        );
    }

    #[test]
    fn materialize_rejects_traversal_and_unknown_paths() {
        let paths = temporary_storage("traversal");
        let outside = paths.storage.join("escape.bin");
        for wire in [
            "image/../escape.bin",
            "image/..\\..\\escape.bin",
            "image/C:/escape.bin",
            "image/fragment#escape.bin",
            "image/encoded%2fescape.bin",
            "unknown/escape.bin",
            "icon/nested/escape.bin",
        ] {
            let resources = vec![OplogResource {
                rel_path: wire.to_string(),
                bytes: Some(b"escape".to_vec()),
            }];
            assert!(
                materialize_resources(&resources, &paths, None).is_err(),
                "{wire} must be rejected"
            );
        }
        assert!(!outside.exists());
    }

    #[test]
    fn collect_does_not_read_paths_outside_managed_roots() {
        let paths = temporary_storage("source-escape");
        let secret = paths.storage.join("secret.bin");
        fs::write(&secret, b"secret").unwrap();

        let mut entry = sample_image_entry();
        entry.resource_path = Some(
            paths
                .images
                .join("..")
                .join("secret.bin")
                .to_string_lossy()
                .to_string(),
        );
        let (entries, resources) = collect_entry_resources(vec![entry], &paths);

        assert!(resources.is_empty());
        assert!(entries[0].resource_path.is_none());
    }

    #[test]
    fn collect_does_not_upload_corrupted_content_addressed_files() {
        let paths = temporary_storage("source-corrupt");
        let wire = canonical_wire("image", b"expected", "png");
        let target = target_for_wire(&paths, &wire);
        fs::write(&target, b"corrupt").unwrap();

        let mut entry = sample_image_entry();
        entry.resource_path = Some(target.to_string_lossy().to_string());
        let (entries, resources) = collect_entry_resources(vec![entry], &paths);

        assert!(resources.is_empty());
        assert!(entries[0].resource_path.is_none());
    }

    #[test]
    fn collect_rejects_non_bare_icon_paths() {
        let paths = temporary_storage("icon-escape");
        let icons = paths.storage.join("icons");
        fs::create_dir_all(&icons).unwrap();
        fs::write(paths.storage.join("secret.png"), b"secret-icon").unwrap();

        let mut entry = sample_image_entry();
        entry.icon_path = Some("../secret.png".to_string());
        let (entries, resources) = collect_entry_resources(vec![entry], &paths);

        assert!(resources.is_empty());
        assert!(entries[0].icon_path.is_none());
    }

    #[test]
    fn canonical_digest_is_verified_before_inline_write() {
        let paths = temporary_storage("canonical-inline");
        let bytes = b"verified-inline";
        let wire = canonical_wire("image", bytes, "png");
        let resources = vec![OplogResource {
            rel_path: wire.clone(),
            bytes: Some(bytes.to_vec()),
        }];

        materialize_resources(&resources, &paths, None).unwrap();
        assert_eq!(fs::read(target_for_wire(&paths, &wire)).unwrap(), bytes);

        let bad_wire = canonical_wire("image", b"different", "png");
        let bad_target = target_for_wire(&paths, &bad_wire);
        let bad = vec![OplogResource {
            rel_path: bad_wire,
            bytes: Some(bytes.to_vec()),
        }];
        assert!(materialize_resources(&bad, &paths, None).is_err());
        assert!(!bad_target.exists());
    }

    #[test]
    fn pool_digest_mismatch_is_rejected_without_writing() {
        let paths = temporary_storage("canonical-pool-mismatch");
        let wire = canonical_wire("file", b"expected", "bin");
        let target = target_for_wire(&paths, &wire);
        let resources = vec![OplogResource {
            rel_path: wire,
            bytes: None,
        }];

        assert!(
            materialize_resources(&resources, &paths, Some(&|_| Ok(b"tampered".to_vec())),)
                .is_err()
        );
        assert!(!target.exists());
    }

    #[test]
    fn corrupted_canonical_cache_is_refetched_and_repaired() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let paths = temporary_storage("canonical-cache-repair");
        let bytes = b"pool-correct";
        let wire = canonical_wire("image", bytes, "png");
        let target = target_for_wire(&paths, &wire);
        fs::write(&target, b"corrupt").unwrap();
        let fetches = AtomicUsize::new(0);
        let resources = vec![OplogResource {
            rel_path: wire,
            bytes: None,
        }];

        materialize_resources(
            &resources,
            &paths,
            Some(&|_| {
                fetches.fetch_add(1, Ordering::SeqCst);
                Ok(bytes.to_vec())
            }),
        )
        .unwrap();

        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read(target).unwrap(), bytes);
    }

    #[test]
    fn corrupted_canonical_cache_without_fetcher_is_rejected() {
        let paths = temporary_storage("canonical-cache-no-fetch");
        let wire = canonical_wire("image", b"expected", "png");
        let target = target_for_wire(&paths, &wire);
        fs::write(&target, b"corrupt").unwrap();
        let resources = vec![OplogResource {
            rel_path: wire,
            bytes: None,
        }];

        assert!(materialize_resources(&resources, &paths, None).is_err());
        assert_eq!(fs::read(target).unwrap(), b"corrupt");
    }

    #[test]
    fn safe_legacy_hash_paths_remain_compatible() {
        let paths = temporary_storage("legacy-hash");
        let file_bytes = b"legacy-file";
        let file_wire = format!("file/{}.pdf", sha256_hex(file_bytes));
        let image_bytes = b"legacy-image";
        let image_wire = format!(
            "image/{}.png",
            crate::content::hash::compute_media_hash("image", image_bytes)
        );
        let resources = vec![
            OplogResource {
                rel_path: file_wire.clone(),
                bytes: Some(file_bytes.to_vec()),
            },
            OplogResource {
                rel_path: image_wire.clone(),
                bytes: Some(image_bytes.to_vec()),
            },
        ];

        materialize_resources(&resources, &paths, None).unwrap();
        assert_eq!(
            fs::read(target_for_wire(&paths, &file_wire)).unwrap(),
            file_bytes
        );
        assert_eq!(
            fs::read(target_for_wire(&paths, &image_wire)).unwrap(),
            image_bytes
        );
    }

    #[test]
    fn rewrite_rejects_absolute_traversal_and_wrong_category_paths() {
        let paths = temporary_storage("rewrite-invalid");
        for wire in [
            "image/../escape.png".to_string(),
            paths
                .project
                .join("absolute.png")
                .to_string_lossy()
                .to_string(),
            "file/wrong-category.pdf".to_string(),
            "image/missing-resource.png".to_string(),
        ] {
            let mut entry = sample_image_entry();
            entry.resource_path = Some(wire.clone());
            assert!(rewrite_to_local(std::slice::from_mut(&mut entry), &paths).is_err());
            assert_eq!(entry.resource_path.as_deref(), Some(wire.as_str()));
        }
    }

    #[test]
    fn materialize_rejects_nested_symlink_escape_when_supported() {
        let paths = temporary_storage("symlink-escape");
        let outside = paths.project.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let nested = paths.images.join("nested");
        if create_directory_symlink(&outside, &nested).is_err() {
            return;
        }
        let resources = vec![OplogResource {
            rel_path: "image/nested/escape.bin".to_string(),
            bytes: Some(b"escape".to_vec()),
        }];

        assert!(materialize_resources(&resources, &paths, None).is_err());
        assert!(!outside.join("escape.bin").exists());
    }

    #[cfg(unix)]
    fn create_directory_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }
}
