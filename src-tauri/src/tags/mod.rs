//! User-configured auto-tag rules with conjunctive regex/source/type conditions.
//! Capture and explicit historical application share this matcher; media match titles.
//!
//! Invalid regexes are skipped loudly (logged with the offending pattern) so
//! a broken rule cannot silently leave the user believing it protects — the
//! same posture as invalid sensitive-content patterns. An empty compiled set
//! simply tags nothing; tagging is best-effort, never a capture gate.

use crate::config::AutoTagRule;
use crate::domain::ClipboardKind;

/// One compiled auto-tag rule.
#[derive(Debug, Clone)]
pub struct CompiledAutoTagRule {
    pub tag: String,
    pattern: regex_lite::Regex,
    source_app: String,
    kind: Option<ClipboardKind>,
}

/// Compiles configured rules, skipping entries whose pattern fails to parse.
pub fn compile_auto_tag_rules(rules: &[AutoTagRule]) -> Vec<CompiledAutoTagRule> {
    let mut compiled = Vec::with_capacity(rules.len());
    for rule in rules {
        match regex_lite::Regex::new(&rule.pattern) {
            Ok(pattern) => compiled.push(CompiledAutoTagRule {
                tag: rule.tag.clone(),
                pattern,
                source_app: rule.source_app.to_lowercase(),
                kind: rule.kind,
            }),
            Err(error) => {
                crate::log_warn!(
                    "[autotag] ignoring invalid auto-tag pattern {:?}: {error}",
                    rule.pattern
                );
            }
        }
    }
    compiled
}

/// Tags whose conditions all match, in rule order without duplicates.
pub fn match_auto_tags(
    rules: &[CompiledAutoTagRule],
    text: &str,
    source_app: &str,
    kind: ClipboardKind,
) -> Vec<String> {
    let mut tags = Vec::new();
    let source_app = source_app.to_lowercase();
    for rule in rules {
        if rule.kind.is_none_or(|expected| expected == kind)
            && source_app.contains(&rule.source_app)
            && rule.pattern.is_match(text)
            && !tags.iter().any(|tag| tag == &rule.tag)
        {
            tags.push(rule.tag.clone());
        }
    }
    tags
}

pub fn validate_auto_tag_rules(rules: &[AutoTagRule]) -> Result<(), String> {
    if rules.len() > crate::config::ConfigStore::MAX_AUTO_TAG_RULES {
        return Err("at most 100 auto-tag rules are supported".into());
    }
    for rule in rules {
        let rule = rule.normalized();
        regex_lite::Regex::new(&rule.pattern)
            .map_err(|error| format!("invalid auto-tag pattern: {error}"))?;
        if rule.tag.is_empty() {
            return Err("auto-tag rule tag must not be empty".into());
        }
        if rule.pattern.is_empty() && rule.source_app.is_empty() && rule.kind.is_none() {
            return Err("an auto-tag rule needs a pattern, source or type condition".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn match_auto_tags(rules: &[CompiledAutoTagRule], text: &str) -> Vec<String> {
        super::match_auto_tags(rules, text, "", ClipboardKind::Text)
    }

    fn rule(pattern: &str, tag: &str) -> AutoTagRule {
        AutoTagRule {
            pattern: pattern.to_owned(),
            tag: tag.to_owned(),
            ..AutoTagRule::default()
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

    #[test]
    fn conditions_are_conjunctive_and_support_media_without_a_pattern() {
        let rules = compile_auto_tag_rules(&[AutoTagRule {
            source_app: "editor".into(),
            kind: Some(ClipboardKind::Image),
            tag: "screens".into(),
            ..AutoTagRule::default()
        }]);
        assert_eq!(
            super::match_auto_tags(&rules, "screenshot", "My Editor", ClipboardKind::Image),
            vec!["screens"]
        );
        assert!(
            super::match_auto_tags(&rules, "screenshot", "Browser", ClipboardKind::Image)
                .is_empty()
        );
        assert!(
            super::match_auto_tags(&rules, "screenshot", "Editor", ClipboardKind::Text).is_empty()
        );
        assert!(validate_auto_tag_rules(&[AutoTagRule {
            tag: "everything".into(),
            ..AutoTagRule::default()
        }])
        .is_err());
        assert!(validate_auto_tag_rules(&[rule("[", "broken")]).is_err());
    }
}
