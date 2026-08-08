//! Bincode v2 wire encoding for sync payloads (`Baseline`, `Oplog` envelopes).
//! Despite its historical name, the on-wire format is bincode, not protobuf.
//!
//! Field evolution follows the append-only + envelope-versioning rules in
//! `skills/clipboard-dev/references/data-contracts.md`: never change a field's
//! type in place; when the layout changes, add a new envelope and keep the
//! older structs so `deserialize_*` can fall back newest-first.

use crate::domain::ClipboardItem;

// ---------------------------------------------------------------------------
// Legacy envelopes (only used to decode data written by older versions).
// ---------------------------------------------------------------------------

/// Legacy baseline envelope written before inline resources shipped.
/// Kept separate from the resource-carrying envelopes so older bincode files
/// (items only) still decode via the V1 fallback path.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct BaselineV1 {
    pub format_version: u32,
    pub created_at_ms: i64,
    pub device_id: String,
    pub app_version: String,
    pub items: Vec<ClipboardItem>,
}

/// Legacy baseline envelope carrying inline resource bytes alongside items.
/// Superseded by `Baseline` (pool-aware); kept so files written after inline
/// resources shipped (before pooling) still read.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct BaselineV2 {
    pub format_version: u32,
    pub created_at_ms: i64,
    pub device_id: String,
    pub app_version: String,
    pub items: Vec<ClipboardItem>,
    pub resources: Vec<LegacyResource>,
}

/// Legacy oplog envelope (entries only), written before inline resources.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Oplog {
    pub entries: Vec<crate::storage::SyncChangeLogEntry>,
}

/// Legacy oplog envelope carrying inline resource bytes.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct OplogV2 {
    pub entries: Vec<crate::storage::SyncChangeLogEntry>,
    pub resources: Vec<LegacyResource>,
}

/// A resource file carried inline with a payload (`bytes` always present).
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct LegacyResource {
    pub rel_path: String,
    pub bytes: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Current envelopes (resource bytes optional so pool references are possible).
// ---------------------------------------------------------------------------

/// Baseline envelope. Each resource may be embedded inline (`bytes: Some`) so a
/// fresh device can materialize it without touching the remote pool, or
/// reference the pool (`bytes: None`) when the file already exists remotely at
/// `resources/<rel_path>`.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Baseline {
    pub format_version: u32,
    pub created_at_ms: i64,
    pub device_id: String,
    pub app_version: String,
    pub items: Vec<ClipboardItem>,
    pub resources: Vec<OplogResource>,
}

/// A resource referenced by an oplog or baseline. `rel_path` uses the
/// `category/relative` wire form (e.g. `image/abc.png`, `file/report.pdf`,
/// `preview/thumb.webp`, `icon/app.png`). `bytes` is `Some` when the file is
/// embedded inline (first occurrence, keeps the payload self-sufficient) and
/// `None` when the file is already stored in the remote pool at
/// `resources/<rel_path>` and should be fetched from there if missing locally.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct OplogResource {
    pub rel_path: String,
    pub bytes: Option<Vec<u8>>,
}

/// Oplog envelope carrying resource references alongside changelog entries.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct OplogV3 {
    pub entries: Vec<crate::storage::SyncChangeLogEntry>,
    pub resources: Vec<OplogResource>,
}

