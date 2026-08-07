use crate::domain::ClipboardItem;

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Baseline {
    pub format_version: u32,
    pub created_at_ms: i64,
    pub device_id: String,
    pub app_version: String,
    pub items: Vec<ClipboardItem>,
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Oplog {
    pub entries: Vec<crate::storage::SyncChangeLogEntry>,
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
    let oplog = Oplog {
        entries: entries.to_vec(),
    };

    bincode::encode_to_vec(oplog, bincode::config::standard())
        .map_err(|e| format!("failed to serialize oplog: {e}"))
}

pub fn deserialize_oplog(data: &[u8]) -> Result<Vec<crate::storage::SyncChangeLogEntry>, String> {
    let (oplog, _): (Oplog, _) = bincode::decode_from_slice(data, bincode::config::standard())
        .map_err(|e| format!("failed to deserialize oplog: {e}"))?;
    Ok(oplog.entries)
}
