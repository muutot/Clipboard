//! Bulk transactional write funnels for backup restore and history import.
//!
//! Owns `TransactionalSaveSummary`, `Database::validate_restore_items`,
//! `Database::restore_items_transactional(_with_progress)`,
//! `Database::save_items_transactional`, and the import bounds
//! (`MAX_IMPORT_*`, `validate_imported_item`) every importer is validated against.

use rusqlite::TransactionBehavior;

use super::{content_exists_on_connection, current_time_ms, insert_item_row};
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
        let _resource_publication = self.begin_resource_write();
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

/// Hard ceiling for one imported text payload.
///
/// `ConfigStore::max_text_capture_bytes` clamps to 10 MB, so an import is never
/// stricter than what capture itself would have accepted, and the capture limit
/// can never be widened past what the import funnel admits.
const MAX_IMPORT_TEXT_BYTES: usize = 10_000_000;

/// Hard ceiling for an imported title. Titles are rendered in list rows, card
/// headers, and the search index, and never approach this size legitimately.
const MAX_IMPORT_TITLE_BYTES: usize = 4_096;

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
    for (field, value) in [
        ("title", item.title.as_str()),
        (
            "text_content",
            item.text_content.as_deref().unwrap_or_default(),
        ),
        (
            "html_content",
            item.html_content.as_deref().unwrap_or_default(),
        ),
        (
            "rtf_content",
            item.rtf_content.as_deref().unwrap_or_default(),
        ),
    ] {
        if value.len() > MAX_IMPORT_TEXT_BYTES {
            return Err(format!(
                "{field} is {} bytes, over the {MAX_IMPORT_TEXT_BYTES} byte import limit",
                value.len()
            ));
        }
    }
    if item.title.len() > MAX_IMPORT_TITLE_BYTES {
        return Err(format!(
            "title is {} bytes, over the {MAX_IMPORT_TITLE_BYTES} byte import limit",
            item.title.len()
        ));
    }
    Ok(())
}
