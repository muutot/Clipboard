//! Clipboard command surface, split by concern.
//!
//! The `#[tauri::command]` entry points live in focused submodules; the shared
//! announcement helpers and usage recording stay here so every submodule reaches
//! them through `use super::*`. The split is a pure move: signatures, Tauri
//! command names and behavior are unchanged.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Emitter, Runtime};

use crate::commands::lock::lock_state;
use crate::config::ConfigStore;
use crate::content;
use crate::domain::{ClipboardItem, ClipboardKind, OcrResult};
use crate::performance::PerformanceTracker;
use crate::search::{SearchIndex, SearchSyncSummary, SearchSyncWorker, SearchSynchronizer};
use crate::storage::{
    ClipboardRepository, Database, HistoryCursor, HistoryFilter, KindStorageStats, OcrRepository,
    SearchRepository, StoragePaths, TagInfo, TextItemUpdate,
};
use crate::CaptureState;

use super::types::{
    permanently_delete_storage_kind_for, ClipboardHistoryInvalidated, ClipboardItemsChanged,
    ClipboardListItem, ClipboardUsageUpdate, HistoryFilterArgs, SearchPage, SearchResultCache,
    SearchSortDirection, SearchSortField, SearchSortRule, StorageKindDeleteExpectation,
    StorageKindDeleteResult,
};

mod delete;
mod duplicate;
mod editor;
mod items;
mod listing;
mod ocr;
mod rename;
mod tags;

#[cfg(test)]
mod tests;

pub use delete::*;
pub use duplicate::*;
pub use editor::*;
pub use items::*;
pub use listing::*;
pub use ocr::*;
pub use rename::*;
pub use tags::*;

// Private helpers the `tests` submodule reaches as `super::<name>`.
#[cfg(test)]
use delete::{clear_non_favorite_records, delete_clipboard_item_record};
#[cfg(test)]
use rename::{
    is_owned_rename_source, rename_item_record, rewrite_stored_resource_paths, sanitize_file_stem,
};

/// Announces a content change: the mutated rows are read back and attached, so
/// a receiver replaces what it displays instead of guessing a patch.
fn broadcast_content_changed(app: &AppHandle, database: &Database, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let items = match database.get_items_by_ids(ids) {
        Ok(rows) => rows,
        Err(error) => {
            crate::log_warn!("[clipboard] unable to read back mutated items: {error}");
            return;
        }
    };
    emit_items_changed(
        app,
        ClipboardItemsChanged {
            items,
            ..ClipboardItemsChanged::default()
        },
    );
}

/// Announces a membership transition. No read-back: the receiver flips its own
/// deleted flag or drops the id, and the row itself is unchanged.
fn broadcast_membership_changed(
    app: &AppHandle,
    deleted_ids: &[String],
    restored_ids: &[String],
    removed_ids: &[String],
) {
    if deleted_ids.is_empty() && restored_ids.is_empty() && removed_ids.is_empty() {
        return;
    }
    emit_items_changed(
        app,
        ClipboardItemsChanged {
            deleted_ids: deleted_ids.to_vec(),
            restored_ids: restored_ids.to_vec(),
            removed_ids: removed_ids.to_vec(),
            ..ClipboardItemsChanged::default()
        },
    );
}

/// Broadcast failures are logged and swallowed: the mutation itself already
/// succeeded, and the next event or a reload repairs a missed announcement.
fn emit_items_changed(app: &AppHandle, payload: ClipboardItemsChanged) {
    if let Err(error) = app.emit("clipboard-items-changed", &payload) {
        crate::log_warn!("[clipboard] unable to broadcast clipboard-items-changed: {error}");
    }
}

/// Announces rows whose `last_used_at_ms` was stamped.
///
/// Usage is not a content change, so it travels on its own field: the receiving
/// window only has to move the row up. Without this the copy looked inert in the
/// window that did not initiate it — the database order was already correct, so
/// the row jumped to the top on the next reload, which is exactly the confusing
/// "it only sorts after a refresh" behaviour this replaces.
///
/// Generic over the runtime because the tray, which has no view of its own to
/// reorder, announces its copies through here too.
pub(crate) fn broadcast_item_usage<R: Runtime>(
    app: &AppHandle<R>,
    database: &Database,
    ids: impl IntoIterator<Item = impl AsRef<str>>,
) {
    let ids: Vec<String> = ids.into_iter().map(|id| id.as_ref().to_owned()).collect();
    if ids.is_empty() {
        return;
    }
    let payload = ClipboardItemsChanged {
        usage_updates: match database.get_items_by_ids(&ids) {
            Ok(items) => items
                .into_iter()
                .filter_map(|item| {
                    item.last_used_at_ms
                        .map(|last_used_at_ms| ClipboardUsageUpdate {
                            id: item.id,
                            last_used_at_ms,
                        })
                })
                .collect(),
            Err(error) => {
                crate::log_warn!("[clipboard] unable to read back usage timestamps: {error}");
                Vec::new()
            }
        },
        used_ids: ids,
        ..ClipboardItemsChanged::default()
    };
    if let Err(error) = app.emit("clipboard-items-changed", &payload) {
        crate::log_warn!("[clipboard] unable to broadcast clipboard-items-changed: {error}");
    }
}

/// Stamps usage and invalidates the search result cache in one step.
/// `set_last_used` writes no `search_outbox` event (usage is device-local and
/// unwatched by the sync triggers), so neither the lazy drain nor the
/// background worker ever clears `SearchResultCache` for it; a cached
/// `lastUsedAt` page would keep serving the pre-usage order.
pub fn record_item_usage(
    database: &Database,
    search_cache: &SearchResultCache,
    id: &str,
) -> Result<bool, String> {
    let updated = crate::item_operations::record_usage(database, id)?;
    if updated {
        search_cache.clear();
    }
    Ok(updated)
}
