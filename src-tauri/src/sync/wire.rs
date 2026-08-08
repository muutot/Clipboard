//! Bincode v2 wire encoding for sync payloads (`Baseline`, `Oplog` envelopes).
//! Despite its historical name, the on-wire format is bincode, not protobuf.

use crate::domain::ClipboardItem;

/// Legacy baseline envelope written before inline resources shipped.
/// Kept separate from `Baseline` so older bincode files (items only) still
/// decode via the V1 fallback path.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct BaselineV1 {
    pub format_version: u32,
    pub created_at_ms: i64,
    pub device_id: String,
    pub app_version: String,
    pub items: Vec<ClipboardItem>,
}

/// Baseline envelope carrying inline resource bytes alongside items, so a
/// remote baseline is self-sufficient: a new device can fully materialize
/// images/files without depending on oplog files surviving cleanup.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Baseline {
    pub format_version: u32,
    pub created_at_ms: i64,
    pub device_id: String,
    pub app_version: String,
    pub items: Vec<ClipboardItem>,
    pub resources: Vec<OplogResource>,
}

/// A resource file carried inline with an oplog so the receiving device can
/// materialize images/files/previews/icons regardless of its local storage
/// layout. `rel_path` uses the `category/relative` wire form (e.g.
/// `image/abc.png`, `file/report.pdf`, `preview/thumb.webp`, `icon/app.png`).
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct OplogResource {
    pub rel_path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Oplog {
    pub entries: Vec<crate::storage::SyncChangeLogEntry>,
}

/// Versioned oplog envelope carrying inline resource bytes. Kept separate from
/// `Oplog` so older bincode files (entries only) still decode via the V1
/// fallback path.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct OplogV2 {
    pub entries: Vec<crate::storage::SyncChangeLogEntry>,
    pub resources: Vec<OplogResource>,
}

pub fn serialize_baseline_with_resources(
    items: &[ClipboardItem],
    resources: &[OplogResource],
    device_id: &str,
) -> Result<Vec<u8>, String> {
    let baseline = Baseline {
        format_version: 2,
        created_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        device_id: device_id.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        items: items.to_vec(),
        resources: resources.to_vec(),
    };

    bincode::encode_to_vec(baseline, bincode::config::standard())
        .map_err(|e| format!("failed to serialize baseline: {e}"))
}

pub fn serialize_baseline(items: &[ClipboardItem], device_id: &str) -> Result<Vec<u8>, String> {
    serialize_baseline_with_resources(items, &[], device_id)
}

pub fn deserialize_baseline_with_resources(
    data: &[u8],
) -> Result<(Vec<ClipboardItem>, Vec<OplogResource>), String> {
    // Try current (V2, items + resources) first, then fall back to the legacy
    // V1 baseline written before inline resources shipped.
    let v2: Result<(Baseline, usize), _> =
        bincode::decode_from_slice(data, bincode::config::standard());
    if let Ok((baseline, _)) = v2 {
        return Ok((baseline.items, baseline.resources));
    }
    let (legacy, _): (BaselineV1, _) =
        bincode::decode_from_slice(data, bincode::config::standard())
            .map_err(|e| format!("failed to deserialize baseline: {e}"))?;
    Ok((legacy.items, Vec::new()))
}

pub fn deserialize_baseline(data: &[u8]) -> Result<Vec<ClipboardItem>, String> {
    deserialize_baseline_with_resources(data).map(|(items, _)| items)
}