pub fn serialize_baseline_with_resources(
    items: &[ClipboardItem],
    resources: &[OplogResource],
    device_id: &str,
) -> Result<Vec<u8>, String> {
    let baseline = Baseline {
        format_version: 3,
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
    // Try current (V3, resources may reference the pool) first, then fall back
    // to V2 (inline-only resources) and finally the legacy items-only V1.
    if let Ok((baseline, _)) =
        bincode::decode_from_slice::<Baseline, _>(data, bincode::config::standard())
    {
        return Ok((baseline.items, baseline.resources));
    }
    if let Ok((baseline, _)) =
        bincode::decode_from_slice::<BaselineV2, _>(data, bincode::config::standard())
    {
        let resources = baseline
            .resources
            .into_iter()
            .map(|r| OplogResource {
                rel_path: r.rel_path,
                bytes: Some(r.bytes),
            })
            .collect();
        return Ok((baseline.items, resources));
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
    let mut resources: std::collections::HashMap<String, Option<Vec<u8>>> =
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
            resources
                .entry(resource.rel_path)
                .and_modify(|existing| {
                    // Prefer inline bytes when a payload embeds them.
                    if existing.is_none() {
                        *existing = resource.bytes.clone();
                    }
                })
                .or_insert(resource.bytes);
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
    let oplog = OplogV3 {
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
    // Try current (V3) first, then V2 (inline resources) and V1 (entries only).
    if let Ok((oplog, _)) =
        bincode::decode_from_slice::<OplogV3, _>(data, bincode::config::standard())
    {
        return Ok((oplog.entries, oplog.resources));
    }
    if let Ok((oplog, _)) =
        bincode::decode_from_slice::<OplogV2, _>(data, bincode::config::standard())
    {
        let resources = oplog
            .resources
            .into_iter()
            .map(|r| OplogResource {
                rel_path: r.rel_path,
                bytes: Some(r.bytes),
            })
            .collect();
        return Ok((oplog.entries, resources));
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
            bytes: Some(vec![0x89, 0x50, 0x4e, 0x47]),
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
    fn oplog_resource_can_be_a_pool_reference() {
        // A `None` bytes resource survives a round trip: it references a file
        // already stored in the remote pool instead of carrying it inline.
        let resource = OplogResource {
            rel_path: "image/abc.png".to_string(),
            bytes: None,
        };
        let data = serialize_oplog_with_resources(&[sample_entry()], &[resource.clone()]).unwrap();
        let (entries, resources) = deserialize_oplog_with_resources(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].rel_path, "image/abc.png");
        assert!(resources[0].bytes.is_none());
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
    fn legacy_v2_oplog_with_inline_resources_decodes() {
        // Simulate a file written between inline resources and pooling: its
        // resources carry `Vec<u8>` bytes, which must come back as `Some`.
        let legacy = OplogV2 {
            entries: vec![sample_entry()],
            resources: vec![LegacyResource {
                rel_path: "image/abc.png".to_string(),
                bytes: vec![0x89, 0x50, 0x4e, 0x47],
            }],
        };
        let data = bincode::encode_to_vec(legacy, bincode::config::standard()).unwrap();
        let (entries, resources) = deserialize_oplog_with_resources(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].rel_path, "image/abc.png");
        assert_eq!(resources[0].bytes, Some(vec![0x89, 0x50, 0x4e, 0x47]));
    }

    #[test]
    fn baseline_with_resources_round_trips() {
        let resource = OplogResource {
            rel_path: "image/abc.png".to_string(),
            bytes: Some(vec![0x89, 0x50, 0x4e, 0x47]),
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
    fn baseline_resources_may_reference_the_pool() {
        let resource = OplogResource {
            rel_path: "file/a.pdf".to_string(),
            bytes: None,
        };
        let items = vec![sample_item()];
        let data = serialize_baseline_with_resources(&items, &[resource.clone()], "dev-a").unwrap();
        let (stored_items, stored_resources) = deserialize_baseline_with_resources(&data).unwrap();
        assert_eq!(stored_items.len(), 1);
        assert_eq!(stored_resources.len(), 1);
        assert_eq!(stored_resources[0].rel_path, "file/a.pdf");
        assert!(stored_resources[0].bytes.is_none());
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

    #[test]
    fn legacy_v2_baseline_with_inline_resources_decodes() {
        let legacy = BaselineV2 {
            format_version: 2,
            created_at_ms: 100,
            device_id: "dev-a".to_string(),
            app_version: "test".to_string(),
            items: vec![sample_item()],
            resources: vec![LegacyResource {
                rel_path: "image/abc.png".to_string(),
                bytes: vec![0x89, 0x50, 0x4e, 0x47],
            }],
        };
        let data = bincode::encode_to_vec(legacy, bincode::config::standard()).unwrap();
        let (items, resources) = deserialize_baseline_with_resources(&data).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].rel_path, "image/abc.png");
        assert_eq!(resources[0].bytes, Some(vec![0x89, 0x50, 0x4e, 0x47]));
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
                bytes: Some(b"a".to_vec()),
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
                bytes: Some(b"b".to_vec()),
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
    fn merge_baselines_prefers_inline_over_pool_reference() {
        // One baseline references a resource from the pool (`bytes: None`);
        // another embeds the same resource inline. The merge must keep the
        // inline bytes so consolidated payloads stay self-sufficient.
        let referenced = serialize_baseline_with_resources(
            &[sample_item_with("img_x", 100)],
            &[OplogResource {
                rel_path: "image/a.png".to_string(),
                bytes: None,
            }],
            "dev-a",
        )
        .unwrap();
        let inline = serialize_baseline_with_resources(
            &[sample_item_with("img_x", 100)],
            &[OplogResource {
                rel_path: "image/a.png".to_string(),
                bytes: Some(b"a".to_vec()),
            }],
            "dev-b",
        )
        .unwrap();
        let (_, resources) = merge_baselines(&[referenced, inline]).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].bytes, Some(b"a".to_vec()));
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
