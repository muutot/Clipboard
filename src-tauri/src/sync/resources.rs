use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::wire::OplogResource;
use crate::storage::{StoragePaths, SyncChangeLogEntry};

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
                                    bytes,
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
                                    bytes,
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
                                    bytes,
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
                                            bytes,
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
                let wire = format!("icon/{}", icon.replace('\\', "/"));
                let path = paths.storage.join("icons").join(&icon);
                if let Ok(bytes) = std::fs::read(&path) {
                    if seen.insert(wire.clone()) {
                        resources.push(OplogResource {
                            rel_path: wire.clone(),
                            bytes,
                        });
                    }
                }
                item.icon_path = Some(wire);
            }
            item
        })
        .collect();

    (items, resources)
}

/// Converts wire-form paths on `items` (from a downloaded baseline or merged
/// payload) back to local absolute paths so they point at this device's
/// storage. Fields that are not wire form are left untouched.
pub fn rewrite_item_paths_to_local(
    items: &mut [crate::domain::ClipboardItem],
    paths: &StoragePaths,
) {
    use crate::domain::ClipboardKind;

    for item in items {
        match item.kind {
            ClipboardKind::Image => {
                if let Some(wire) = item.resource_path.take() {
                    item.resource_path = wire_to_abs(&wire, paths).or(Some(wire));
                }
                if let Some(wire) = item.preview_path.take() {
                    item.preview_path = wire_to_abs(&wire, paths).or(Some(wire));
                }
            }
            ClipboardKind::File => {
                if let Some(wire) = item.resource_path.take() {
                    item.resource_path = wire_to_abs(&wire, paths).or(Some(wire));
                }
                if let Some(json) = item.text_content.take() {
                    if let Ok(wires) = serde_json::from_str::<Vec<String>>(&json) {
                        let abs = wires
                            .into_iter()
                            .map(|wire| wire_to_abs(&wire, paths).unwrap_or(wire))
                            .collect::<Vec<_>>();
                        item.text_content = serde_json::to_string(&abs).ok();
                    } else {
                        item.text_content = Some(json);
                    }
                }
            }
            _ => {}
        }
        if let Some(wire) = item.icon_path.take() {
            item.icon_path = wire_to_icon(&wire).or(Some(wire));
        }
    }
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
                                    bytes,
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
                                    bytes,
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
                                    bytes,
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
                                            bytes,
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
                let wire = format!("icon/{}", icon.replace('\\', "/"));
                let path = paths.storage.join("icons").join(&icon);
                if let Ok(bytes) = std::fs::read(&path) {
                    if seen.insert(wire.clone()) {
                        resources.push(OplogResource {
                            rel_path: wire.clone(),
                            bytes,
                        });
                    }
                }
                entry.icon_path = Some(wire);
            }
            entry
        })
        .collect();

    (entries, resources)
}

