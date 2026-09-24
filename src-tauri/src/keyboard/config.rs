use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::StorageError;

use super::actions::is_global_action;
use super::ShortcutBinding;

const CONFIG_DIRECTORY_NAME: &str = "conf";
const KEYBOARD_CONFIG_FILE_NAME: &str = "keyboard.json";
/// Canonical default bindings shared with the frontend. The single source
/// of truth is `keyboard-defaults.json` at the repository root (read by the
/// frontend through `src/lib/keyboard-defaults.ts`). A malformed bundle
/// fails loudly here instead of registering wrong global hotkeys.
const KEYBOARD_DEFAULTS_JSON: &str = include_str!("../../../keyboard-defaults.json");

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KeyboardConfig {
    pub shortcuts: BTreeMap<String, Vec<String>>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        // Parse into a plain helper instead of `KeyboardConfig` itself:
        // the config type carries a container-level `#[serde(default)]`,
        // so deserializing it from inside `Default::default()` would
        // recurse into this very function and overflow the stack.
        #[derive(Deserialize)]
        struct KeyboardDefaultsFile {
            shortcuts: BTreeMap<String, Vec<String>>,
        }
        let bundled: KeyboardDefaultsFile =
            serde_json::from_str(KEYBOARD_DEFAULTS_JSON).expect("bundled keyboard defaults parse");
        Self {
            shortcuts: bundled.shortcuts,
            extra: BTreeMap::new(),
        }
    }
}

