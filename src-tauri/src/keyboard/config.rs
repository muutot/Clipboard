use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::StorageError;

use super::ShortcutBinding;

const CONFIG_DIRECTORY_NAME: &str = "conf";
const KEYBOARD_CONFIG_FILE_NAME: &str = "keyboard.json";
const DEFAULT_TOGGLE_WINDOW_ACTION: &str = "toggleWindow";
const DEFAULT_TOGGLE_WINDOW_SHORTCUT: &str = "Alt+V";
const DEFAULT_COPY_ITEM_SHORTCUT: &str = "Ctrl+C";
const DEFAULT_DELETE_ITEM_SHORTCUT: &str = "Ctrl+D";
const DEFAULT_FAVORITE_ITEM_SHORTCUT: &str = "Ctrl+F";
const DEFAULT_EDIT_ITEM_SHORTCUT: &str = "Ctrl+E";
const DEFAULT_SELECT_ALL_SHORTCUT: &str = "Ctrl+A";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KeyboardConfig {
    pub shortcuts: BTreeMap<String, Vec<String>>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            shortcuts: BTreeMap::from([
                (
                    DEFAULT_TOGGLE_WINDOW_ACTION.to_owned(),
                    vec![DEFAULT_TOGGLE_WINDOW_SHORTCUT.to_owned()],
                ),
                (
                    "copyItem".to_owned(),
                    vec![DEFAULT_COPY_ITEM_SHORTCUT.to_owned()],
                ),
                (
                    "deleteItem".to_owned(),
                    vec![DEFAULT_DELETE_ITEM_SHORTCUT.to_owned()],
                ),
                (
                    "favoriteItem".to_owned(),
                    vec![DEFAULT_FAVORITE_ITEM_SHORTCUT.to_owned()],
                ),
                (
                    "editItem".to_owned(),
                    vec![DEFAULT_EDIT_ITEM_SHORTCUT.to_owned()],
                ),
                (
                    "selectAll".to_owned(),
                    vec![DEFAULT_SELECT_ALL_SHORTCUT.to_owned()],
                ),
            ]),
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
        let mut config = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            KeyboardConfig::default()
        };
        normalize_and_validate(&mut config)?;
        let store = Self { path, config };

        if !store.path.exists() {
            store.save()?;
        }

        Ok(store)
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
        let mut updated = self.config.clone();
        updated.shortcuts.insert(action, normalized.clone());
        normalize_and_validate(&mut updated)?;
        self.config = updated;
        self.save()?;
        Ok(normalized)
    }

    pub fn delete_action(&mut self, action: &str) -> Result<(), StorageError> {
        let mut updated = self.config.clone();
        updated.shortcuts.remove(action);
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
            fs::rename(&temporary_path, &self.path)?;
            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }
}

fn normalize_and_validate(config: &mut KeyboardConfig) -> Result<(), StorageError> {
    let mut owners = HashMap::<String, String>::new();

    for (action, shortcuts) in &mut config.shortcuts {
        validate_action_name(action)?;
        *shortcuts = normalize_shortcuts(shortcuts)?;

        for shortcut in shortcuts {
            if let Some(existing_action) = owners.insert(shortcut.clone(), action.clone()) {
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
        assert_eq!(saved["shortcuts"]["toggleWindow"], json!(["Alt+V"]));
        assert_eq!(saved["shortcuts"]["copyItem"], json!(["Ctrl+C"]));
        assert_eq!(saved["shortcuts"]["deleteItem"], json!(["Ctrl+D"]));
        assert_eq!(saved["shortcuts"]["favoriteItem"], json!(["Ctrl+F"]));
        assert_eq!(saved["shortcuts"]["editItem"], json!(["Ctrl+E"]));
        assert_eq!(saved["shortcuts"]["selectAll"], json!(["Ctrl+A"]));
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
                "shortcuts": { "toggleWindow": ["Alt+V"] },
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
