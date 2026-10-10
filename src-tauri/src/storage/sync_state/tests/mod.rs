//! Sync-state persistence tests, grouped by the behavior they exercise.
//!
//! The shared row/version fixtures (`item`, `replicated`, `cursor` and the
//! remote-scope constants) live here so every group module can reach them with
//! `use super::*`.

//! Regression tests for `sync_state` (moved verbatim from `sync_state.rs`).

use super::*;
use crate::{
    domain::{ClipboardItem, ClipboardKind},
    storage::{ClipboardRepository, TextItemUpdate},
};

fn item(id: &str, hash: &str, text: &str) -> ClipboardItem {
    ClipboardItem {
        id: id.to_string(),
        kind: ClipboardKind::Text,
        title: text.to_string(),
        text_content: Some(text.to_string()),
        html_content: None,
        rtf_content: None,
        resource_path: None,
        preview_path: None,
        content_hash: hash.to_string(),
        source_app: Some("test".to_string()),
        icon_path: None,
        size_bytes: text.len() as u64,
        created_at_ms: 100,
        last_used_at_ms: None,
        is_favorite: false,
        metadata_json: Some("{}".to_string()),
    }
}

const REMOTE_SCOPE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REMOTE_DEVICE: &str = "11111111-1111-4111-8111-111111111111";
const REMOTE_EPOCH: &str = "22222222-2222-4222-8222-222222222222";

fn replicated(id: &str, hash: &str, text: &str, version: RecordVersion) -> ReplicatedItem {
    ReplicatedItem {
        item: item(id, hash, text).into(),
        version,
    }
}

fn cursor(sequence: u64, key: Option<&str>) -> DeviceCursor {
    DeviceCursor {
        device_id: REMOTE_DEVICE.to_string(),
        epoch: REMOTE_EPOCH.to_string(),
        sequence,
        last_segment_key: key.map(str::to_string),
    }
}

mod apply;
mod local_state;
mod outbox;
mod resource_refs;
mod state;
mod tombstones;
