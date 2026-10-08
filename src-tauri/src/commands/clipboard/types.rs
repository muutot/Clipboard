use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::super::cleanup::cleanup_orphan_storage_files;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::search::{SearchIndex, SearchSyncSummary, SearchSynchronizer};
use crate::storage::{
    ClipboardRepository, Database, HistoryCursor, KindDeleteResult, KindStorageStats, StoragePaths,
};
use crate::STORAGE_KIND_DELETE_SCOPE;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSortRule {
    pub(crate) field: SearchSortField,
    pub(crate) direction: SearchSortDirection,
}

/// Optional filters for paginated active-history listing, matching
/// `HistoryFilter` in the storage layer. All fields are optional so a partial
/// payload filters only on the supplied axes.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFilterArgs {
    pub(crate) kind: Option<ClipboardKind>,
    pub(crate) favorite: Option<bool>,
    pub(crate) tag: Option<String>,
    pub(crate) source_app: Option<String>,
    pub(crate) date_from_ms: Option<i64>,
    pub(crate) date_to_ms: Option<i64>,
    /// Keyset pagination anchor: resume strictly after this row. When present
    /// the command ignores `offset`. See `HistoryCursor` for the semantics.
    pub(crate) cursor: Option<HistoryCursorArgs>,
}

/// Frontend payload for `HistoryCursor`. The frontend derives both keys
/// from the last `ClipboardItem` of the previous page, so the shapes must
/// stay in sync with `toClipboardItem` in `clipboard.ts`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCursorArgs {
    pub(crate) last_used_at_ms: i64,
    pub(crate) id: String,
}

impl From<HistoryCursorArgs> for HistoryCursor {
    fn from(args: HistoryCursorArgs) -> Self {
        Self {
            last_used_at_ms: args.last_used_at_ms,
            id: args.id,
        }
    }
}

pub struct SearchResultCache {
    inner: Mutex<SearchCacheState>,
}

#[derive(Default)]
struct SearchCacheState {
    generation: u64,
    result: Option<CachedSearchResult>,
}

#[derive(Clone, Copy)]
pub struct SearchCacheToken {
    generation: u64,
    date_bucket: i64,
}

pub(crate) type CachedSearchResult = (
    String,
    Vec<SearchSortRule>,
    usize,
    i64,
    Vec<ClipboardItem>,
    usize,
    bool,
);

/// One page of search results with the true match total. `total_count` counts
/// every index match (not just the capped candidate set) and `truncated` is
/// set when matches beyond `max_results` were dropped, so the UI can say so
/// instead of ending pagination silently.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub(crate) items: Vec<ClipboardListItem>,
    pub(crate) total_count: usize,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardListItem {
    #[serde(flatten)]
    pub(crate) item: ClipboardItem,
    pub(crate) content_loaded: bool,
}

impl From<ClipboardItem> for ClipboardListItem {
    fn from(item: ClipboardItem) -> Self {
        Self {
            item,
            content_loaded: false,
        }
    }
}

/// Local calendar-day bucket. Relative date phrases ("今天", "本周", "本月")
/// resolve against the local day, so a cached page must not survive a local
/// midnight boundary.
fn current_date_bucket() -> i64 {
    use chrono::Datelike;
    chrono::Local::now().date_naive().num_days_from_ce() as i64
}

