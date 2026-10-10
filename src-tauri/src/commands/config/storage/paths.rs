//! Rewriting stored paths after the storage root moves (database rows and
//! JSON config values).

use std::path::{Path, PathBuf};

use rusqlite::{params, TransactionBehavior};

use crate::storage::{Database, StorageError, StoragePaths};

pub fn storage_path_mappings(old: &StoragePaths, new: &StoragePaths) -> Vec<(PathBuf, PathBuf)> {
    let mut mappings = vec![
        (old.previews.clone(), new.previews.clone()),
        (old.images.clone(), new.images.clone()),
        (old.files.clone(), new.files.clone()),
        (old.storage.join("icons"), new.storage.join("icons")),
        (old.storage.clone(), new.storage.clone()),
    ];
    mappings.retain(|(from, to)| from != to);
    mappings.sort_by_key(|(from, _)| std::cmp::Reverse(from.components().count()));
    mappings
}

pub fn rewrite_database_storage_paths(
    database: &Database,
    mappings: &[(PathBuf, PathBuf)],
) -> Result<u64, StorageError> {
    database.with_connection(|connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let records = {
            let mut statement = transaction.prepare(
                "SELECT id, kind, text_content, resource_path, preview_path, icon_path, metadata_json
                 FROM clipboard_items",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut updated = 0u64;
        for (id, kind, text_content, resource_path, preview_path, icon_path, metadata_json) in
            records
        {
            let rewritten_resource = rewrite_optional_storage_path(resource_path.as_deref(), mappings);
            let rewritten_preview = rewrite_optional_storage_path(preview_path.as_deref(), mappings);
            let rewritten_icon = rewrite_optional_storage_path(icon_path.as_deref(), mappings);
            let rewritten_text = if kind == "file" {
                rewrite_json_storage_paths(text_content.as_deref(), mappings)
            } else {
                text_content.clone()
            };
            let rewritten_metadata = rewrite_json_storage_paths(metadata_json.as_deref(), mappings);

            if rewritten_resource == resource_path
                && rewritten_preview == preview_path
                && rewritten_icon == icon_path
                && rewritten_text == text_content
                && rewritten_metadata == metadata_json
            {
                continue;
            }

            transaction.execute(
                "UPDATE clipboard_items
                 SET text_content = ?2,
                     resource_path = ?3,
                     preview_path = ?4,
                     icon_path = ?5,
                     metadata_json = ?6
                 WHERE id = ?1",
                params![
                    id,
                    rewritten_text,
                    rewritten_resource,
                    rewritten_preview,
                    rewritten_icon,
                    rewritten_metadata,
                ],
            )?;
            updated = updated.saturating_add(1);
        }

        transaction.commit()?;
        Ok(updated)
    })
}

pub fn rewrite_optional_storage_path(
    value: Option<&str>,
    mappings: &[(PathBuf, PathBuf)],
) -> Option<String> {
    value.map(|value| rewrite_storage_path(value, mappings))
}

pub fn rewrite_storage_path(value: &str, mappings: &[(PathBuf, PathBuf)]) -> String {
    let path = Path::new(value);
    for (from, to) in mappings {
        if let Ok(relative) = path.strip_prefix(from) {
            return to.join(relative).to_string_lossy().into_owned();
        }
    }
    value.to_owned()
}

pub fn rewrite_json_storage_paths(
    value: Option<&str>,
    mappings: &[(PathBuf, PathBuf)],
) -> Option<String> {
    let value = value?;
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(value) else {
        return Some(value.to_owned());
    };
    let changed = rewrite_json_value_paths(&mut json, mappings);
    if changed {
        serde_json::to_string(&json)
            .ok()
            .or_else(|| Some(value.to_owned()))
    } else {
        Some(value.to_owned())
    }
}

pub fn rewrite_json_value_paths(
    value: &mut serde_json::Value,
    mappings: &[(PathBuf, PathBuf)],
) -> bool {
    match value {
        serde_json::Value::String(path) => {
            let rewritten = rewrite_storage_path(path, mappings);
            if rewritten == *path {
                false
            } else {
                *path = rewritten;
                true
            }
        }
        serde_json::Value::Array(values) => {
            let mut changed = false;
            for value in values {
                changed |= rewrite_json_value_paths(value, mappings);
            }
            changed
        }
        serde_json::Value::Object(values) => {
            let mut changed = false;
            for value in values.values_mut() {
                changed |= rewrite_json_value_paths(value, mappings);
            }
            changed
        }
        _ => false,
    }
}
