use serde::{Deserialize, Serialize};

use crate::domain::ClipboardItem;
use crate::storage::Database;

pub(crate) mod backup;
mod ppaste;
pub(crate) use ppaste::{import_from_ppaste_backup, BACKUP_EXTENSION};

mod csv;
mod import;
mod stream;

#[cfg(test)]
mod tests;

pub(crate) use csv::{clipboard_kind_name, escape_csv, CsvReader, CSV_TEXT_ENCODING};
pub use import::*;
#[cfg(test)]
pub(crate) use import::{normalize_imported_icon_key, sanitize_imported_path};
pub use stream::{export_database_to_file, export_database_to_writer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Json,
    Csv,
    PlainText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub include_favorites: bool,
    pub date_from_ms: Option<i64>,
    pub date_to_ms: Option<i64>,
    pub content_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub imported_count: u64,
    pub skipped_count: u64,
    pub errors: Vec<String>,
    #[serde(default)]
    pub pending_truncation: u64,
    #[serde(default)]
    pub max_items: u32,
}

pub fn export_items(items: &[ClipboardItem], options: &ExportOptions) -> Result<String, String> {
    stream::export_slice(items, options)
}

/// IPC compatibility wrapper; file exports use the bounded writer directly.
pub fn export_database(database: &Database, options: &ExportOptions) -> Result<String, String> {
    let mut output = Vec::new();
    export_database_to_writer(database, options, &mut output).map_err(|error| error.to_string())?;
    String::from_utf8(output).map_err(|error| error.to_string())
}
