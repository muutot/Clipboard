use serde::{Deserialize, Serialize};

use crate::domain::ClipboardItem;
use crate::storage::Database;

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
}

pub fn export_items(
    items: &[ClipboardItem],
    options: &ExportOptions,
) -> Result<String, String> {
    match options.format {
        ExportFormat::Json => {
            serde_json::to_string_pretty(items).map_err(|e| e.to_string())
        }
        ExportFormat::Csv => {
            let mut wtr = String::new();
            wtr.push_str("id,kind,title,text_content,source_app,created_at_ms,is_favorite\n");
            for item in items {
                let kind = match item.kind {
                    crate::domain::ClipboardKind::Text => "text",
                    crate::domain::ClipboardKind::Link => "link",
                    crate::domain::ClipboardKind::Image => "image",
                    crate::domain::ClipboardKind::File => "file",
                };
                wtr.push_str(&format!(
                    "{},{},{},{},{},{},{}\n",
                    escape_csv(&item.id),
                    kind,
                    escape_csv(&item.title),
                    escape_csv(&item.text_content.as_deref().unwrap_or("")),
                    escape_csv(&item.source_app.as_deref().unwrap_or("")),
                    item.created_at_ms,
                    item.is_favorite,
                ));
            }
            Ok(wtr)
        }
        ExportFormat::PlainText => {
            let mut out = String::new();
            for item in items {
                if let Some(ref text) = item.text_content {
                    if !out.is_empty() {
                        out.push_str("\n---\n");
                    }
                    out.push_str(text);
                }
            }
            Ok(out)
        }
    }
}

pub fn import_from_json(
    json: &str,
    database: &Database,
) -> Result<ImportSummary, String> {
    let items: Vec<ClipboardItem> =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;

    let mut imported = 0u64;
    let mut skipped = 0u64;
    let mut errors = Vec::new();

    for item in &items {
        match crate::storage::ClipboardRepository::save_item(database, item) {
            Ok(_) => imported += 1,
            Err(e) => {
                skipped += 1;
                errors.push(format!("failed to import {}: {e}", item.id));
            }
        }
    }

    Ok(ImportSummary {
        imported_count: imported,
        skipped_count: skipped,
        errors,
    })
}

fn escape_csv(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ClipboardItem, ClipboardKind};

    fn sample_items() -> Vec<ClipboardItem> {
        vec![
            ClipboardItem {
                id: "item-1".to_owned(),
                kind: ClipboardKind::Text,
                title: "Hello".to_owned(),
                text_content: Some("Hello, world!".to_owned()),
                resource_path: None,
                preview_path: None,
                content_hash: "abc".to_owned(),
                source_app: Some("Notepad".to_owned()),
                size_bytes: 13,
                created_at_ms: 1000,
                last_used_at_ms: None,
                is_favorite: true,
                icon_path: None,
                metadata_json: None,
            },
            ClipboardItem {
                id: "item-2".to_owned(),
                kind: ClipboardKind::Link,
                title: "Example".to_owned(),
                text_content: Some("https://example.com".to_owned()),
                resource_path: None,
                preview_path: None,
                content_hash: "def".to_owned(),
                source_app: Some("Chrome".to_owned()),
                size_bytes: 19,
                created_at_ms: 2000,
                last_used_at_ms: None,
                is_favorite: false,
                icon_path: None,
                metadata_json: None,
            },
        ]
    }

    #[test]
    fn exports_to_json_format() {
        let items = sample_items();
        let options = ExportOptions {
            format: ExportFormat::Json,
            include_favorites: true,
            date_from_ms: None,
            date_to_ms: None,
            content_types: vec!["text".to_owned(), "link".to_owned()],
        };

        let result = export_items(&items, &options).unwrap();
        assert!(result.contains("item-1"));
        assert!(result.contains("item-2"));
        assert!(result.contains("Hello, world!"));
    }

    #[test]
    fn exports_to_csv_format() {
        let items = sample_items();
        let options = ExportOptions {
            format: ExportFormat::Csv,
            include_favorites: true,
            date_from_ms: None,
            date_to_ms: None,
            content_types: vec!["text".to_owned(), "link".to_owned()],
        };

        let result = export_items(&items, &options).unwrap();
        assert!(result.starts_with("id,kind,title,text_content"));
        assert!(result.contains("item-1"));
        assert!(result.contains("item-2"));
    }

    #[test]
    fn exports_to_plain_text_format() {
        let items = sample_items();
        let options = ExportOptions {
            format: ExportFormat::PlainText,
            include_favorites: true,
            date_from_ms: None,
            date_to_ms: None,
            content_types: vec!["text".to_owned(), "link".to_owned()],
        };

        let result = export_items(&items, &options).unwrap();
        assert!(result.contains("Hello, world!"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn imports_from_json_into_database() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        let json = serde_json::to_string(&sample_items()).unwrap();

        let summary = import_from_json(&json, &database).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(summary.skipped_count, 0);
        assert!(summary.errors.is_empty());
    }
}
