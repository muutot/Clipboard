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

    pub fn delete_action(&mut self, action: String) -> Result<(), StorageError> {
        self.store.delete_action(&action)?;
        self.matcher = ShortcutMatcher::from_config(self.store.config())?;
        Ok(())
    }

    pub fn reset_to_defaults(&mut self) -> Result<(), StorageError> {
        self.store.reset_to_defaults()?;
        self.matcher = ShortcutMatcher::from_config(self.store.config())?;
        Ok(())
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
    use crate::storage::StorageError;

    #[test]
    fn stale_keyboard_instance_cannot_restore_a_disabled_global_shortcut() {
        let project = temporary_directory();
        let mut first = KeyboardManager::load(&project).unwrap();
        let mut stale = KeyboardManager::load(&project).unwrap();
        let before = stale.config();
        first.delete_action("toggleWindow".into()).unwrap();
        let committed = fs::read(first.path()).unwrap();

        let result = stale.set_action_shortcuts("focusSearch".into(), vec!["Ctrl+F12".into()]);
        assert!(stale.delete_action("focusSearch".into()).is_err());
        assert!(stale.reset_to_defaults().is_err());
        let retained = fs::read(first.path()).unwrap();
        let stale_config = stale.config();
        let stale_match = stale.match_chord([Modifier::Alt], "C").to_vec();
        let mut reopened = KeyboardManager::load(&project).unwrap();
        let disabled = reopened.config().shortcuts["toggleWindow"].is_empty();
        reopened
            .set_action_shortcuts("focusSearch".into(), vec!["Ctrl+F12".into()])
            .unwrap();
        fs::remove_dir_all(project).unwrap();

        assert!(
            result.is_err(),
            "stale save must reject rather than restore a disabled shortcut"
        );
        assert_eq!(retained, committed);
        assert_eq!(stale_config.shortcuts, before.shortcuts);
        assert_eq!(stale_match, vec!["toggleWindow"]);
        assert!(disabled);
    }

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

        assert_eq!(
            manager.match_chord([Modifier::Alt], "V"),
            &["toggleFloatPanel"]
        );
        assert!(manager.match_chord([Modifier::Alt], "C").is_empty());
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

    fn verify_failed_mutation(
        mutate: impl FnOnce(&mut KeyboardManager) -> Result<(), StorageError>,
    ) {
        let project = temporary_directory();
        let mut manager = KeyboardManager::load(&project).unwrap();
        manager
            .set_action_shortcuts("toggleWindow".into(), vec!["Ctrl+Space".into()])
            .unwrap();
        let before = manager.config();
        let path = manager.path();
        let original_file = project.join("keyboard-before-failure.json");
        fs::rename(&path, &original_file).unwrap();
        fs::create_dir(&path).unwrap();

        let result = mutate(&mut manager);
        let after_failure = manager.config();
        let matched_after_failure = manager.match_chord([Modifier::Control], "Space").to_vec();

        fs::remove_dir(&path).unwrap();
        fs::rename(&original_file, &path).unwrap();
        let disk_after_failure = KeyboardManager::load(&project).unwrap().config();
        // A later, unrelated window binding must not accidentally commit the
        // global edit that the failed operation reported as rejected.
        manager
            .set_action_shortcuts("focusSearch".into(), vec!["Ctrl+F12".into()])
            .unwrap();
        let reopened = KeyboardManager::load(&project).unwrap();
        let after_later_save = reopened.config();
        let matched_after_later_save = reopened.match_chord([Modifier::Control], "Space").to_vec();
        drop(reopened);
        drop(manager);
        fs::remove_dir_all(project).unwrap();

        assert!(result.is_err());
        assert_eq!(after_failure.shortcuts, before.shortcuts);
        assert_eq!(disk_after_failure.shortcuts, before.shortcuts);
        assert_eq!(matched_after_failure, vec!["toggleWindow"]);
        assert_eq!(
            after_later_save.shortcuts["toggleWindow"],
            before.shortcuts["toggleWindow"]
        );
        assert_eq!(matched_after_later_save, vec!["toggleWindow"]);
        assert_eq!(after_later_save.shortcuts["focusSearch"], vec!["Ctrl+F12"]);
    }

    #[test]
    fn failed_binding_save_keeps_config_matcher_and_later_saves_consistent() {
        verify_failed_mutation(|manager| {
            manager
                .set_action_shortcuts("toggleWindow".into(), vec!["Ctrl+F11".into()])
                .map(|_| ())
        });
    }

    #[test]
    fn failed_binding_delete_keeps_config_matcher_and_later_saves_consistent() {
        verify_failed_mutation(|manager| manager.delete_action("toggleWindow".into()));
    }

    #[test]
    fn failed_binding_reset_keeps_config_matcher_and_later_saves_consistent() {
        verify_failed_mutation(KeyboardManager::reset_to_defaults);
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
