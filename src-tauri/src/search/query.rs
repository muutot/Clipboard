use std::collections::BTreeSet;

/// A normalized, order-independent search query.
///
/// Every term is required to match. The terms are sorted and deduplicated so
/// queries such as `脸 脏` and `脏 脸` share the same representation and cache key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    terms: Vec<String>,
}

impl SearchQuery {
    pub fn parse(input: &str) -> Self {
        let mut terms = input
            .split_whitespace()
            .map(str::trim)
            .filter(|term| !term.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>();

        terms.sort_unstable();
        terms.dedup();

        Self { terms }
    }

    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    /// Returns the longest n-grams available in the 1-3 character index for
    /// every required term. Keeping short terms intact preserves adjacency:
    /// `脸 脏` becomes two clauses while `脸脏` remains one two-character clause.
    pub fn required_ngrams(&self) -> Vec<String> {
        let mut ngrams = BTreeSet::new();

        for term in &self.terms {
            let characters = term.chars().collect::<Vec<_>>();
            if characters.len() <= 3 {
                ngrams.insert(term.clone());
                continue;
            }

            for window in characters.windows(3) {
                ngrams.insert(window.iter().collect());
            }
        }

        ngrams.into_iter().collect()
    }

    /// Baseline matcher used to verify query semantics before the Tantivy
    /// implementation is connected. Production search will translate every
    /// term into a Tantivy MUST clause instead of scanning records.
    pub fn matches(&self, candidate: &str) -> bool {
        let normalized_candidate = candidate.to_lowercase();
        self.terms
            .iter()
            .all(|term| normalized_candidate.contains(term))
    }
}

#[cfg(test)]
mod tests {
    use super::SearchQuery;

    #[test]
    fn term_order_does_not_change_the_query() {
        assert_eq!(SearchQuery::parse("脸 脏"), SearchQuery::parse("脏 脸"));
    }

    #[test]
    fn all_terms_are_required() {
        let query = SearchQuery::parse("脸 脏");

        assert!(!query.matches("脸皮挺好"));
        assert!(query.matches("脸皮挺脏"));
    }

    #[test]
    fn duplicate_terms_are_removed() {
        let query = SearchQuery::parse("脸 脏 脸");

        assert_eq!(query.terms().len(), 2);
        assert!(query.terms().contains(&"脸".to_string()));
        assert!(query.terms().contains(&"脏".to_string()));
    }

    #[test]
    fn spaced_terms_remain_independent_required_ngrams() {
        let ngrams = SearchQuery::parse("脸 脏").required_ngrams();

        assert_eq!(ngrams.len(), 2);
        assert!(ngrams.contains(&"脸".to_owned()));
        assert!(ngrams.contains(&"脏".to_owned()));
    }

    #[test]
    fn adjacent_short_terms_are_not_split_apart() {
        assert_eq!(
            SearchQuery::parse("脸脏").required_ngrams(),
            vec!["脸脏".to_owned()]
        );
    }

    #[test]
    fn long_terms_use_overlapping_trigrams() {
        let ngrams = SearchQuery::parse("脸皮挺脏").required_ngrams();

        assert_eq!(ngrams.len(), 2);
        assert!(ngrams.contains(&"脸皮挺".to_owned()));
        assert!(ngrams.contains(&"皮挺脏".to_owned()));
    }

    // ── Task 4: CJK character tokenization ──

    #[test]
    fn cjk_single_character_is_untouched() {
        let query = SearchQuery::parse("中");
        assert_eq!(query.terms(), &["中"]);
        assert_eq!(query.required_ngrams(), vec!["中"]);
    }

    #[test]
    fn cjk_two_characters_form_one_ngram() {
        let query = SearchQuery::parse("中文");
        assert_eq!(query.required_ngrams().len(), 1);
        assert!(query.matches("中文测试"));
    }

    #[test]
    fn cjk_three_characters_form_one_ngram() {
        let ngrams = SearchQuery::parse("中国人").required_ngrams();
        assert_eq!(ngrams, vec!["中国人"]);
    }

    #[test]
    fn cjk_four_characters_form_two_overlapping_trigrams() {
        let ngrams = SearchQuery::parse("中国人民").required_ngrams();
        assert_eq!(ngrams.len(), 2);
        assert!(ngrams.contains(&"中国人".to_owned()));
        assert!(ngrams.contains(&"国人民".to_owned()));
    }

    #[test]
    fn cjk_japanese_hiragana() {
        let query = SearchQuery::parse("こんにちは");
        assert_eq!(query.terms(), &["こんにちは"]);
        assert_eq!(query.required_ngrams().len(), 3);
    }

    // ── Task 4: Mixed CJK + ASCII ──

    #[test]
    fn mixed_cjk_and_ascii_separated_by_space() {
        let query = SearchQuery::parse("hello 中文 test");
        assert_eq!(query.terms().len(), 3);
        assert!(query.terms().iter().any(|t| t == "hello"));
        assert!(query.terms().iter().any(|t| t == "中文"));
        assert!(query.terms().iter().any(|t| t == "test"));
    }

    #[test]
    fn mixed_cjk_and_ascii_no_spaces() {
        let query = SearchQuery::parse("hello中文test");
        let ngrams = query.required_ngrams();
        // The entire string is one term (no whitespace), producing n-grams over all chars
        assert!(!ngrams.is_empty());
    }

    #[test]
    fn numbers_are_handled_as_terms() {
        let query = SearchQuery::parse("12345");
        assert_eq!(query.terms().len(), 1);
    }

    // ── Task 4: Very long queries ──

    #[test]
    fn very_long_query_does_not_panic() {
        let long = "a".repeat(10_000);
        let query = SearchQuery::parse(&long);
        assert!(!query.terms().is_empty());
    }

    #[test]
    fn very_long_query_with_spaces() {
        let long = (0..200)
            .map(|i| format!("term{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let query = SearchQuery::parse(&long);
        assert_eq!(query.terms().len(), 200);
        // All terms should be deduplicated (they are all unique here)
    }

    // ── Task 4: Empty and single-char queries ──

    #[test]
    fn empty_query_has_no_terms() {
        let query = SearchQuery::parse("");
        assert!(query.terms().is_empty());
        assert!(query.required_ngrams().is_empty());
    }

    #[test]
    fn whitespace_only_query_has_no_terms() {
        let query = SearchQuery::parse("   \t  \n  ");
        assert!(query.terms().is_empty());
    }

    #[test]
    fn single_char_query_is_preserved() {
        let query = SearchQuery::parse("a");
        assert_eq!(query.terms(), &["a"]);
        assert_eq!(query.required_ngrams(), vec!["a"]);
    }

    #[test]
    fn single_char_chinese_query() {
        let query = SearchQuery::parse("脸");
        assert_eq!(query.required_ngrams(), vec!["脸"]);
    }

    // ── Task 4: Case insensitivity ──

    #[test]
    fn upper_case_query_is_lowercased() {
        let query = SearchQuery::parse("HELLO WORLD");
        assert!(query.terms().iter().any(|t| t == "hello"));
        assert!(query.terms().iter().any(|t| t == "world"));
    }

    // ── Task 4: Matches method coverage ──

    #[test]
    fn matches_with_empty_query() {
        let query = SearchQuery::parse("");
        assert!(query.matches("anything"));
    }

    #[test]
    fn matches_with_exact_substring() {
        let query = SearchQuery::parse("clip");
        assert!(query.matches("clipboard"));
        assert!(!query.matches("board"));
    }
}
