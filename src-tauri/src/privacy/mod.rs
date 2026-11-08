use crate::config::ConfigStore;

/// Canonical list of password-manager / credential-vault application names.
///
/// Single source of truth consumed by `PrivacyConfig::default` as the seed
/// for the user-editable ignore list (`ignored_applications`), so the docs,
/// the settings UI, and capture behavior cannot drift apart. Sorted
/// alphabetically; matching is case-insensitive and name-normalized by
/// `state.rs`.
pub const PASSWORD_MANAGER_APPS: &[&str] = &[
    "1Password",
    "Bitwarden",
    "Dashlane",
    "iCloud Keychain",
    "KeePass",
    "KeePassXC",
    "LastPass",
    "NordPass",
    "Windows Credential Manager",
];

pub fn default_password_manager_apps() -> Vec<String> {
    PASSWORD_MANAGER_APPS
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

/// Default sensitive-content regex patterns. Matching content is skipped by
/// the capture pipeline unless the user overrides the list in settings.
pub const DEFAULT_SENSITIVE_PATTERNS: &[&str] = &[
    r"\b\d{3}-\d{2}-\d{4}\b",
    r"\b\d{4}[ -]?\d{4}[ -]?\d{4}[ -]?\d{4}\b",
    r#"(?i)\bpassword["']?\s*[=:]\s*(?:["'][^"'\s][^"']*["']|[^"'\s,}\]]+)"#,
    r#"(?i)\bsecret["']?\s*[=:]\s*(?:["'][^"'\s][^"']*["']|[^"'\s,}\]]+)"#,
    r#"(?i)\bapi[_-]?key["']?\s*[=:]\s*(?:["'][^"'\s][^"']*["']|[^"'\s,}\]]+)"#,
    r#"(?i)\btoken["']?\s*[=:]\s*(?:["'][^"'\s][^"']*["']|[^"'\s,}\]]+)"#,
    r"-----BEGIN\s+(?:(?:RSA|EC|DSA|OPENSSH|ENCRYPTED)\s+)?PRIVATE\s+KEY-----",
];

// Only this exact untouched historical list is eligible for replacement.
// Partial, reordered, empty and custom lists remain user-owned.
pub(crate) const LEGACY_SENSITIVE_PATTERNS: &[&str] = &[
    r"\b\d{3}-\d{2}-\d{4}\b",
    r"\b\d{4}[ -]?\d{4}[ -]?\d{4}[ -]?\d{4}\b",
    r"\bpassword\s*[=:]\s*\S+",
    r"\bsecret\s*[=:]\s*\S+",
    r"\bapi[_-]?key\s*[=:]\s*\S+",
    r"\btoken\s*[=:]\s*\S+",
    r"-----BEGIN\s+(RSA|EC|DSA|OPENSSH)\s+PRIVATE\s+KEY-----",
];

pub struct PrivacyManager {
    pub paused: bool,
    pub sensitive_patterns: Vec<regex_lite::Regex>,
}

impl Default for PrivacyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivacyManager {
    pub fn new() -> Self {
        let sensitive_patterns = DEFAULT_SENSITIVE_PATTERNS
            .iter()
            .filter_map(|p| regex_lite::Regex::new(p).ok())
            .collect();
        Self {
            paused: false,
            sensitive_patterns,
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn is_sensitive_content(&self, text: &str) -> bool {
        self.sensitive_patterns.iter().any(|re| re.is_match(text))
    }

    pub fn sync_with_config(&mut self, config: &ConfigStore) {
        self.paused = config.privacy_paused();
        self.sensitive_patterns = compile_sensitive_patterns(config.sensitive_patterns());
    }
}

/// Compiles the configured sensitive-content patterns, skipping (and loudly
/// reporting) entries that fail to parse so a silently ignored rule cannot
/// leave the user believing a pattern is protecting them.
fn compile_sensitive_patterns(patterns: &[String]) -> Vec<regex_lite::Regex> {
    let mut compiled = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        match regex_lite::Regex::new(pattern) {
            Ok(regex) => compiled.push(regex),
            Err(error) => {
                crate::log_warn!(
                    "[privacy] ignoring invalid sensitive pattern {pattern:?}: {error}"
                );
            }
        }
    }
    compiled
}

#[cfg(test)]
mod tests {
    use super::{compile_sensitive_patterns, PrivacyManager};

    #[test]
    fn skips_invalid_patterns_without_dropping_valid_ones() {
        let patterns = vec![
            r"\btoken\s*[=:]\s*\S+".to_owned(),
            "(unclosed[".to_owned(),
            r"\bpassword\s*[=:]\s*\S+".to_owned(),
        ];
        let compiled = compile_sensitive_patterns(&patterns);
        assert_eq!(compiled.len(), 2);
        assert!(compiled.iter().any(|re| re.is_match("token: abc")));
        assert!(compiled.iter().any(|re| re.is_match("password=x")));
    }

    #[test]
    fn detects_credit_card_patterns() {
        let manager = PrivacyManager::new();
        assert!(manager.is_sensitive_content("card: 1234-5678-9012-3456"));
        assert!(manager.is_sensitive_content("4111 1111 1111 1111"));
        assert!(!manager.is_sensitive_content("hello world"));
    }

    #[test]
    fn detects_credential_patterns() {
        let manager = PrivacyManager::new();
        assert!(manager.is_sensitive_content("password=supersecret123"));
        assert!(manager.is_sensitive_content("api_key = abcdefgh"));
        assert!(manager.is_sensitive_content("token: eyJhbGciOiJIUzI1NiJ9"));
    }

    #[test]
    fn detects_private_key_patterns() {
        let manager = PrivacyManager::new();
        assert!(manager.is_sensitive_content("-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQE..."));
    }

    #[test]
    fn default_patterns_cover_structured_credentials_and_key_types() {
        let manager = PrivacyManager::new();
        for text in [
            "PASSWORD=synthetic",
            r#"{"password":"synthetic"}"#,
            r#"{"SECRET": "synthetic"}"#,
            r#"{"api_key": "synthetic"}"#,
            "'API-KEY': 'synthetic'",
            r#"{"token":"synthetic"}"#,
            "-----BEGIN PRIVATE KEY-----",
            "-----BEGIN ENCRYPTED PRIVATE KEY-----",
            "-----BEGIN EC PRIVATE KEY-----",
        ] {
            assert!(manager.is_sensitive_content(text), "missed {text:?}");
        }
    }

    #[test]
    fn default_patterns_keep_empty_values_and_ordinary_text() {
        let manager = PrivacyManager::new();
        for text in [
            r#"{"password":""}"#,
            r#"{"token": " "}"#,
            "password=",
            "tokenizer=ordinary",
            "The password field is required.",
            r#"{"name":"example","enabled":true}"#,
            "-----BEGIN PUBLIC KEY-----",
        ] {
            assert!(
                !manager.is_sensitive_content(text),
                "false positive {text:?}"
            );
        }
    }

    #[test]
    fn toggles_pause_state() {
        let mut manager = PrivacyManager::new();
        assert!(!manager.is_paused());
        manager.toggle_pause();
        assert!(manager.is_paused());
        manager.toggle_pause();
        assert!(!manager.is_paused());
    }
}
