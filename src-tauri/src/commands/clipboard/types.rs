use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::super::cleanup::cleanup_orphan_storage_files;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::search::{SearchIndex, SearchSyncSummary, SearchSynchronizer};
use crate::storage::{
    ClipboardRepository, Database, KindDeleteResult, KindStorageStats, StoragePaths,
};
use crate::STORAGE_KIND_DELETE_SCOPE;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSortRule {
    pub(crate) field: SearchSortField,
    pub(crate) direction: SearchSortDirection,
}

pub struct SearchResultCache {
    inner: Mutex<Option<CachedSearchResult>>,
}

pub(crate) type CachedSearchResult = (String, Vec<SearchSortRule>, usize, Vec<ClipboardItem>);

impl Default for SearchResultCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchResultCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    pub fn get(
        &self,
        query: &str,
        rules: &[SearchSortRule],
        max_results: usize,
        offset: usize,
        limit: usize,
    ) -> Option<Vec<ClipboardItem>> {
        let cache = self.inner.lock().ok()?;
        let (cached_query, cached_rules, cached_max, cached_items) = cache.as_ref()?;
        if cached_query != query || cached_rules != rules || *cached_max < max_results {
            return None;
        }
        let total = cached_items.len();
        if offset >= total {
            return Some(Vec::new());
        }
        let end = (offset + limit).min(total);
        Some(cached_items[offset..end].to_vec())
    }

    pub fn set(
        &self,
        query: String,
        rules: Vec<SearchSortRule>,
        max_results: usize,
        items: Vec<ClipboardItem>,
    ) {
        if let Ok(mut cache) = self.inner.lock() {
            *cache = Some((query, rules, max_results, items));
        }
    }

    pub fn clear(&self) {
        if let Ok(mut cache) = self.inner.lock() {
            *cache = None;
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipboardHistoryInvalidated {
    pub(crate) deleted_ids: Vec<String>,
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