impl Default for SearchResultCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchResultCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(SearchCacheState::default()),
        }
    }

    /// Capture before reading candidates; invalidation and publication use
    /// the same lock so an old request cannot undo a cache clear.
    pub fn write_token(&self) -> Option<SearchCacheToken> {
        let cache = self.inner.lock().ok()?;
        Some(SearchCacheToken {
            generation: cache.generation,
            date_bucket: current_date_bucket(),
        })
    }

    pub fn get(
        &self,
        query: &str,
        rules: &[SearchSortRule],
        max_results: usize,
        offset: usize,
        limit: usize,
    ) -> Option<(Vec<ClipboardItem>, usize, bool)> {
        let cache = self.inner.lock().ok()?;
        let (cached_query, cached_rules, cached_max, cached_bucket, cached_items, total, truncated) =
            cache.result.as_ref()?;
        if cached_query != query
            || cached_rules != rules
            || *cached_max != max_results
            || *cached_bucket != current_date_bucket()
        {
            return None;
        }
        let reachable = cached_items.len();
        if offset >= reachable {
            return Some((Vec::new(), *total, *truncated));
        }
        let end = (offset + limit).min(reachable);
        Some((cached_items[offset..end].to_vec(), *total, *truncated))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &self,
        token: Option<SearchCacheToken>,
        query: String,
        rules: Vec<SearchSortRule>,
        max_results: usize,
        items: Vec<ClipboardItem>,
        total: usize,
        truncated: bool,
    ) {
        let Some(token) = token else { return };
        if let Ok(mut cache) = self.inner.lock() {
            if cache.generation != token.generation || current_date_bucket() != token.date_bucket {
                return;
            }
            // Metadata/file lists can still be large. Do not retain a result above 16 MiB.
            let bytes = items.iter().fold(0usize, |bytes, item| {
                [
                    Some(item.id.as_str()),
                    Some(item.title.as_str()),
                    Some(item.content_hash.as_str()),
                    item.text_content.as_deref(),
                    item.html_content.as_deref(),
                    item.rtf_content.as_deref(),
                    item.metadata_json.as_deref(),
                    item.resource_path.as_deref(),
                    item.preview_path.as_deref(),
                    item.icon_path.as_deref(),
                    item.source_app.as_deref(),
                ]
                .into_iter()
                .flatten()
                .fold(bytes, |sum, field| sum.saturating_add(field.len()))
            });
            if bytes > 16 * 1024 * 1024 {
                cache.result = None;
                return;
            }
            cache.result = Some((
                query,
                rules,
                max_results,
                token.date_bucket,
                items,
                total,
                truncated,
            ));
        }
    }

    pub fn clear(&self) {
        if let Ok(mut cache) = self.inner.lock() {
            cache.generation = cache.generation.wrapping_add(1);
            cache.result = None;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchSortField {
    #[serde(rename = "createdAt")]
    CreatedAt,
    #[serde(rename = "lastUsedAt")]
    LastUsedAt,
    #[serde(rename = "title")]
    Title,
    #[serde(rename = "size")]
    Size,
    #[serde(rename = "kind")]
    Kind,
    #[serde(rename = "favorite")]
    Favorite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SearchSortDirection {
    #[serde(rename = "asc")]
    Asc,
    #[serde(rename = "desc")]
    Desc,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageKindDeleteExpectation {
    pub(crate) item_count: u64,
    pub(crate) size_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageKindDeleteResult {
    pub(crate) deleted_count: u64,
    pub(crate) deleted_size_bytes: u64,
    pub(crate) removed_files: u64,
    pub(crate) search_sync: Option<SearchSyncSummary>,
    pub(crate) warnings: Vec<String>,
    #[serde(skip_serializing)]
    pub(crate) deleted_ids: Vec<String>,
}

pub use crate::item_operations::CopyError as ClipboardFilesCopyError;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipboardHistoryInvalidated {
    pub(crate) deleted_ids: Vec<String>,
}

/// Broadcast for item-level mutations (favorite, tags, edit, soft delete,
/// restore, permanent delete, and their batch forms).
///
/// Every window keeps its own in-memory copy of the loaded rows, and a Tauri
/// command can only be invoked from the window that asked for it — so without
/// this event a favorite toggled in the float panel stays stale in the main
/// window until something unrelated forces a full reload.
///
/// The four lists exist because a soft delete and a restore cannot be expressed
/// as "an updated record": `deleted` is not a column the record carries, it is
/// which query produced the row (`list_recent` vs `list_deleted`), and the flag
/// lives in the receiver's own display state. So content changes travel as
/// records and the three membership transitions travel as ids.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipboardItemsChanged {
    /// Rows whose content changed and that still exist.
    pub(crate) items: Vec<ClipboardItem>,
    /// Ids that moved into the recycle bin: the receiver marks them deleted.
    pub(crate) deleted_ids: Vec<String>,
    /// Ids that came back out of the recycle bin: the receiver clears the flag.
    pub(crate) restored_ids: Vec<String>,
    /// Ids that are gone for good: the receiver drops them everywhere.
    pub(crate) removed_ids: Vec<String>,
    /// Ids whose usage changed; timestamps travel in usage_updates.
    pub(crate) used_ids: Vec<String>,
    /// Persisted device-local timestamps, without copying record bodies.
    pub(crate) usage_updates: Vec<ClipboardUsageUpdate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipboardUsageUpdate {
    pub(crate) id: String,
    pub(crate) last_used_at_ms: i64,
}

pub(crate) fn permanently_delete_storage_kind_for(
    database: &Database,
    paths: &StoragePaths,
    search_index: &SearchIndex,
    kind: ClipboardKind,
    expected: Option<KindStorageStats>,
) -> Result<StorageKindDeleteResult, String> {
    let KindDeleteResult { stats, deleted_ids } = match expected {
        Some(expected) => database
            .permanently_delete_by_kind_if_stats_match(kind, STORAGE_KIND_DELETE_SCOPE, expected)
            .map_err(|error| error.to_string())?,
        None => database
            .permanently_delete_by_kind(kind, STORAGE_KIND_DELETE_SCOPE)
            .map_err(|error| error.to_string())?,
    };
    let mut warnings = Vec::new();
    let search_sync = match SearchSynchronizer::default().sync_until_idle(database, search_index) {
        Ok(summary) => Some(summary),
        Err(error) => {
            warnings.push(format!("search index cleanup is pending: {error}"));
            None
        }
    };
    let removed_files = match cleanup_orphan_storage_files(database, paths) {
        Ok(cleanup) => cleanup.removed_files,
        Err(error) => {
            warnings.push(format!("managed resource cleanup is pending: {error}"));
            0
        }
    };

    Ok(StorageKindDeleteResult {
        deleted_count: stats.item_count,
        deleted_size_bytes: stats.size_bytes,
        removed_files,
        search_sync,
        warnings,
        deleted_ids,
    })
}