impl KeyboardConfig {
    pub fn from_shortcuts(shortcuts: BTreeMap<String, Vec<String>>) -> Self {
        Self {
            shortcuts,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct KeyboardConfigStore {
    path: PathBuf,
    config: KeyboardConfig,
}

impl KeyboardConfigStore {
    pub fn load(project_directory: &Path) -> Result<Self, StorageError> {
        let config_directory = project_directory.join(CONFIG_DIRECTORY_NAME);
        fs::create_dir_all(&config_directory)?;
        let path = config_directory.join(KEYBOARD_CONFIG_FILE_NAME);
        let (config, merged_defaults) = if path.exists() {
            match Self::load_and_validate(&path) {
                Ok(parsed) => parsed,
                Err(error) => {
                    // Mirror `ConfigStore::load`: a corrupt or schema-invalid
                    // file must never make the app unlaunchable. Quarantine it
                    // and continue from the bundled defaults instead of
                    // propagating the error into the Tauri setup hook.
                    Self::quarantine_corrupt(&config_directory, &path, &error);
                    (KeyboardConfig::default(), false)
                }
            }
        } else {
            (KeyboardConfig::default(), false)
        };
        let store = Self { path, config };

        if !store.path.exists() || merged_defaults {
            store.save()?;
        }

        Ok(store)
    }

    fn load_and_validate(path: &Path) -> Result<(KeyboardConfig, bool), StorageError> {
        let loaded: KeyboardConfig = serde_json::from_slice(&fs::read(path)?)?;
        let (mut config, merged_defaults) = merge_missing_default_actions(loaded);
        normalize_and_validate(&mut config)?;
        Ok((config, merged_defaults))
    }

    fn quarantine_corrupt(config_directory: &Path, path: &Path, error: &StorageError) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let quarantined =
            config_directory.join(format!("{KEYBOARD_CONFIG_FILE_NAME}.corrupt-{stamp}"));
        crate::log_event!(
            "[keyboard] {} is unreadable ({error}); quarantining it as {} and starting with defaults",
            path.display(),
            quarantined.display()
        );
        if fs::rename(path, &quarantined).is_err() {
            // Mirror `ConfigStore::load`: a locked or undeletable rename must
            // not leave the corrupt file in place — `KeyboardConfigStore::load`
            // only rewrites when the path is gone, so a failed rename alone
            // would re-read the same bytes on every launch forever.
            let _ = fs::remove_file(path);
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn config(&self) -> &KeyboardConfig {
        &self.config
    }

    pub fn set_action_shortcuts(
        &mut self,
        action: String,
        shortcuts: Vec<String>,
    ) -> Result<Vec<String>, StorageError> {
        validate_action_name(&action)?;
        let normalized = normalize_shortcuts(&shortcuts)?;
        // Only OS-global actions need a registrable global-hotkey chord. A
        // window action may legitimately use a key with no global mapping
        // (e.g. the shipped `focusSearch` default `/`), which is dispatched by
        // the webview instead of `RegisterHotKey`.
        if is_global_action(&action) {
            for shortcut in &normalized {
                let binding = ShortcutBinding::from_str(shortcut)
                    .map_err(|error| StorageError::InvalidShortcut(error.to_string()))?;
                if crate::platform::hotkey_common::hotkey_registration_identity(&binding).is_none()
                {
                    return Err(StorageError::InvalidShortcut(format!(
                        "'{shortcut}' uses a key that cannot be registered as a global shortcut"
                    )));
                }
            }
        }
        let mut updated = self.config.clone();
        updated.shortcuts.insert(action, normalized.clone());
        normalize_and_validate(&mut updated)?;
        self.config = updated;
        self.save()?;
        Ok(normalized)
    }

    pub fn delete_action(&mut self, action: &str) -> Result<(), StorageError> {
        // Store an explicit empty binding instead of removing the key. The
        // load-time default merge treats an absent key as "this config predates
        // the action" and re-adds the bundled default, so removing the key here
        // would silently undo the deletion on the next launch.
        let mut updated = self.config.clone();
        updated.shortcuts.insert(action.to_owned(), Vec::new());
        normalize_and_validate(&mut updated)?;
        self.config = updated;
        self.save()?;
        Ok(())
    }

    pub fn reset_to_defaults(&mut self) -> Result<(), StorageError> {
        self.config = KeyboardConfig::default();
        self.save()?;
        Ok(())
    }

    fn save(&self) -> Result<(), StorageError> {
        use std::io::Write;

        let directory = self.path.parent().ok_or_else(|| {
            StorageError::Io(std::io::Error::other(
                "keyboard config has no parent directory",
            ))
        })?;
        let temporary_path = directory.join(".keyboard.json.tmp");
        let contents = serde_json::to_vec_pretty(&self.config)?;

        let result = (|| -> Result<(), StorageError> {
            let mut file = fs::File::create(&temporary_path)?;
            file.write_all(&contents)?;
            file.sync_all()?;
            drop(file);
            crate::storage::replace_file(&temporary_path, &self.path)?;
            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }
}

/// Backfills bundled defaults for actions that are absent from a loaded
/// config file. The settings UI only ever rebinds an action to a new chord
/// list (possibly empty), never deletes its key, so a missing key means the
/// file predates the action — without this merge, upgraders never receive
/// new defaults like `toggleFloatPanel`. Defaults whose chords would
/// collide with a binding the user already owns are skipped, so the merge
/// can never introduce a `normalize_and_validate` conflict or fail load.
fn merge_missing_default_actions(mut config: KeyboardConfig) -> (KeyboardConfig, bool) {
    let taken: HashSet<String> = config
        .shortcuts
        .values()
        .flatten()
        .filter_map(|shortcut| ShortcutBinding::from_str(shortcut).ok())
        .filter_map(|binding| {
            crate::platform::hotkey_common::hotkey_registration_identity(&binding)
        })
        .collect();

    let mut merged = false;
    for (action, chords) in KeyboardConfig::default().shortcuts {
        if chords.is_empty() || config.shortcuts.contains_key(&action) {
            continue;
        }
        if chords.iter().any(|chord| {
            ShortcutBinding::from_str(chord)
                .ok()
                .and_then(|binding| {
                    crate::platform::hotkey_common::hotkey_registration_identity(&binding)
                })
                .is_some_and(|identity| taken.contains(&identity))
        }) {
            continue;
        }
        config.shortcuts.insert(action, chords);
        merged = true;
    }
    (config, merged)
}

fn normalize_and_validate(config: &mut KeyboardConfig) -> Result<(), StorageError> {
    let mut owners = HashMap::<String, String>::new();

    for (action, shortcuts) in &mut config.shortcuts {
        validate_action_name(action)?;
        *shortcuts = normalize_shortcuts(shortcuts)?;

        for shortcut in shortcuts {
            // Compare the resolved OS chord, not the spelling: `Ctrl+Esc` and
            // `Ctrl+Escape` are different canonicals that register the same
            // virtual key, so a string comparison would miss the collision.
            let Some(identity) = ShortcutBinding::from_str(shortcut)
                .ok()
                .and_then(|binding| {
                    crate::platform::hotkey_common::hotkey_registration_identity(&binding)
                })
            else {
                // A key without a global-hotkey mapping is kept (older configs
                // may contain one) but cannot collide with anything.
                continue;
            };
            if let Some(existing_action) = owners.insert(identity, action.clone()) {
                return Err(StorageError::ShortcutConflict {
                    shortcut: shortcut.clone(),
                    first_action: existing_action,
                    second_action: action.clone(),
                });
            }
        }
    }

    Ok(())
}

fn normalize_shortcuts(shortcuts: &[String]) -> Result<Vec<String>, StorageError> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(shortcuts.len());

    for shortcut in shortcuts {
        let canonical = ShortcutBinding::from_str(shortcut)
            .map_err(|error| StorageError::InvalidShortcut(error.to_string()))?
            .canonical();
        if seen.insert(canonical.clone()) {
            normalized.push(canonical);
        }
    }

    Ok(normalized)
}

fn validate_action_name(action: &str) -> Result<(), StorageError> {
    let mut characters = action.chars();
    let valid_first = characters
        .next()
        .is_some_and(|character| character.is_ascii_lowercase());
    let valid_rest = characters.all(|character| character.is_ascii_alphanumeric());

    if valid_first && valid_rest {
        Ok(())
    } else {
        Err(StorageError::InvalidKeyboardAction(action.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use serde_json::{json, Value};

    use super::KeyboardConfigStore;
    use crate::storage::StorageError;

    #[test]
    fn creates_a_separate_keyboard_configuration_file() {
        let project = temporary_directory("default");

        let store = KeyboardConfigStore::load(&project).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();

        assert_eq!(store.path(), project.join("conf/keyboard.json"));
        assert_eq!(saved["shortcuts"]["toggleWindow"], json!(["Alt+C"]));
        assert_eq!(saved["shortcuts"]["toggleFloatPanel"], json!(["Alt+V"]));
        assert_eq!(saved["shortcuts"]["copyItem"], json!(["Ctrl+C", "Enter"]));
        assert_eq!(saved["shortcuts"]["deleteItem"], json!(["Ctrl+D"]));
        assert_eq!(saved["shortcuts"]["favoriteItem"], json!(["Ctrl+F"]));
        assert_eq!(saved["shortcuts"]["addTag"], json!(["Ctrl+T"]));
        assert_eq!(saved["shortcuts"]["openDetail"], json!(["Space", "Ctrl+E"]));
        assert_eq!(saved["shortcuts"]["selectAll"], json!(["Ctrl+A"]));
        assert_eq!(saved["shortcuts"]["quickPaste"], json!([]));
        assert_eq!(saved["shortcuts"]["quickCopy1"], json!(["Ctrl+1"]));
        assert_eq!(saved["shortcuts"]["quickCopy9"], json!(["Ctrl+9"]));
        assert_eq!(saved["shortcuts"]["hideWindow"], json!(["Escape"]));
        assert_eq!(saved["shortcuts"]["focusSearch"], json!(["/", "Ctrl+K"]));
        assert_eq!(saved["shortcuts"]["clearSelection"], json!(["Backspace"]));
        assert_eq!(saved["shortcuts"]["downloadItem"], json!(["Ctrl+S"]));
        assert_eq!(saved["shortcuts"]["moveSelectionUp"], json!(["Arrowup"]));
        assert_eq!(
            saved["shortcuts"]["moveSelectionDown"],
            json!(["Arrowdown"])
        );
        assert_eq!(saved["shortcuts"]["switchFilterNext"], json!(["Tab"]));
        assert_eq!(saved["shortcuts"]["switchFilterPrev"], json!(["Shift+Tab"]));
        assert_eq!(saved["shortcuts"]["switchFilter1"], json!(["Alt+1"]));
        assert_eq!(saved["shortcuts"]["switchFilter7"], json!(["Alt+7"]));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn stores_multiple_bindings_for_one_action() {
        let project = temporary_directory("multiple");
        let mut store = KeyboardConfigStore::load(&project).unwrap();

        let saved = store
            .set_action_shortcuts(
                "toggleWindow".to_owned(),
                vec![
                    "shift + ctrl + v".to_owned(),
                    "Shift+Shift".to_owned(),
                    "Ctrl+Shift+V".to_owned(),
                ],
            )
            .unwrap();

        assert_eq!(saved, vec!["Ctrl+Shift+V", "Shift+Shift"]);
        let reopened = KeyboardConfigStore::load(&project).unwrap();
        assert_eq!(reopened.config().shortcuts["toggleWindow"], saved);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn allows_window_actions_without_a_global_hotkey_mapping() {
        let project = temporary_directory("window-action");
        let mut store = KeyboardConfigStore::load(&project).unwrap();

        // `focusSearch` is a window action; `/` has no global-hotkey mapping
        // and must still be saveable.
        let saved = store
            .set_action_shortcuts(
                "focusSearch".to_owned(),
                vec!["/".to_owned(), "Ctrl+K".to_owned()],
            )
            .unwrap();
        assert_eq!(saved, vec!["/", "Ctrl+K"]);

        // A global action still rejects an unregistrable key.
        let error = store
            .set_action_shortcuts("toggleWindow".to_owned(), vec!["/".to_owned()])
            .unwrap_err();
        assert!(matches!(error, StorageError::InvalidShortcut(_)));

        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn rejects_a_binding_owned_by_another_action() {
        let project = temporary_directory("conflict");
        let mut store = KeyboardConfigStore::load(&project).unwrap();

        let error = store
            .set_action_shortcuts("quickPaste".to_owned(), vec!["Alt+V".to_owned()])
            .unwrap_err();

        assert!(matches!(
            error,
            StorageError::ShortcutConflict { shortcut, .. } if shortcut == "Alt+V"
        ));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn preserves_unknown_keyboard_configuration_fields() {
        let project = temporary_directory("preserve");
        let config_directory = project.join("conf");
        fs::create_dir_all(&config_directory).unwrap();
        fs::write(
            config_directory.join("keyboard.json"),
            serde_json::to_vec_pretty(&json!({
                "shortcuts": { "toggleWindow": ["Alt+C"] },
                "doubleTapIntervalMs": 280
            }))
            .unwrap(),
        )
        .unwrap();
        let mut store = KeyboardConfigStore::load(&project).unwrap();

        store
            .set_action_shortcuts("toggleWindow".to_owned(), vec!["Ctrl+Space".to_owned()])
            .unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();

        assert_eq!(saved["doubleTapIntervalMs"], 280);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn merges_missing_default_actions_into_an_existing_config() {
        let project = temporary_directory("merge-missing");
        let config_directory = project.join("conf");
        fs::create_dir_all(&config_directory).unwrap();
        // A v1.5.2-era file: no toggleFloatPanel key at all.
        fs::write(
            config_directory.join("keyboard.json"),
            serde_json::to_vec_pretty(&json!({
                "shortcuts": { "toggleWindow": ["Ctrl+Space"], "copyItem": [] }
            }))
            .unwrap(),
        )
        .unwrap();

        let store = KeyboardConfigStore::load(&project).unwrap();

        assert_eq!(store.config().shortcuts["toggleWindow"], vec!["Ctrl+Space"]);
        assert_eq!(store.config().shortcuts["toggleFloatPanel"], vec!["Alt+V"]);
        // An explicit empty binding is a user choice, not a missing key.
        assert_eq!(store.config().shortcuts["copyItem"], Vec::<String>::new());
        // The merge is persisted so later saves keep the new defaults.
        let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert_eq!(saved["shortcuts"]["toggleFloatPanel"], json!(["Alt+V"]));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn skips_a_default_that_would_conflict_with_existing_bindings() {
        let project = temporary_directory("merge-conflict");
        let config_directory = project.join("conf");
        fs::create_dir_all(&config_directory).unwrap();
        // The user kept the v1.5.2 default Alt+V for toggleWindow, which is
        // also the new toggleFloatPanel default: the merge must skip the
        // float default instead of failing load with a conflict.
        fs::write(
            config_directory.join("keyboard.json"),
            serde_json::to_vec_pretty(&json!({
                "shortcuts": { "toggleWindow": ["Alt+V"] }
            }))
            .unwrap(),
        )
        .unwrap();

        let store = KeyboardConfigStore::load(&project).unwrap();

        assert_eq!(store.config().shortcuts["toggleWindow"], vec!["Alt+V"]);
        assert!(!store.config().shortcuts.contains_key("toggleFloatPanel"));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn quarantines_an_unparsable_keyboard_config_instead_of_failing() {
        let project = temporary_directory("corrupt-unparsable");
        let config_directory = project.join("conf");
        fs::create_dir_all(&config_directory).unwrap();
        fs::write(config_directory.join("keyboard.json"), b"{ not valid json").unwrap();

        let store = KeyboardConfigStore::load(&project)
            .expect("an unparsable keyboard config must not abort startup");

        assert_eq!(store.config().shortcuts["toggleWindow"], vec!["Alt+C"]);
        assert_eq!(quarantine_count(&config_directory), 1);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn quarantines_a_wrong_shape_keyboard_config_instead_of_failing() {
        let project = temporary_directory("corrupt-shape");
        let config_directory = project.join("conf");
        fs::create_dir_all(&config_directory).unwrap();
        fs::write(
            config_directory.join("keyboard.json"),
            serde_json::to_vec_pretty(&json!({
                "shortcuts": { "toggleWindow": "Alt+C" }
            }))
            .unwrap(),
        )
        .unwrap();

        let store = KeyboardConfigStore::load(&project)
            .expect("a wrong-shape keyboard config must not abort startup");

        assert_eq!(store.config().shortcuts["toggleWindow"], vec!["Alt+C"]);
        assert_eq!(quarantine_count(&config_directory), 1);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn deleting_an_action_is_not_undone_by_the_default_merge() {
        let project = temporary_directory("delete-action");
        let mut store = KeyboardConfigStore::load(&project).unwrap();
        assert_eq!(store.config().shortcuts["toggleWindow"], vec!["Alt+C"]);

        store.delete_action("toggleWindow").unwrap();
        let reopened = KeyboardConfigStore::load(&project).unwrap();

        assert_eq!(
            reopened.config().shortcuts["toggleWindow"],
            Vec::<String>::new()
        );
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn rejects_alias_collisions_that_map_to_the_same_os_chord() {
        let project = temporary_directory("alias-collision");
        let mut store = KeyboardConfigStore::load(&project).unwrap();
        store
            .set_action_shortcuts("toggleWindow".to_owned(), vec!["Ctrl+Esc".to_owned()])
            .unwrap();

        let error = store
            .set_action_shortcuts("quickPaste".to_owned(), vec!["Ctrl+Escape".to_owned()])
            .unwrap_err();

        assert!(
            matches!(error, StorageError::ShortcutConflict { .. }),
            "{error:?}"
        );
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn rejects_a_key_without_a_global_hotkey_mapping() {
        let project = temporary_directory("unsupported-key");
        let mut store = KeyboardConfigStore::load(&project).unwrap();

        let error = store
            .set_action_shortcuts("toggleWindow".to_owned(), vec!["Ctrl+,".to_owned()])
            .unwrap_err();

        assert!(
            matches!(error, StorageError::InvalidShortcut(_)),
            "{error:?}"
        );
        fs::remove_dir_all(project).unwrap();
    }

    fn quarantine_count(config_directory: &std::path::Path) -> usize {
        fs::read_dir(config_directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("keyboard.json.corrupt-")
            })
            .count()
    }

    fn temporary_directory(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "clipboard-keyboard-config-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
