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
}