/// Merges multiple baseline payloads (already decrypted) into a single
/// consolidated view: items are unioned by id (keeping the one with the later
/// `created_at_ms` for the same id), resources are unioned by `rel_path`.
///
/// Multiple baselines arise only from concurrent first syncs of separate
/// devices; they are disjoint full snapshots with no common root, so any
/// delete-one-keep-one policy would lose data. Merging produces a superset
/// that covers every source device's items and resources.
pub fn merge_baselines(
    payloads: &[Vec<u8>],
) -> Result<(Vec<ClipboardItem>, Vec<OplogResource>), String> {
    let mut items: std::collections::HashMap<String, ClipboardItem> =
        std::collections::HashMap::new();
    let mut resources: std::collections::HashMap<String, Vec<u8>> =
        std::collections::HashMap::new();

    for payload in payloads {
        let (batch_items, batch_resources) = deserialize_baseline_with_resources(payload)?;
        for item in batch_items {
            match items.get(&item.id) {
                Some(existing) if existing.created_at_ms >= item.created_at_ms => {}
                _ => {
                    items.insert(item.id.clone(), item);
                }
            }
        }
        for resource in batch_resources {
            resources.entry(resource.rel_path).or_insert(resource.bytes);
        }
    }

    let mut merged_items: Vec<ClipboardItem> = items.into_values().collect();
    merged_items.sort_by_key(|i| i.created_at_ms);
    let merged_resources: Vec<OplogResource> = resources
        .into_iter()
        .map(|(rel_path, bytes)| OplogResource { rel_path, bytes })
        .collect();
    Ok((merged_items, merged_resources))
}

pub fn serialize_oplog(entries: &[crate::storage::SyncChangeLogEntry]) -> Result<Vec<u8>, String> {
    serialize_oplog_with_resources(entries, &[])
}

pub fn serialize_oplog_with_resources(
    entries: &[crate::storage::SyncChangeLogEntry],
    resources: &[OplogResource],
) -> Result<Vec<u8>, String> {
    let oplog = OplogV2 {
        entries: entries.to_vec(),
        resources: resources.to_vec(),
    };

    bincode::encode_to_vec(oplog, bincode::config::standard())
        .map_err(|e| format!("failed to serialize oplog: {e}"))
}

pub fn deserialize_oplog(data: &[u8]) -> Result<Vec<crate::storage::SyncChangeLogEntry>, String> {
    deserialize_oplog_with_resources(data).map(|(entries, _)| entries)
}

pub fn deserialize_oplog_with_resources(
    data: &[u8],
) -> Result<(Vec<crate::storage::SyncChangeLogEntry>, Vec<OplogResource>), String> {
    // Try V2 (entries + resources) first, then fall back to V1 (entries only)
    // so files written before inline resources shipped remain readable.
    let v2: Result<(OplogV2, usize), _> =
        bincode::decode_from_slice(data, bincode::config::standard());
    if let Ok((oplog, _)) = v2 {
        return Ok((oplog.entries, oplog.resources));
    }
    let (oplog, _): (Oplog, _) = bincode::decode_from_slice(data, bincode::config::standard())
        .map_err(|e| format!("failed to deserialize oplog: {e}"))?;
    Ok((oplog.entries, Vec::new()))
}

#[cfg(test)]
mod tests {
    use crate::storage::SyncChangeLogEntry;

    use super::*;

    fn sample_entry() -> SyncChangeLogEntry {
        SyncChangeLogEntry {
            sequence: 1,
            item_id: "img_abc".to_string(),
            operation: "insert".to_string(),
            kind: "image".to_string(),
            title: "abc".to_string(),
            content_hash: "hash-abc".to_string(),
            resource_path: Some("image/abc.png".to_string()),
            preview_path: Some("image/abc.png".to_string()),
            icon_path: None,
            text_content: None,
            html_content: None,
            rtf_content: None,
            metadata_json: None,
            is_favorite: false,
            source_app: Some("test".to_string()),
            size_bytes: 0,
            last_used_at_ms: None,
            created_at_ms: 100,
            modified_at_ms: 100,
            device_id: "test-device".to_string(),
        }
    }

