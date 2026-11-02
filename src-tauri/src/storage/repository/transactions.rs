//! Transactional write funnels for distinct saves, renames, backup restore and history import.
//!
//! Owns `Database::rename_item_transactional`, `Database::save_item_as_new`,
//! `TransactionalSaveSummary`, `Database::validate_restore_items`,
//! `Database::restore_items_transactional(_with_progress)`,
//! `Database::save_items_transactional`, and the import bounds
//! (`MAX_IMPORT_*`, `validate_imported_item`) every importer is validated against.

use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::{
    content_exists_on_connection, current_time_ms, insert_item_row, sync_item_tags_from_metadata,
    StoredClipboardItem, ITEM_COLUMNS,
};
use crate::domain::ClipboardItem;
use crate::storage::{Database, StorageError};

/// Outcome of [`Database::save_items_transactional`].
#[derive(Debug, Default)]
pub struct TransactionalSaveSummary {
    pub imported_count: u64,
    pub skipped_count: u64,
    pub errors: Vec<String>,
}

impl Database {
    /// Rename only an active row using its latest state. The callback may change
    /// title, resource/preview paths, file-list text and metadata, and must not
    /// reenter this Database. Other fields are outside the rename write set.
    pub(crate) fn rename_item_transactional(
        &self,
        id: &str,
        rename: impl FnOnce(&mut ClipboardItem) -> Result<(), StorageError>,
    ) -> Result<Option<ClipboardItem>, StorageError> {
        let _resource_publication = self.begin_resource_write()?;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let stored = transaction
                .query_row(
                    &format!(
                        "SELECT {ITEM_COLUMNS} FROM clipboard_items WHERE id = ?1 AND deleted = 0"
                    ),
                    [id],
                    StoredClipboardItem::from_row,
                )
                .optional()?;
            let Some(stored) = stored else {
                return Ok(None);
            };
            let mut item: ClipboardItem = stored.try_into()?;
            rename(&mut item)?;
            validate_clipboard_text(
                &item.title,
                item.text_content.as_deref(),
                item.html_content.as_deref(),
                item.rtf_content.as_deref(),
            )?;
            transaction.execute(
                "UPDATE clipboard_items SET title = ?2, resource_path = ?3,
                    preview_path = ?4, text_content = ?5, metadata_json = ?6
                 WHERE id = ?1 AND deleted = 0",
                params![
                    id,
                    item.title,
                    item.resource_path,
                    item.preview_path,
                    item.text_content,
                    item.metadata_json
                ],
            )?;
            sync_item_tags_from_metadata(&transaction, id)?;
            transaction.commit()?;
            Ok(Some(item))
        })
    }

    /// Persist a separate item, keeping its proposed hash only when unused.
    /// Collision checks and insertion share the SQLite writer transaction, so
    /// capture on another connection cannot turn this save into a dedup upsert.
    pub(crate) fn save_item_as_new(
        &self,
        mut item: ClipboardItem,
    ) -> Result<ClipboardItem, StorageError> {
        validate_clipboard_text(
            &item.title,
            item.text_content.as_deref(),
            item.html_content.as_deref(),
            item.rtf_content.as_deref(),
        )?;
        let size_bytes =
            i64::try_from(item.size_bytes).map_err(|_| StorageError::ValueOutOfRange {
                field: "size_bytes",
            })?;
        let _resource_publication = self.begin_resource_write()?;
        let proposed_hash = item.content_hash.clone();
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            while content_exists_on_connection(&transaction, item.kind, &item.content_hash)? {
                item.content_hash = format!("{proposed_hash}-{}", uuid::Uuid::new_v4().simple());
            }
            let saved_id = insert_item_row(&transaction, &item, size_bytes)?;
            if saved_id != item.id {
                return Err(StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "save-as-new unexpectedly reused another record",
                )));
            }
            transaction.commit()?;
            Ok(item)
        })
    }

    pub fn validate_restore_items(entries: &[ClipboardItem]) -> Result<(), StorageError> {
        let upper = current_time_ms().saturating_add(MAX_IMPORT_FUTURE_SKEW_MS);
        for item in entries {
            validate_imported_item(item, upper).map_err(|reason| {
                StorageError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, reason))
            })?;
        }
        Ok(())
    }
    /// Restore a validated bundle atomically; duplicates are skipped, any invalid row aborts.
    pub fn restore_items_transactional(
        &self,
        entries: &[ClipboardItem],
    ) -> Result<TransactionalSaveSummary, StorageError> {
        let _resource_publication = self.begin_resource_write()?;
        self.restore_items_transactional_with_progress(entries, |_, _| Ok(()))
    }
    pub fn restore_items_transactional_with_progress(
        &self,
        entries: &[ClipboardItem],
        mut progress: impl FnMut(u64, u64) -> Result<(), StorageError>,
    ) -> Result<TransactionalSaveSummary, StorageError> {
        self.with_connection(|connection| {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut summary = TransactionalSaveSummary::default();
            for (index, item) in entries.iter().enumerate() {
                progress(index as u64, entries.len() as u64)?;
                validate_imported_item(
                    item,
                    current_time_ms().saturating_add(MAX_IMPORT_FUTURE_SKEW_MS),
                )
                .map_err(|reason| {
                    StorageError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, reason))
                })?;
                if content_exists_on_connection(&tx, item.kind, &item.content_hash)? {
                    summary.skipped_count += 1;
                    continue;
                }
                let size =
                    i64::try_from(item.size_bytes).map_err(|_| StorageError::ValueOutOfRange {
                        field: "size_bytes",
                    })?;
                insert_item_row(&tx, item, size)?;
                summary.imported_count += 1;
            }
            progress(entries.len() as u64, entries.len() as u64)?;
            tx.commit()?;
            Ok(summary)
        })
    }

    /// Saves many items inside a single transaction so a bulk import commits
    /// once instead of producing one fsync per row. Rows whose
    /// `(kind, content_hash)` already exists are counted as skipped (a
    /// duplicate import neither duplicates nor rewrites records); per-row
    /// failures are collected instead of aborting the whole batch.
    /// The single funnel for every importer (JSON, CSV, and the replication
    /// mirror), so bounds belong here rather than in three parsers.
    ///
    /// An imported row is attacker-controlled data with respect to the rest of
    /// the system: `created_at_ms` becomes the replication version, and an
    /// unbounded `text_content` bypasses the capture limit that protects the
    /// search index. Each entry is validated independently and a rejected one is
    /// reported in `errors` instead of failing the whole batch.
    pub fn save_items_transactional(
        &self,
        entries: &[(String, ClipboardItem)],
    ) -> Result<TransactionalSaveSummary, StorageError> {
        let upper_bound = current_time_ms().saturating_add(MAX_IMPORT_FUTURE_SKEW_MS);
        self.with_connection(|connection| {
            let mut transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut summary = TransactionalSaveSummary::default();

            for (label, item) in entries {
                if let Err(reason) = validate_imported_item(item, upper_bound) {
                    summary.skipped_count += 1;
                    summary.errors.push(format!("skipped {label}: {reason}"));
                    continue;
                }
                let size_bytes =
                    i64::try_from(item.size_bytes).map_err(|_| StorageError::ValueOutOfRange {
                        field: "size_bytes",
                    })?;
                let already_exists =
                    content_exists_on_connection(&transaction, item.kind, &item.content_hash)?;
                if already_exists {
                    summary.skipped_count += 1;
                    continue;
                }
                // A failed derived-tag write must not leave an imported row
                // behind when this batch intentionally continues after errors.
                let mut row = transaction.savepoint()?;
                match insert_item_row(&row, item, size_bytes) {
                    Ok(_) => {
                        row.commit()?;
                        summary.imported_count += 1;
                    }
                    Err(error) => {
                        row.rollback()?;
                        summary.skipped_count += 1;
                        summary
                            .errors
                            .push(format!("failed to import {label}: {error}"));
                    }
                }
            }

            transaction.commit()?;
            Ok(summary)
        })
    }
}

