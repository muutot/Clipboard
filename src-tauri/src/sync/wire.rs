//! Bincode v2 wire encoding for sync payloads (`Baseline`, `Oplog` envelopes).
//! Despite its historical name, the on-wire format is bincode, not protobuf.

use crate::domain::ClipboardItem;

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Baseline {
    pub format_version: u32,
    pub created_at_ms: i64,
    pub device_id: String,
    pub app_version: String,
    pub items: Vec<ClipboardItem>,
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

pub fn serialize_baseline(items: &[ClipboardItem], device_id: &str) -> Result<Vec<u8>, String> {
    let baseline = Baseline {
        format_version: 1,
        created_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        device_id: device_id.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        items: items.to_vec(),
    };

    bincode::encode_to_vec(baseline, bincode::config::standard())
        .map_err(|e| format!("failed to serialize baseline: {e}"))
}

pub fn deserialize_baseline(data: &[u8]) -> Result<Vec<ClipboardItem>, String> {
    let (baseline, _): (Baseline, _) =
        bincode::decode_from_slice(data, bincode::config::standard())
            .map_err(|e| format!("failed to deserialize baseline: {e}"))?;
    Ok(baseline.items)
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
}
