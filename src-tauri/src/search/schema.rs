use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, FAST, INDEXED, STORED, STRING,
};
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::Index;

pub const NGRAM_TOKENIZER_NAME: &str = "cjk_ngram_1_3";

#[derive(Debug, Clone, Copy)]
pub struct SearchFields {
    pub item_id: Field,
    pub kind: Field,
    pub content: Field,
    pub created_at_ms: Field,
    pub is_favorite: Field,
}

pub fn build_schema() -> (Schema, SearchFields) {
    let mut builder = Schema::builder();
    let item_id = builder.add_text_field("item_id", STRING | STORED);
    let kind = builder.add_text_field("kind", STRING | STORED);
    let content_indexing = TextFieldIndexing::default()
        .set_tokenizer(NGRAM_TOKENIZER_NAME)
        .set_index_option(IndexRecordOption::WithFreqs);
    let content_options = TextOptions::default().set_indexing_options(content_indexing);
    let content = builder.add_text_field("content", content_options);
    let created_at_ms = builder.add_i64_field("created_at_ms", FAST | STORED);
    let is_favorite = builder.add_u64_field("is_favorite", INDEXED | FAST | STORED);
    let schema = builder.build();

    (
        schema,
        SearchFields {
            item_id,
            kind,
            content,
            created_at_ms,
            is_favorite,
        },
    )
}

pub fn register_tokenizers(index: &Index) -> tantivy::Result<()> {
    let analyzer = TextAnalyzer::builder(NgramTokenizer::all_ngrams(1, 3)?)
        .filter(LowerCaser)
        .build();
    index.tokenizers().register(NGRAM_TOKENIZER_NAME, analyzer);
    Ok(())
}

#[cfg(test)]
mod tests {
    use tantivy::tokenizer::{NgramTokenizer, TokenStream, Tokenizer};
    use tantivy::Index;

    use super::{build_schema, register_tokenizers};

    #[test]
    fn registers_the_expected_search_fields() {
        let (schema, fields) = build_schema();
        let index = Index::create_in_ram(schema.clone());
        register_tokenizers(&index).unwrap();

        assert_eq!(schema.get_field_name(fields.item_id), "item_id");
        assert_eq!(schema.get_field_name(fields.kind), "kind");
        assert_eq!(schema.get_field_name(fields.content), "content");
        assert_eq!(schema.get_field_name(fields.created_at_ms), "created_at_ms");
        assert_eq!(schema.get_field_name(fields.is_favorite), "is_favorite");
    }

    #[test]
    fn emits_one_to_three_character_chinese_ngrams() {
        let mut tokenizer = NgramTokenizer::all_ngrams(1, 3).unwrap();
        let mut stream = tokenizer.token_stream("脸皮挺脏");
        let mut tokens = Vec::new();
        while stream.advance() {
            tokens.push(stream.token().text.clone());
        }

        assert!(tokens.contains(&"脸".to_owned()));
        assert!(tokens.contains(&"脸皮".to_owned()));
        assert!(tokens.contains(&"脸皮挺".to_owned()));
        assert!(tokens.contains(&"皮挺脏".to_owned()));
        assert!(!tokens.contains(&"脸皮挺脏".to_owned()));
    }

    #[test]
    fn registered_analyzer_normalizes_latin_letter_case() {
        let (schema, fields) = build_schema();
        let index = Index::create_in_ram(schema);
        register_tokenizers(&index).unwrap();
        let mut analyzer = index.tokenizer_for_field(fields.content).unwrap();
        let mut stream = analyzer.token_stream("AbC");
        let mut tokens = Vec::new();
        while stream.advance() {
            tokens.push(stream.token().text.clone());
        }

        assert!(tokens.contains(&"abc".to_owned()));
        assert!(!tokens.contains(&"AbC".to_owned()));
    }
}
