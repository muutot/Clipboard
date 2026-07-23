use std::collections::BTreeSet;

use serde::Serialize;

use crate::storage::SearchRepository;

use super::{SearchError, SearchIndex, SearchIndexChange};

pub trait SearchIndexSink {
    fn apply_search_changes(&self, changes: &[SearchIndexChange]) -> Result<(), SearchError>;
}

impl SearchIndexSink for SearchIndex {
    fn apply_search_changes(&self, changes: &[SearchIndexChange]) -> Result<(), SearchError> {
        self.apply_changes(changes)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSyncSummary {
    pub processed_events: u64,
    pub upserted_documents: u64,
    pub deleted_documents: u64,
    pub last_sequence: Option<i64>,
}

pub struct SearchSynchronizer {
    batch_size: u32,
}

impl SearchSynchronizer {
    pub fn new(batch_size: u32) -> Self {
        Self {
            batch_size: batch_size.clamp(1, 1_000),
        }
    }

    pub fn sync_batch(
        &self,
        repository: &impl SearchRepository,
        index: &impl SearchIndexSink,
    ) -> Result<SearchSyncSummary, SearchError> {
        let events = repository.read_search_outbox(self.batch_size)?;
        let Some(last_sequence) = events.last().map(|event| event.sequence) else {
            return Ok(SearchSyncSummary::default());
        };

        let item_ids = events
            .iter()
            .map(|event| event.item_id.clone())
            .collect::<BTreeSet<_>>();
        let mut changes = Vec::with_capacity(item_ids.len());
        let mut upserted_documents = 0;
        let mut deleted_documents = 0;

        for item_id in item_ids {
            if let Some(document) = repository.get_search_document(&item_id)? {
                changes.push(SearchIndexChange::Upsert(document));
                upserted_documents += 1;
            } else {
                changes.push(SearchIndexChange::Delete(item_id));
                deleted_documents += 1;
            }
        }

        index.apply_search_changes(&changes)?;
        repository.acknowledge_search_outbox(last_sequence)?;

        Ok(SearchSyncSummary {
            processed_events: events.len() as u64,
            upserted_documents,
            deleted_documents,
            last_sequence: Some(last_sequence),
        })
    }

    pub fn sync_until_idle(
        &self,
        repository: &impl SearchRepository,
        index: &impl SearchIndexSink,
    ) -> Result<SearchSyncSummary, SearchError> {
        let mut total = SearchSyncSummary::default();

        loop {
            let batch = self.sync_batch(repository, index)?;
            total.processed_events += batch.processed_events;
            total.upserted_documents += batch.upserted_documents;
            total.deleted_documents += batch.deleted_documents;
            if batch.last_sequence.is_some() {
                total.last_sequence = batch.last_sequence;
            }

            if batch.processed_events < u64::from(self.batch_size) {
                return Ok(total);
            }
        }
    }

    pub fn initialize(
        &self,
        repository: &impl SearchRepository,
        index: &SearchIndex,
    ) -> Result<SearchSyncSummary, SearchError> {
        if index.requires_full_rebuild() {
            self.rebuild(repository, index)
        } else {
            self.sync_until_idle(repository, index)
        }
    }

    pub fn rebuild(
        &self,
        repository: &impl SearchRepository,
        index: &SearchIndex,
    ) -> Result<SearchSyncSummary, SearchError> {
        index.begin_full_rebuild()?;
        repository.enqueue_full_search_rebuild()?;
        let summary = self.sync_until_idle(repository, index)?;
        index.mark_rebuild_complete()?;
        Ok(summary)
    }
}

impl Default for SearchSynchronizer {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{ClipboardItem, ClipboardKind},
        search::{SearchError, SearchIndex, SearchIndexChange, SearchIndexSink},
        storage::{ClipboardRepository, Database, SearchRepository},
    };

    use super::SearchSynchronizer;

    fn item(id: &str, title: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: title.to_owned(),
            text_content: Some("脸皮挺脏".to_owned()),
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 32,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
        }
    }

    #[test]
    fn commits_the_index_before_acknowledging_outbox_events() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&item("match", "匹配项")).unwrap();
        database.save_item(&item("other", "其他项")).unwrap();
        let index = SearchIndex::in_memory().unwrap();

        let summary = SearchSynchronizer::default()
            .sync_until_idle(&database, &index)
            .unwrap();

        assert_eq!(summary.processed_events, 2);
        assert_eq!(summary.upserted_documents, 2);
        assert_eq!(index.search("脸 脏", 20).unwrap().len(), 2);
        assert!(database.read_search_outbox(100).unwrap().is_empty());
    }

    #[test]
    fn collapses_repeated_events_to_the_latest_document_snapshot() {
        let database = Database::open_in_memory().unwrap();
        let first = item("item", "旧标题");
        database.save_item(&first).unwrap();
        let mut updated = first;
        updated.title = "新标题".to_owned();
        updated.created_at_ms = 200;
        database.save_item(&updated).unwrap();
        let index = SearchIndex::in_memory().unwrap();

        let summary = SearchSynchronizer::default()
            .sync_until_idle(&database, &index)
            .unwrap();

        assert_eq!(summary.processed_events, 2);
        assert_eq!(summary.upserted_documents, 1);
        assert!(index.search("旧标题", 20).unwrap().is_empty());
        assert_eq!(index.search("新标题", 20).unwrap().len(), 1);
    }

    #[test]
    fn leaves_events_pending_when_the_index_commit_fails() {
        struct FailingIndex;

        impl SearchIndexSink for FailingIndex {
            fn apply_search_changes(
                &self,
                _changes: &[SearchIndexChange],
            ) -> Result<(), SearchError> {
                Err(SearchError::WriterPoisoned)
            }
        }

        let database = Database::open_in_memory().unwrap();
        database.save_item(&item("item", "待重试")).unwrap();

        let result = SearchSynchronizer::default().sync_batch(&database, &FailingIndex);

        assert!(result.is_err());
        assert_eq!(database.read_search_outbox(100).unwrap().len(), 1);
    }

    #[test]
    fn rebuild_recovers_records_even_without_pending_events() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&item("item", "重建可见")).unwrap();
        let pending = database.read_search_outbox(100).unwrap();
        database
            .acknowledge_search_outbox(pending.last().unwrap().sequence)
            .unwrap();
        let index = SearchIndex::in_memory().unwrap();

        let summary = SearchSynchronizer::default()
            .rebuild(&database, &index)
            .unwrap();

        assert_eq!(summary.upserted_documents, 1);
        assert_eq!(index.search("重建", 20).unwrap().len(), 1);
        assert!(!index.requires_full_rebuild());
        assert!(database.read_search_outbox(100).unwrap().is_empty());
    }
}
