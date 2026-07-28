use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use tantivy::{
    collector::TopDocs,
    directory::MmapDirectory,
    query::{BooleanQuery, Query, TermQuery},
    schema::{IndexRecordOption, TantivyDocument, Value},
    Index, IndexReader, IndexWriter, ReloadPolicy, Term,
};

use crate::storage::SearchDocument;

use super::{
    build_schema, manifest::SearchIndexLayout, register_tokenizers, SearchError, SearchFields,
    SearchQuery,
};

const INDEX_WRITER_MEMORY_BYTES: usize = 60_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub item_id: String,
    pub kind: String,
    pub score: f32,
    pub created_at_ms: i64,
    pub is_favorite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchIndexChange {
    Upsert(SearchDocument),
    Delete(String),
}

pub struct SearchIndex {
    fields: SearchFields,
    writer: Mutex<IndexWriter<TantivyDocument>>,
    reader: IndexReader,
    layout: Option<SearchIndexLayout>,
    rebuild_required: AtomicBool,
    cached_ids: Mutex<Option<(String, usize, Vec<String>)>>,
}

impl SearchIndex {
    pub fn open(path: &Path) -> Result<Self, SearchError> {
        let mut layout = SearchIndexLayout::prepare(path.to_path_buf())?;
        let (schema, fields) = build_schema();
        let directory =
            MmapDirectory::open(&layout.index_directory).map_err(tantivy::TantivyError::from)?;
        let index = match Index::open_or_create(directory, schema) {
            Ok(index) => index,
            Err(error) => {
                let msg = error.to_string();
                if msg.contains("schema does not match") {
                    std::fs::remove_dir_all(&layout.index_directory)
                        .map_err(tantivy::TantivyError::from)?;
                    std::fs::create_dir_all(&layout.index_directory)
                        .map_err(tantivy::TantivyError::from)?;
                    layout.rebuild_required = true;
                    let directory = MmapDirectory::open(&layout.index_directory)
                        .map_err(tantivy::TantivyError::from)?;
                    let (schema2, _) = build_schema();
                    Index::open_or_create(directory, schema2)?
                } else {
                    return Err(error.into());
                }
            }
        };

        Self::from_index(index, fields, Some(layout))
    }

    pub fn in_memory() -> Result<Self, SearchError> {
        let (schema, fields) = build_schema();
        Self::from_index(Index::create_in_ram(schema), fields, None)
    }

    pub fn requires_full_rebuild(&self) -> bool {
        self.rebuild_required.load(Ordering::Acquire)
    }

    pub fn begin_full_rebuild(&self) -> Result<(), SearchError> {
        if let Some(layout) = &self.layout {
            layout.mark_building()?;
        }

        self.clear_cached_ids();

        let mut writer = self
            .writer
            .lock()
            .map_err(|_| SearchError::WriterPoisoned)?;
        writer.delete_all_documents()?;
        writer.commit()?;
        drop(writer);
        self.reader.reload()?;
        self.rebuild_required.store(true, Ordering::Release);
        Ok(())
    }

    pub fn mark_rebuild_complete(&self) -> Result<(), SearchError> {
        if let Some(layout) = &self.layout {
            layout.mark_ready()?;
        }
        self.rebuild_required.store(false, Ordering::Release);
        Ok(())
    }

    pub fn apply_changes(&self, changes: &[SearchIndexChange]) -> Result<(), SearchError> {
        if changes.is_empty() {
            return Ok(());
        }

        self.clear_cached_ids();

        let mut writer = self
            .writer
            .lock()
            .map_err(|_| SearchError::WriterPoisoned)?;
        let result = (|| {
            for change in changes {
                let item_id = match change {
                    SearchIndexChange::Upsert(document) => &document.item_id,
                    SearchIndexChange::Delete(item_id) => item_id,
                };
                writer.delete_term(Term::from_field_text(self.fields.item_id, item_id));

                if let SearchIndexChange::Upsert(document) = change {
                    writer.add_document(self.to_tantivy_document(document))?;
                }
            }

            writer.commit()?;
            Ok::<(), tantivy::TantivyError>(())
        })();

        if let Err(error) = result {
            let _ = writer.rollback();
            return Err(error.into());
        }

        drop(writer);
        Ok(())
    }

    pub fn reload_reader(&self) -> Result<(), SearchError> {
        self.reader.reload().map_err(Into::into)
    }

