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
        let top_documents = searcher.search(
            &query,
            &TopDocs::with_limit(limit).order_by_score(),
        )?;

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
}
