use std::path::{Path, PathBuf};

use crate::storage::StorageError;

use super::{KeyboardConfig, KeyboardConfigStore, Modifier, ShortcutMatcher};

pub struct KeyboardManager {
    store: KeyboardConfigStore,
    matcher: ShortcutMatcher,
}

impl KeyboardManager {
    pub fn load(project_directory: &Path) -> Result<Self, StorageError> {
        let store = KeyboardConfigStore::load(project_directory)?;
        let matcher = ShortcutMatcher::from_config(store.config())?;
        Ok(Self { store, matcher })
    }

    pub fn path(&self) -> PathBuf {
        self.store.path().to_path_buf()
    }

    pub fn config(&self) -> KeyboardConfig {
        self.store.config().clone()
    }

    pub fn set_action_shortcuts(
        &mut self,
        action: String,
        shortcuts: Vec<String>,
    ) -> Result<Vec<String>, StorageError> {
        let normalized = self.store.set_action_shortcuts(action, shortcuts)?;
        self.matcher = ShortcutMatcher::from_config(self.store.config())?;
        Ok(normalized)
    }

    pub fn match_chord(
        &self,
        modifiers: impl IntoIterator<Item = Modifier>,
        key: &str,
    ) -> &[String] {
        self.matcher.match_chord(modifiers, key)
    }

    pub fn record_modifier_tap(&mut self, modifier: Modifier, timestamp_ms: u64) -> &[String] {
        self.matcher.record_modifier_tap(modifier, timestamp_ms)
    }

    pub fn cancel_modifier_sequence(&mut self) {
        self.matcher.cancel_modifier_sequence();
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::KeyboardManager;
    use crate::keyboard::Modifier;

    #[test]
    fn saved_bindings_replace_the_active_runtime_matcher() {
        let project = temporary_directory();
        let mut manager = KeyboardManager::load(&project).unwrap();

        manager
            .set_action_shortcuts(
                "toggleWindow".to_owned(),
                vec!["Ctrl+Space".to_owned(), "Shift+Shift".to_owned()],
            )
            .unwrap();

        assert!(manager.match_chord([Modifier::Alt], "V").is_empty());
        assert_eq!(
            manager.match_chord([Modifier::Control], "Space"),
            &["toggleWindow"]
        );
        manager.record_modifier_tap(Modifier::Shift, 1_000);
        assert_eq!(
            manager.record_modifier_tap(Modifier::Shift, 1_200),
            &["toggleWindow"]
        );
        fs::remove_dir_all(project).unwrap();
    }

    fn temporary_directory() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "clipboard-keyboard-manager-{}-{unique}",
            std::process::id()
        ))
    }
}