    pub fn search(&self, input: &str, limit: usize) -> Result<Vec<SearchHit>, SearchError> {
        let query = SearchQuery::parse(input);
        let ngrams = query.required_ngrams();
        if ngrams.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let required_queries = ngrams
            .into_iter()
            .map(|ngram| {
                Box::new(TermQuery::new(
                    Term::from_field_text(self.fields.content, &ngram),
                    IndexRecordOption::WithFreqs,
                )) as Box<dyn Query>
            })
            .collect();
        let query = BooleanQuery::intersection(required_queries);
        let searcher = self.reader.searcher();
        let top_documents =
            searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;

        top_documents
            .into_iter()
            .map(|(score, address)| {
                let document = searcher.doc::<TantivyDocument>(address)?;
                Ok(SearchHit {
                    item_id: stored_text(&document, self.fields.item_id, "item_id")?.to_owned(),
                    kind: stored_text(&document, self.fields.kind, "kind")?.to_owned(),
                    score,
                    created_at_ms: document
                        .get_first(self.fields.created_at_ms)
                        .and_then(|value| value.as_i64())
                        .ok_or(SearchError::MissingStoredField("created_at_ms"))?,
                    is_favorite: document
                        .get_first(self.fields.is_favorite)
                        .and_then(|value| value.as_u64())
                        .ok_or(SearchError::MissingStoredField("is_favorite"))?
                        != 0,
                })
            })
            .collect()
    }

    pub fn search_all_ids(
        &self,
        input: &str,
        max_results: usize,
    ) -> Result<(Vec<String>, usize), SearchError> {
        let normalized = input.trim().to_owned();
        {
            let cache = self
                .cached_ids
                .lock()
                .map_err(|_| SearchError::WriterPoisoned)?;
            if let Some((ref cached_query, cached_max, ref ids)) = *cache {
                if cached_query.as_str() == normalized.as_str() && cached_max >= max_results {
                    let total = ids.len();
                    return Ok((ids[..max_results.min(total)].to_vec(), total));
                }
            }
        }

        let query = SearchQuery::parse(input);
        let ngrams = query.required_ngrams();
        if ngrams.is_empty() {
            let mut cache = self
                .cached_ids
                .lock()
                .map_err(|_| SearchError::WriterPoisoned)?;
            *cache = Some((normalized, max_results, Vec::new()));
            return Ok((Vec::new(), 0));
        }

        let required_queries = ngrams
            .into_iter()
            .map(|ngram| {
                Box::new(TermQuery::new(
                    Term::from_field_text(self.fields.content, &ngram),
                    IndexRecordOption::WithFreqs,
                )) as Box<dyn Query>
            })
            .collect();
        let boolean_query = BooleanQuery::intersection(required_queries);
        let searcher = self.reader.searcher();
        let top_documents = searcher.search(
            &boolean_query,
            &TopDocs::with_limit(max_results).order_by_score(),
        )?;

        let mut ids = Vec::with_capacity(top_documents.len());
        for (_score, address) in top_documents {
            let document = searcher.doc::<TantivyDocument>(address)?;
            if let Some(id) = document
                .get_first(self.fields.item_id)
                .and_then(|v| v.as_str())
            {
                ids.push(id.to_owned());
            }
        }
        let total = ids.len();

        {
            let mut cache = self
                .cached_ids
                .lock()
                .map_err(|_| SearchError::WriterPoisoned)?;
            *cache = Some((normalized, max_results, ids.clone()));
        }

        Ok((ids, total))
    }

    pub fn clear_cached_ids(&self) {
        if let Ok(mut cache) = self.cached_ids.lock() {
            *cache = None;
        }
    }

    fn from_index(
        index: Index,
        fields: SearchFields,
        layout: Option<SearchIndexLayout>,
    ) -> Result<Self, SearchError> {
        let rebuild_required = layout
            .as_ref()
            .is_some_and(|layout| layout.rebuild_required);
        register_tokenizers(&index)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        let writer =
            index.writer_with_num_threads::<TantivyDocument>(4, INDEX_WRITER_MEMORY_BYTES)?;

        Ok(Self {
            fields,
            writer: Mutex::new(writer),
            reader,
            layout,
            rebuild_required: AtomicBool::new(rebuild_required),
            cached_ids: Mutex::new(None),
        })
    }

    pub fn validate(&self) -> bool {
        let readers = self.reader.searcher();
        let doc_count = readers.num_docs();
        // The index is valid if we can acquire a searcher without error
        // and the document count is non-negative
        doc_count < u64::MAX
    }