/// How far past the current wall clock an imported `created_at_ms` may sit.
///
/// Generous enough for a device whose clock runs a little fast, tight enough
/// that a saturated version clock is still far away.
const MAX_IMPORT_FUTURE_SKEW_MS: i64 = 24 * 60 * 60 * 1000;

/// Shared ceiling for one persisted/imported/archive text payload.
///
/// `ConfigStore::max_text_capture_bytes` clamps to 10 MB, so an import is never
/// stricter than what capture itself would have accepted, and the capture limit
/// can never be widened past what the import funnel admits.
const MAX_IMPORT_TEXT_BYTES: usize = 10_000_000;

/// Hard ceiling for an imported title. Titles are rendered in list rows, card
/// headers, and the search index, and never approach this size legitimately.
const MAX_IMPORT_TITLE_BYTES: usize = 4_096;

/// Keep manual edits and ordinary saves within the portable archive bounds.
/// Limits count UTF-8 bytes, independently of the reported `size_bytes`.
pub(crate) fn validate_clipboard_text(
    title: &str,
    text: Option<&str>,
    html: Option<&str>,
    rtf: Option<&str>,
) -> Result<(), StorageError> {
    for (field, value, limit) in [
        ("title", title, MAX_IMPORT_TITLE_BYTES),
        (
            "text_content",
            text.unwrap_or_default(),
            MAX_IMPORT_TEXT_BYTES,
        ),
        (
            "html_content",
            html.unwrap_or_default(),
            MAX_IMPORT_TEXT_BYTES,
        ),
        (
            "rtf_content",
            rtf.unwrap_or_default(),
            MAX_IMPORT_TEXT_BYTES,
        ),
    ] {
        if value.len() > limit {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{field} is {} bytes, over the {limit} byte limit",
                    value.len()
                ),
            )));
        }
    }
    Ok(())
}

/// Rejects an imported row that would poison the version clock or the search
/// index. The bounds are intentionally independent of the replication version:
/// `created_at_ms` is validated because it becomes that version.
fn validate_imported_item(item: &ClipboardItem, upper_bound_ms: i64) -> Result<(), String> {
    if item.created_at_ms < 0 {
        return Err(format!("created_at_ms {} is negative", item.created_at_ms));
    }
    if item.created_at_ms > upper_bound_ms {
        return Err(format!(
            "created_at_ms {} is too far in the future",
            item.created_at_ms
        ));
    }
    validate_clipboard_text(
        &item.title,
        item.text_content.as_deref(),
        item.html_content.as_deref(),
        item.rtf_content.as_deref(),
    )
    .map_err(|error| error.to_string())
}
