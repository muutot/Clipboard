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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoTagPhase {
    Scanning,
    Applying,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTagProgress {
    pub phase: AutoTagPhase,
    pub completed: u64,
    pub total: u64,
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

    /// Match on a separate WAL snapshot, then atomically apply the bounded on-disk plan.
    pub fn auto_tag_history(
        &self,
        rules: &[CompiledAutoTagRule],
        apply: bool,
    ) -> Result<AutoTagHistoryPreview, StorageError> {
        self.auto_tag_history_with_progress(rules, apply, |_| Ok(()))
    }

    /// Returning an error from progress cancels the operation, including the final commit.
    /// Matching uses content hashes as the persisted body identity; manual tags are reread.
    pub fn auto_tag_history_with_progress(
        &self,
        rules: &[CompiledAutoTagRule],
        apply: bool,
        mut progress: impl FnMut(AutoTagProgress) -> Result<(), StorageError>,
    ) -> Result<AutoTagHistoryPreview, StorageError> {
        self.with_task_connection(|connection| {
            connection.execute_batch("CREATE TEMP TABLE auto_tag_plan (
                id TEXT PRIMARY KEY, kind TEXT, title TEXT, hash TEXT, source TEXT, tags TEXT
            )")?;
            let result = (|| {
            let mut result = AutoTagHistoryPreview::default();
            {
                let tx = connection.transaction()?;
                let total = tx.query_row("SELECT COUNT(*) FROM clipboard_items WHERE deleted = 0", [], |row| row.get::<_, i64>(0))? as u64;
                progress(AutoTagProgress { phase: AutoTagPhase::Scanning, completed: 0, total })?;
                let mut statement = tx.prepare("SELECT id, title, kind,
                    CASE WHEN kind IN ('text', 'link') THEN COALESCE(text_content, title) ELSE title END,
                    COALESCE(source_app, ''), metadata_json, content_hash FROM clipboard_items WHERE deleted = 0 ORDER BY id")?;
                let mut rows = statement.query([])?;
                let mut completed = 0;
                while let Some(row) = rows.next()? {
                    completed += 1;
                    progress(AutoTagProgress { phase: AutoTagPhase::Scanning, completed, total })?;
                    let id: String = row.get(0)?;
                    let title: String = row.get(1)?;
                    let stored_kind: String = row.get(2)?;
                    let kind = kind_from_storage(&stored_kind)?;
                    let text: String = row.get(3)?;
                    let source: String = row.get(4)?;
                    let tags = match_auto_tags(rules, &text, &source, kind);
                    if tags.is_empty() { continue; }
                    result.matched_count += 1;
                    let metadata: Option<String> = row.get(5)?;
                    if metadata_with_added_tags(metadata.as_deref(), &tags).is_none() { continue; }
                    if apply {
                        let hash: String = row.get(6)?;
                        tx.execute("INSERT INTO auto_tag_plan VALUES (?1,?2,?3,?4,?5,?6)",
                            params![id, stored_kind, title, hash, source, serde_json::to_string(&tags)?])?;
                    }
                    result.changed_count += 1;
                    if result.samples.len() < 20 {
                        result.samples.push(AutoTagHistorySample { id, title: title.chars().take(200).collect(), tags });
                    }
                }
                drop(rows);
                drop(statement);
                progress(AutoTagProgress { phase: AutoTagPhase::Scanning, completed, total })?;
                tx.commit()?;
            }
            if apply {
                let total = result.changed_count;
                result.changed_count = 0;
                progress(AutoTagProgress { phase: AutoTagPhase::Applying, completed: 0, total })?;
                let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                {
                    let mut statement = tx.prepare("SELECT id, kind, title, hash, source, tags FROM auto_tag_plan ORDER BY id")?;
                    let mut rows = statement.query([])?;
                    let mut completed = 0;
                    while let Some(row) = rows.next()? {
                        let id: String = row.get(0)?;
                        let unchanged: bool = tx.query_row(
                            "SELECT EXISTS(SELECT 1 FROM clipboard_items WHERE id = ?1 AND deleted = 0
                             AND kind = ?2 AND title = ?3 AND content_hash = ?4 AND COALESCE(source_app, '') = ?5)",
                            params![id, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?],
                            |row| row.get(0))?;
                        if !unchanged {
                            return Err(StorageError::Io(std::io::Error::other("history changed while matching; preview and try again")));
                        }
                        let tags: Vec<String> = serde_json::from_str(&row.get::<_, String>(5)?)?;
                        if add_tags_on_connection(&tx, &id, &tags)? { result.changed_count += 1; }
                        completed += 1;
                        progress(AutoTagProgress { phase: AutoTagPhase::Applying, completed, total })?;
                    }
                }
                // Check at EOF too: cancellation on the last row must roll back every update.
                progress(AutoTagProgress { phase: AutoTagPhase::Applying, completed: total, total })?;
                tx.commit()?;
            }
            Ok(result)
            })();
            let cleanup = connection.execute_batch("DROP TABLE auto_tag_plan");
            match result { Ok(value) => { cleanup?; Ok(value) }, Err(error) => Err(error) }
        })
    }
}