    #[test]
    fn oplog_round_trips_resources() {
        let resource = OplogResource {
            rel_path: "image/abc.png".to_string(),
            bytes: vec![0x89, 0x50, 0x4e, 0x47],
        };
        let data = serialize_oplog_with_resources(&[sample_entry()], &[resource.clone()]).unwrap();
        let (entries, resources) = deserialize_oplog_with_resources(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].item_id, "img_abc");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].rel_path, resource.rel_path);
        assert_eq!(resources[0].bytes, resource.bytes);
    }

    #[test]
    fn oplog_without_resources_still_deserializes() {
        let data = serialize_oplog(&[sample_entry()]).unwrap();
        let (entries, resources) = deserialize_oplog_with_resources(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(resources.is_empty());
    }

    #[test]
    fn legacy_v1_oplog_without_resources_decodes() {
        // Simulate a file written before inline resources shipped: a V1 Oplog
        // envelope (entries only) must still decode through the fallback path.
        let legacy = Oplog {
            entries: vec![sample_entry()],
        };
        let data = bincode::encode_to_vec(legacy, bincode::config::standard()).unwrap();
        let (entries, resources) = deserialize_oplog_with_resources(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].item_id, "img_abc");
        assert!(resources.is_empty());
    }

    #[test]
    fn baseline_with_resources_round_trips() {
        let resource = OplogResource {
            rel_path: "image/abc.png".to_string(),
            bytes: vec![0x89, 0x50, 0x4e, 0x47],
        };
        let items = vec![sample_item()];
        let data = serialize_baseline_with_resources(&items, &[resource.clone()], "dev-a").unwrap();
        let (stored_items, stored_resources) = deserialize_baseline_with_resources(&data).unwrap();
        assert_eq!(stored_items.len(), 1);
        assert_eq!(stored_items[0].id, "img_abc");
        assert_eq!(stored_resources.len(), 1);
        assert_eq!(stored_resources[0].rel_path, "image/abc.png");
        assert_eq!(stored_resources[0].bytes, resource.bytes);
    }

    #[test]
    fn legacy_v1_baseline_without_resources_decodes() {
        // A V1 baseline envelope (items only) must still decode through the
        // fallback path and yield no resources.
        let legacy = BaselineV1 {
            format_version: 1,
            created_at_ms: 100,
            device_id: "dev-a".to_string(),
            app_version: "test".to_string(),
            items: vec![sample_item()],
        };
        let data = bincode::encode_to_vec(legacy, bincode::config::standard()).unwrap();
        let (items, resources) = deserialize_baseline_with_resources(&data).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "img_abc");
        assert!(resources.is_empty());
    }

    fn sample_item() -> ClipboardItem {
        ClipboardItem {
            id: "img_abc".to_string(),
            kind: crate::domain::ClipboardKind::Image,
            title: "abc".to_string(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: Some("image/abc.png".to_string()),
            preview_path: Some("image/abc.png".to_string()),
            content_hash: "hash-abc".to_string(),
            source_app: Some("test".to_string()),
            icon_path: None,
            size_bytes: 0,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    fn sample_item_with(id: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_string(),
            created_at_ms,
            ..sample_item()
        }
    }

    #[test]
    fn merge_baselines_unions_disjoint_snapshots() {
        // Two concurrent first syncs produce disjoint full snapshots (like two
        // repositories with unrelated histories). Merging must keep both sets.
        let a = serialize_baseline_with_resources(
            &[
                sample_item_with("img_a1", 100),
                sample_item_with("img_a2", 150),
            ],
            &[OplogResource {
                rel_path: "image/a.png".to_string(),
                bytes: b"a".to_vec(),
            }],
            "dev-a",
        )
        .unwrap();
        let b = serialize_baseline_with_resources(
            &[
                sample_item_with("img_b1", 200),
                sample_item_with("img_b2", 250),
            ],
            &[OplogResource {
                rel_path: "file/b.pdf".to_string(),
                bytes: b"b".to_vec(),
            }],
            "dev-b",
        )
        .unwrap();

        let (items, resources) = merge_baselines(&[a, b]).unwrap();
        let mut ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["img_a1", "img_a2", "img_b1", "img_b2"]);
        assert_eq!(resources.len(), 2);
    }

    #[test]
    fn merge_baselines_keeps_newer_for_same_id() {
        let old =
            serialize_baseline_with_resources(&[sample_item_with("img_x", 100)], &[], "dev-a")
                .unwrap();
        let newer =
            serialize_baseline_with_resources(&[sample_item_with("img_x", 900)], &[], "dev-b")
                .unwrap();
        let (items, _) = merge_baselines(&[old, newer]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].created_at_ms, 900);
    }
}
