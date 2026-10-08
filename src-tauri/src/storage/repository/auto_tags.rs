use super::helpers::{kind_from_storage, sync_item_tags_from_metadata};
use crate::storage::{Database, StorageError};
use crate::tags::{match_auto_tags, CompiledAutoTagRule};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTagHistoryPreview {
    pub matched_count: u64,
    pub changed_count: u64,
    pub samples: Vec<AutoTagHistorySample>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTagHistorySample {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
}

fn metadata_with_added_tags(metadata: Option<&str>, tags: &[String]) -> Option<serde_json::Value> {
    let mut value = metadata
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let mut existing: Vec<String> = value["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tag| tag.as_str().map(str::to_owned))
        .collect();
    let mut changed = false;
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !existing.iter().any(|old| old == tag) {
            existing.push(tag.to_owned());
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    value["tags"] = serde_json::json!(existing);
    Some(value)
}

fn add_tags_on_connection(
    connection: &Connection,
    id: &str,
    tags: &[String],
) -> Result<bool, StorageError> {
    let row: Option<Option<String>> = connection
        .query_row(
            "SELECT metadata_json FROM clipboard_items WHERE id = ?1 AND deleted = 0",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(metadata) = row else {
        return Ok(false);
    };
    let Some(value) = metadata_with_added_tags(metadata.as_deref(), tags) else {
        return Ok(false);
    };
    connection.execute(
        "UPDATE clipboard_items SET metadata_json = ?2 WHERE id = ?1",
        params![id, value.to_string()],
    )?;
    sync_item_tags_from_metadata(connection, id)?;
    Ok(true)
}

impl Database {
    /// Additive and atomic: recapture/automation must never erase manually assigned tags.
    pub fn add_tags(&self, id: &str, tags: &[String]) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let changed = add_tags_on_connection(&tx, id, tags)?;
            tx.commit()?;
            Ok(changed)
        })
    }

    /// One transaction gives preview a consistent snapshot and makes application all-or-nothing.
    /// Iterate bodies one row at a time; only twenty short samples escape this function.
    pub fn auto_tag_history(
        &self,
        rules: &[CompiledAutoTagRule],
        apply: bool,
    ) -> Result<AutoTagHistoryPreview, StorageError> {
        self.with_connection(|connection| {
            let tx = connection.transaction_with_behavior(if apply { TransactionBehavior::Immediate } else { TransactionBehavior::Deferred })?;
            let mut result = AutoTagHistoryPreview::default();
            {
                let mut statement = tx.prepare("SELECT id, title, kind,
                    CASE WHEN kind IN ('text', 'link') THEN COALESCE(text_content, title) ELSE title END,
                    COALESCE(source_app, ''), metadata_json FROM clipboard_items WHERE deleted = 0 ORDER BY id")?;
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    let id: String = row.get(0)?;
                    let title: String = row.get(1)?;
                    let kind = kind_from_storage(&row.get::<_, String>(2)?)?;
                    let text: String = row.get(3)?;
                    let source: String = row.get(4)?;
                    let tags = match_auto_tags(rules, &text, &source, kind);
                    if tags.is_empty() { continue; }
                    result.matched_count += 1;
                    let metadata: Option<String> = row.get(5)?;
                    if metadata_with_added_tags(metadata.as_deref(), &tags).is_none() { continue; }
                    if apply && !add_tags_on_connection(&tx, &id, &tags)? { continue; }
                    result.changed_count += 1;
                    if result.samples.len() < 20 {
                        result.samples.push(AutoTagHistorySample { id, title: title.chars().take(200).collect(), tags });
                    }
                }
            }
            tx.commit()?;
            Ok(result)
        })
    }
}
