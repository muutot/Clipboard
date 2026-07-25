use crate::config::ConfigStore;

pub struct PrivacyManager {
    pub paused: bool,
    pub sensitive_patterns: Vec<String>,
    pub password_manager_apps: Vec<String>,
}

impl Default for PrivacyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivacyManager {
    pub fn new() -> Self {
        Self {
            paused: false,
            sensitive_patterns: vec![
                r"\b\d{3}-\d{2}-\d{4}\b".to_owned(),
                r"\b\d{4}[ -]?\d{4}[ -]?\d{4}[ -]?\d{4}\b".to_owned(),
                r"\bpassword\s*[=:]\s*\S+".to_owned(),
                r"\bsecret\s*[=:]\s*\S+".to_owned(),
                r"\bapi[_-]?key\s*[=:]\s*\S+".to_owned(),
                r"\btoken\s*[=:]\s*\S+".to_owned(),
                r"-----BEGIN\s+(RSA|EC|DSA|OPENSSH)\s+PRIVATE\s+KEY-----".to_owned(),
            ],
            password_manager_apps: vec![
                "1Password".to_owned(),
                "Bitwarden".to_owned(),
                "LastPass".to_owned(),
                "KeePass".to_owned(),
                "Dashlane".to_owned(),
                "NordPass".to_owned(),
                "iCloud Keychain".to_owned(),
                "Windows Credential Manager".to_owned(),
            ],
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn is_sensitive_content(&self, text: &str) -> bool {
        for pattern in &self.sensitive_patterns {
            if let Ok(re) = regex_lite::Regex::new(pattern) {
                if re.is_match(text) {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_password_manager(&self, app_name: &str) -> bool {
        let normalized = app_name.trim().to_lowercase();
        self.password_manager_apps
            .iter()
            .any(|name| name.to_lowercase() == normalized)
    }

    pub fn sync_with_config(&mut self, config: &ConfigStore) {
        self.paused = config.privacy_paused();
    }
}

#[cfg(test)]
mod tests {
    use super::PrivacyManager;

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
    fn detects_password_managers_case_insensitively() {
        let manager = PrivacyManager::new();
        assert!(manager.is_password_manager("1Password"));
        assert!(manager.is_password_manager("1password"));
        assert!(manager.is_password_manager("bitwarden"));
        assert!(manager.is_password_manager("KeePass"));
        assert!(!manager.is_password_manager("Notepad"));
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