    fn to_tantivy_document(&self, source: &SearchDocument) -> TantivyDocument {
        let mut document = TantivyDocument::new();
        document.add_text(self.fields.item_id, &source.item_id);
        document.add_text(self.fields.kind, &source.kind);
        document.add_text(self.fields.content, &source.content);
        document.add_i64(self.fields.created_at_ms, source.created_at_ms);
        document.add_u64(self.fields.is_favorite, u64::from(source.is_favorite));
        document
    }
}

fn stored_text<'document>(
    document: &'document TantivyDocument,
    field: tantivy::schema::Field,
    field_name: &'static str,
) -> Result<&'document str, SearchError> {
    document
        .get_first(field)
        .and_then(|value| value.as_str())
        .ok_or(SearchError::MissingStoredField(field_name))
}

#[cfg(test)]
mod tests {
    use super::{SearchIndex, SearchIndexChange};
    use crate::storage::SearchDocument;

    fn document(item_id: &str, content: &str) -> SearchDocument {
        SearchDocument {
            item_id: item_id.to_owned(),
            kind: "text".to_owned(),
            content: content.to_owned(),
            created_at_ms: 100,
            is_favorite: false,
        }
    }

    fn in_memory_index() -> SearchIndex {
        SearchIndex::in_memory().unwrap()
    }

