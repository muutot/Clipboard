//! User-configured auto-tag rules: `{pattern, tag}` pairs applied to captured
//! text at ingestion time.
//!
//! Invalid regexes are skipped loudly (logged with the offending pattern) so
//! a broken rule cannot silently leave the user believing it protects — the
//! same posture as invalid sensitive-content patterns. An empty compiled set
//! simply tags nothing; tagging is best-effort, never a capture gate.

use crate::config::AutoTagRule;

/// One compiled auto-tag rule.
#[derive(Debug, Clone)]
pub struct CompiledAutoTagRule {
    pub tag: String,
    pattern: regex_lite::Regex,
}

/// Compiles configured rules, skipping entries whose pattern fails to parse.
pub fn compile_auto_tag_rules(rules: &[AutoTagRule]) -> Vec<CompiledAutoTagRule> {
    let mut compiled = Vec::with_capacity(rules.len());
    for rule in rules {
        match regex_lite::Regex::new(&rule.pattern) {
            Ok(pattern) => compiled.push(CompiledAutoTagRule {
                tag: rule.tag.clone(),
                pattern,
            }),
            Err(error) => {
                crate::log_event!(
                    "[autotag] ignoring invalid auto-tag pattern {:?}: {error}",
                    rule.pattern
                );
            }
        }
    }
    compiled
}

/// Tags whose rule pattern matches `text`, in rule order without duplicates.
pub fn match_auto_tags(rules: &[CompiledAutoTagRule], text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for rule in rules {
        if rule.pattern.is_match(text) && !tags.iter().any(|tag| tag == &rule.tag) {
            tags.push(rule.tag.clone());
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, tag: &str) -> AutoTagRule {
        AutoTagRule {
            pattern: pattern.to_owned(),
            tag: tag.to_owned(),
        }
    }

    #[test]
    fn matching_returns_rule_order_without_duplicates() {
        let rules = compile_auto_tag_rules(&[
            rule(r"TODO|FIXME", "todo"),
            rule(r"FIXME", "urgent"),
            rule(r"TODO", "todo"),
        ]);
        assert_eq!(rules.len(), 3);
        assert_eq!(
            match_auto_tags(&rules, "FIXME: finish this TODO"),
            vec!["todo".to_owned(), "urgent".to_owned()]
        );
        assert!(match_auto_tags(&rules, "nothing relevant").is_empty());
    }

    #[test]
    fn invalid_patterns_are_skipped_loudly() {
        let rules = compile_auto_tag_rules(&[rule(r"(\w+", "broken"), rule(r"ok", "fine")]);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].tag, "fine");
    }

    #[test]
    fn empty_rule_set_tags_nothing() {
        assert!(match_auto_tags(&[], "anything").is_empty());
    }
}
