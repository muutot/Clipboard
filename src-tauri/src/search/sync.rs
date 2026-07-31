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

    fn resolve_changes(
        &self,
        repository: &impl SearchRepository,
        item_ids: &[String],
    ) -> Result<(Vec<SearchIndexChange>, u64, u64), SearchError> {
        let documents = repository.get_search_documents(item_ids)?;
        let mut document_map = std::collections::HashMap::new();
        for doc in documents {
            document_map.insert(doc.item_id.clone(), doc);
        }

        let mut changes = Vec::with_capacity(item_ids.len());
        let mut upserted = 0u64;
        let mut deleted = 0u64;

        for item_id in item_ids {
            if let Some(document) = document_map.remove(item_id) {
                changes.push(SearchIndexChange::Upsert(document));
                upserted += 1;
            } else {
                changes.push(SearchIndexChange::Delete(item_id.clone()));
                deleted += 1;
            }
        }

        Ok((changes, upserted, deleted))
    }

    fn process_single_batch(
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
            .collect::<std::collections::HashSet<_>>();
        let item_ids_vec: Vec<String> = item_ids.into_iter().collect();

        let (changes, upserted_documents, deleted_documents) =
            self.resolve_changes(repository, &item_ids_vec)?;

        index.apply_search_changes(&changes)?;
        repository.acknowledge_search_outbox(last_sequence)?;

        Ok(SearchSyncSummary {
            processed_events: events.len() as u64,
            upserted_documents,
            deleted_documents,
            last_sequence: Some(last_sequence),
        })
    }

    pub fn sync_batch(
        &self,
        repository: &impl SearchRepository,
        index: &impl SearchIndexSink,
    ) -> Result<SearchSyncSummary, SearchError> {
        self.process_single_batch(repository, index)
    }

    pub fn sync_bounded(
        &self,
        repository: &impl SearchRepository,
        index: &impl SearchIndexSink,
        max_batches: usize,
    ) -> Result<SearchSyncSummary, SearchError> {
        let mut total = SearchSyncSummary::default();

        for _ in 0..max_batches {
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

        Ok(total)
    }

    pub fn sync_until_idle(
        &self,
        repository: &impl SearchRepository,
        index: &SearchIndex,
    ) -> Result<SearchSyncSummary, SearchError> {
        let mut total = SearchSyncSummary::default();

        loop {
            let events = repository.read_search_outbox(self.batch_size)?;
            let Some(last_sequence) = events.last().map(|event| event.sequence) else {
                return Ok(total);
            };

            let mut item_ids_vec: Vec<String> = Vec::with_capacity(events.len());
            {
                let mut seen = std::collections::HashSet::with_capacity(events.len());
                for event in &events {
                    if seen.insert(event.item_id.as_str()) {
                        item_ids_vec.push(event.item_id.clone());
                    }
                }
            }
            let event_count = events.len() as u64;

            let (changes, upserted, deleted) = self.resolve_changes(repository, &item_ids_vec)?;
            index.apply_changes(&changes)?;
            repository.acknowledge_search_outbox(last_sequence)?;

            total.processed_events += event_count;
            total.upserted_documents += upserted;
            total.deleted_documents += deleted;
            total.last_sequence = Some(last_sequence);

            if event_count < u64::from(self.batch_size) {
                break;
            }
        }

        index.reload_reader()?;
        Ok(total)
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
        domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus},
        search::{SearchError, SearchIndex, SearchIndexChange, SearchIndexSink},
        storage::{ClipboardRepository, Database, OcrRepository, SearchRepository},
    };

    use super::SearchSynchronizer;

    fn item(id: &str, title: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: title.to_owned(),
            text_content: Some("脸皮挺脏".to_owned()),
            html_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 32,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    fn image_item(id: &str) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Image,
            title: "screenshot.png".to_owned(),
            text_content: None,
            html_content: None,
            resource_path: Some("image/screenshot.png".to_owned()),
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 1_024,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
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

    #[test]
    fn completed_ocr_text_returns_the_corresponding_image() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&image_item("image")).unwrap();
        database
            .save_ocr_result(&OcrResult {
                item_id: "image".to_owned(),
                status: OcrStatus::Completed,
                engine: "test".to_owned(),
                model_version: "1".to_owned(),
                language: Some("zh-CN".to_owned()),
                full_text: "截图里面的脸有点脏".to_owned(),
                blocks: vec![],
                image_hash: "hash-image".to_owned(),
                created_at_ms: 100,
                completed_at_ms: Some(200),
                error_message: None,
            })
            .unwrap();
        let index = SearchIndex::in_memory().unwrap();

        SearchSynchronizer::default()
            .sync_until_idle(&database, &index)
            .unwrap();
        let hits = index.search("截图 脏", 20).unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item_id, "image");
        assert_eq!(hits[0].kind, "image");
    }
}