/// Writes inline resource bytes back into the local storage subdirectories
/// according to their wire category.
pub fn materialize_resources(
    resources: &[OplogResource],
    paths: &StoragePaths,
) -> Result<(), String> {
    for resource in resources {
        let Some((category, rel)) = resource.rel_path.split_once('/') else {
            continue;
        };
        let base = match category {
            "image" => paths.images.clone(),
            "preview" => paths.previews.clone(),
            "file" => paths.files.clone(),
            "icon" => paths.storage.join("icons"),
            _ => continue,
        };
        let target = base.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, &resource.bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Converts wire-form paths on `entries` to local absolute paths (or, for
/// icons, back to the bare file name the frontend resolves via `icons`).
/// Fields that are not wire form are left untouched (legacy/absolute paths).
pub fn rewrite_to_local(entries: &mut [SyncChangeLogEntry], paths: &StoragePaths) {
    for entry in entries {
        match entry.kind.as_str() {
            "image" => {
                if let Some(wire) = entry.resource_path.take() {
                    entry.resource_path = wire_to_abs(&wire, paths).or(Some(wire));
                }
                if let Some(wire) = entry.preview_path.take() {
                    entry.preview_path = wire_to_abs(&wire, paths).or(Some(wire));
                }
            }
            "file" => {
                if let Some(wire) = entry.resource_path.take() {
                    entry.resource_path = wire_to_abs(&wire, paths).or(Some(wire));
                }
                if let Some(json) = entry.text_content.take() {
                    if let Ok(wires) = serde_json::from_str::<Vec<String>>(&json) {
                        let abs = wires
                            .into_iter()
                            .map(|wire| wire_to_abs(&wire, paths).unwrap_or(wire))
                            .collect::<Vec<_>>();
                        entry.text_content = serde_json::to_string(&abs).ok();
                    } else {
                        entry.text_content = Some(json);
                    }
                }
            }
            _ => {}
        }
        if let Some(wire) = entry.icon_path.take() {
            entry.icon_path = wire_to_icon(&wire).or(Some(wire));
        }
    }
}

/// Reads a referenced file and produces its wire path + bytes.
/// Returns `None` when the path cannot be mapped to storage or read.
fn resource_bytes(abs_or_rel: &str, paths: &StoragePaths) -> Option<(String, Vec<u8>)> {
    let path = resolve_resource_path(abs_or_rel, paths);
    let bytes = std::fs::read(&path).ok()?;
    let (category, rel) = wire_category_and_rel(&path, paths)?;
    Some((format!("{category}/{rel}"), bytes))
}

fn resolve_resource_path(abs_or_rel: &str, paths: &StoragePaths) -> PathBuf {
    let path = Path::new(abs_or_rel);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.storage.join(path)
    }
}

fn wire_category_and_rel(path: &Path, paths: &StoragePaths) -> Option<(&'static str, String)> {
    let rel = |base: &Path, p: &Path| -> Option<String> {
        p.strip_prefix(base)
            .ok()
            .map(|r| r.to_string_lossy().replace('\\', "/"))
    };
    // previews live under images, so check it first.
    if let Some(rel) = rel(&paths.previews, path) {
        return Some(("preview", rel));
    }
    if let Some(rel) = rel(&paths.images, path) {
        return Some(("image", rel));
    }
    if let Some(rel) = rel(&paths.files, path) {
        return Some(("file", rel));
    }
    None
}

fn wire_to_abs(wire: &str, paths: &StoragePaths) -> Option<String> {
    let (category, rel) = wire.split_once('/')?;
    let base = match category {
        "image" => paths.images.clone(),
        "preview" => paths.previews.clone(),
        "file" => paths.files.clone(),
        _ => return None,
    };
    Some(base.join(rel).to_string_lossy().to_string())
}

fn wire_to_icon(wire: &str) -> Option<String> {
    wire.split_once('/')
        .filter(|(category, _)| *category == "icon")
        .map(|(_, name)| name.to_string())
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
        StoragePaths::initialize(project).unwrap()
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
        assert_eq!(resources.len(), 1, "preview equals resource: deduped");
        assert_eq!(resources[0].rel_path, "image/abc.png");
        assert_eq!(resources[0].bytes, b"image-bytes");
        assert_eq!(entries[0].resource_path.as_deref(), Some("image/abc.png"));
        assert_eq!(entries[0].preview_path.as_deref(), Some("image/abc.png"));

        let mut downloaded = entries;
        rewrite_to_local(&mut downloaded, &paths);
        assert_eq!(
            downloaded[0].resource_path.as_deref(),
            Some(paths.images.join("abc.png").to_string_lossy().as_ref())
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
        assert_eq!(resources[0].rel_path, "file/a.pdf");
        assert_eq!(resources[1].rel_path, "file/b.pdf");

        let mut downloaded = entries;
        rewrite_to_local(&mut downloaded, &paths);
        let restored: Vec<String> =
            serde_json::from_str(downloaded[0].text_content.as_deref().unwrap()).unwrap();
        assert_eq!(restored[0], paths.files.join("a.pdf").to_string_lossy());
        assert_eq!(restored[1], paths.files.join("b.pdf").to_string_lossy());
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
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].rel_path, "icon/browser.png");
        assert_eq!(entries[0].icon_path.as_deref(), Some("icon/browser.png"));

        let mut downloaded = entries;
        rewrite_to_local(&mut downloaded, &paths);
        assert_eq!(downloaded[0].icon_path.as_deref(), Some("browser.png"));

        materialize_resources(&resources, &paths).unwrap();
        assert_eq!(
            std::fs::read(icons.join("browser.png")).unwrap(),
            b"icon-bytes"
        );
    }

    #[test]
    fn materialize_writes_into_correct_subdirs() {
        let paths = temporary_storage("materialize");
        let resources = vec![
            OplogResource {
                rel_path: "image/a.png".to_string(),
                bytes: b"i".to_vec(),
            },
            OplogResource {
                rel_path: "preview/a.jpg".to_string(),
                bytes: b"p".to_vec(),
            },
            OplogResource {
                rel_path: "file/a.pdf".to_string(),
                bytes: b"f".to_vec(),
            },
        ];
        materialize_resources(&resources, &paths).unwrap();
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
}