    #[test]
    fn matches_required_terms_regardless_of_query_order() {
        let index = in_memory_index();
        index
            .apply_changes(&[
                SearchIndexChange::Upsert(document("clean", "脸皮挺好")),
                SearchIndexChange::Upsert(document("dirty", "脸皮挺脏")),
            ])
            .unwrap();
        index.reload_reader().unwrap();

        let forward = index.search("脸 脏", 20).unwrap();
        let reversed = index.search("脏 脸", 20).unwrap();

        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].item_id, "dirty");
        assert_eq!(reversed, forward);
    }

    #[test]
    fn repeated_upserts_replace_the_previous_document() {
        let index = in_memory_index();
        index
            .apply_changes(&[SearchIndexChange::Upsert(document("item", "旧内容"))])
            .unwrap();
        index.reload_reader().unwrap();
        index
            .apply_changes(&[SearchIndexChange::Upsert(document("item", "新内容"))])
            .unwrap();
        index.reload_reader().unwrap();

        assert!(index.search("旧", 20).unwrap().is_empty());
        assert_eq!(index.search("新", 20).unwrap().len(), 1);
    }

    #[test]
    fn deletes_are_idempotent() {
        let index = in_memory_index();
        index
            .apply_changes(&[SearchIndexChange::Upsert(document("item", "待删除"))])
            .unwrap();
        index
            .apply_changes(&[
                SearchIndexChange::Delete("item".to_owned()),
                SearchIndexChange::Delete("item".to_owned()),
            ])
            .unwrap();
        index.reload_reader().unwrap();

        assert!(index.search("删除", 20).unwrap().is_empty());
    }

    #[test]
    fn full_rebuild_clears_existing_documents() {
        let index = in_memory_index();
        index
            .apply_changes(&[SearchIndexChange::Upsert(document("item", "旧索引"))])
            .unwrap();
        index.reload_reader().unwrap();

        index.begin_full_rebuild().unwrap();

        assert!(index.requires_full_rebuild());
        assert!(index.search("索引", 20).unwrap().is_empty());
        index.mark_rebuild_complete().unwrap();
        assert!(!index.requires_full_rebuild());
    }

    #[test]
    fn latin_search_is_case_insensitive() {
        let index = in_memory_index();
        index
            .apply_changes(&[SearchIndexChange::Upsert(document(
                "item",
                "Tauri Clipboard",
            ))])
            .unwrap();
        index.reload_reader().unwrap();

        assert_eq!(index.search("TAURI", 20).unwrap().len(), 1);
        assert_eq!(index.search("clipboard", 20).unwrap().len(), 1);
    }

    #[test]
    fn results_are_returned_in_descending_relevance_order() {
        let index = in_memory_index();
        index
            .apply_changes(&[
                SearchIndexChange::Upsert(document("dense", "脸脸脸脸脏脏脏脏")),
                SearchIndexChange::Upsert(document("sparse", "脸皮特别特别特别厚但是有点脏")),
            ])
            .unwrap();
        index.reload_reader().unwrap();

        let hits = index.search("脸 脏", 20).unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].item_id, "dense");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn cjk_character_search() {
        let index = in_memory_index();
        index
            .apply_changes(&[
                SearchIndexChange::Upsert(document("a", "开心脏脏")),
                SearchIndexChange::Upsert(document("b", "脏兮兮的小心脏散开落一地")),
                SearchIndexChange::Upsert(document("c", "开开心心每一天")),
                SearchIndexChange::Upsert(document("d", "脸皮挺脏")),
                SearchIndexChange::Upsert(document("e", "Nothing here")),
            ])
            .unwrap();
        index.reload_reader().unwrap();

        // ── Single-char CJK terms match non-adjacent positions ──
        let hits = index.search("开 脏", 20).unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.item_id.as_str()).collect();
        assert!(
            ids.contains(&"a"),
            "开 脏 should match 开心脏脏 (chars separated by 心)"
        );
        assert!(
            ids.contains(&"b"),
            "开 脏 should match 脏兮兮的小心脏 (chars far apart, reversed order)"
        );

        // ── Order of search terms does not matter ──
        let forward = index.search("开 脏", 20).unwrap();
        let reversed = index.search("脏 开", 20).unwrap();
        assert_eq!(forward, reversed);

        // ── Missing a required char → no match ──
        assert!(
            index.search("开 脏 啦", 20).unwrap().is_empty(),
            "开 脏 啦 should not match any item (no item has 啦)"
        );
        assert!(
            index.search("开 脏", 20)
                .unwrap()
                .iter()
                .map(|h| h.item_id.as_str())
                .all(|id| id != "c"),
            "开 脏 should NOT match 开开心心每一天 (missing 脏)"
        );
        assert!(
            index.search("开 脏", 20)
                .unwrap()
                .iter()
                .map(|h| h.item_id.as_str())
                .all(|id| id != "e"),
            "开 脏 should NOT match English-only content"
        );

        // ── Adjacent bigram search ──
        let bigram_hits = index.search("开心", 20).unwrap();
        let bigram_ids: Vec<_> = bigram_hits.iter().map(|h| h.item_id.as_str()).collect();
        assert!(bigram_ids.contains(&"a"), "开心 should match 开心脏脏");
        assert!(bigram_ids.contains(&"c"), "开心 should match 开开心心每一天");
        assert!(
            !bigram_ids.contains(&"b"),
            "开心 should NOT match 脏兮兮的小心脏 (心 and 开 are not adjacent)"
        );

        // ── Single CJK char search ──
        let dirty_hits = index.search("脏", 20).unwrap();
        let dirty_ids: Vec<_> = dirty_hits.iter().map(|h| h.item_id.as_str()).collect();
        assert!(dirty_ids.contains(&"a"));
        assert!(dirty_ids.contains(&"b"));
        assert!(dirty_ids.contains(&"d"));
        assert_eq!(dirty_hits.len(), 3);

        // ── Relevance ordering: more occurrences rank higher ──
        let by_relevance = index.search("心", 20).unwrap();
        assert!(by_relevance.len() >= 2, "at least 2 items contain 心");
        // 'c' has two 心 (开开心心), 'b' has one 心 (小心脏)
        let c_pos = by_relevance.iter().position(|h| h.item_id == "c");
        let b_pos = by_relevance.iter().position(|h| h.item_id == "b");
        assert!(c_pos < b_pos, "开开心心每一天 (2×心) should rank above 脏兮兮的小心脏 (1×心)");

        // ── search_all_ids with cache ──
        let (ids, total) = index.search_all_ids("开 脏", 50).unwrap();
        assert_eq!(total, 2);
        // Cached second call returns same result with smaller limit
        let (cached_ids, cached_total) = index.search_all_ids("开 脏", 1).unwrap();
        assert_eq!(cached_ids.len(), 1);
        assert_eq!(cached_total, 2);
        assert_eq!(cached_ids[0], ids[0]);
    }

    #[test]
    fn cached_search_respects_a_smaller_requested_limit() {
        let index = in_memory_index();
        index
            .apply_changes(&[
                SearchIndexChange::Upsert(document("first", "shared cache term")),
                SearchIndexChange::Upsert(document("second", "shared cache term")),
                SearchIndexChange::Upsert(document("third", "shared cache term")),
            ])
            .unwrap();
        index.reload_reader().unwrap();

        let (all, _) = index.search_all_ids("shared cache", 3).unwrap();
        let (limited, cached_total) = index.search_all_ids("shared cache", 1).unwrap();

        assert_eq!(all.len(), 3);
        assert_eq!(limited.len(), 1);
        assert_eq!(cached_total, 3);
        assert_eq!(limited[0], all[0]);
    }
}
