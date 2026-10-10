//! Clipboard repository tests, grouped by the behavior they exercise.
//!
//! The shared row/timestamp fixtures (`text_item`, `cursor_of`, the sync
//! versioning helpers) live here so every group module can reach them with
//! `use super::*`.

use crate::domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus};

use crate::storage::{Database, OcrRepository, SearchOperation, SearchRepository, StorageError};

use super::{
    current_time_ms, ClipboardRepository, HistoryCursor, HistoryFilter, KindDeleteResult,
    KindDeleteScope, KindStorageStats, TextItemUpdate,
};

mod content;
mod items;
mod lifecycle;
mod limits;
mod maintenance;
mod pagination;
mod tags;

fn text_item(id: &str, content_hash: &str, created_at_ms: i64) -> ClipboardItem {
    ClipboardItem {
        id: id.to_owned(),
        kind: ClipboardKind::Text,
        title: format!("record-{id}"),
        text_content: Some(format!("content-{id}")),
        html_content: None,
        rtf_content: None,
        resource_path: None,
        preview_path: None,
        content_hash: content_hash.to_owned(),
        source_app: Some("test-suite".to_owned()),
        size_bytes: 12,
        created_at_ms,
        last_used_at_ms: None,
        is_favorite: false,
        icon_path: None,
        metadata_json: None,
    }
}

/// Derives the keyset cursor from a loaded row exactly like the frontend:
/// Both keys are read directly from the persisted row.
fn cursor_of(item: &ClipboardItem) -> HistoryCursor {
    HistoryCursor {
        last_used_at_ms: item.last_used_at_ms.unwrap(),
        id: item.id.clone(),
    }
}

/// Turns the sync outbox triggers on for one test database, which is what makes
/// `modified_at_ms` a replication version at all.
fn enable_sync_versioning(database: &Database) {
    database
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES ('sync_enabled', '1')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

fn column_type(database: &Database, id: &str, column: &str) -> String {
    database
        .with_connection(|connection| {
            let sql = format!("SELECT typeof({column}) FROM clipboard_items WHERE id = ?1");
            Ok(connection.query_row(&sql, [id], |row| row.get::<_, String>(0))?)
        })
        .unwrap()
}

fn read_version(database: &Database, id: &str) -> Result<i64, String> {
    database
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT modified_at_ms FROM clipboard_items WHERE id = ?1",
                [id],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .map_err(|error| format!("{error:?}"))
}
