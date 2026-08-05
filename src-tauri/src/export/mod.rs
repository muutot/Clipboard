use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::ClipboardItem;
use crate::storage::Database;

mod ppaste;
pub(crate) use ppaste::{import_from_ppaste_backup, BACKUP_EXTENSION};

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
    let filtered = items.iter().filter(|item| {
        let favorite_matches = options.include_favorites || !item.is_favorite;
        let from_matches = options
            .date_from_ms
            .is_none_or(|from| item.created_at_ms >= from);
        let to_matches = options.date_to_ms.is_none_or(|to| item.created_at_ms <= to);
        let kind_matches = options.content_types.is_empty()
            || options
                .content_types
                .iter()
                .any(|kind| kind.eq_ignore_ascii_case(clipboard_kind_name(item.kind)));
        favorite_matches && from_matches && to_matches && kind_matches
    });

    match options.format {
        ExportFormat::Json => serde_json::to_string_pretty(&filtered.cloned().collect::<Vec<_>>())
            .map_err(|e| e.to_string()),
        ExportFormat::Csv => {
            let mut wtr = String::new();
            wtr.push_str("id,kind,title,text_content,source_app,created_at_ms,is_favorite\n");
            for item in filtered {
                wtr.push_str(&format!(
                    "{},{},{},{},{},{},{}\n",
                    escape_csv(&item.id),
                    clipboard_kind_name(item.kind),
                    escape_csv(&item.title),
                    escape_csv(item.text_content.as_deref().unwrap_or("")),
                    escape_csv(item.source_app.as_deref().unwrap_or("")),
                    item.created_at_ms,
                    item.is_favorite,
                ));
            }
            Ok(wtr)
        }
        ExportFormat::PlainText => {
            let mut out = String::new();
            for item in filtered {
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

/// Exports all active records without the normal UI page-size cap.
pub fn export_database(database: &Database, options: &ExportOptions) -> Result<String, String> {
    let mut items = Vec::new();
    let mut offset = 0u32;
    const PAGE_SIZE: u32 = 500;

    loop {
        let page = crate::storage::ClipboardRepository::list_recent(database, PAGE_SIZE, offset)
            .map_err(|error| error.to_string())?;
        let page_len = page.len() as u32;
        items.extend(page);
        if page_len < PAGE_SIZE {
            break;
        }
        offset = offset.saturating_add(PAGE_SIZE);
    }

    export_items(&items, options)
}

pub fn import_from_json(json: &str, database: &Database) -> Result<ImportSummary, String> {
    let items: Vec<ClipboardItem> =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;

    let mut imported = 0u64;
    let mut skipped = 0u64;
    let mut errors = Vec::new();

    for mut item in items {
        item.icon_path = normalize_imported_icon_key(item.icon_path.as_deref());
        match crate::storage::ClipboardRepository::save_item(database, &item) {
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
        pending_truncation: 0,
        max_items: 0,
    })
}

fn normalize_imported_icon_key(icon_path: Option<&str>) -> Option<String> {
    let icon_path = icon_path?.trim();
    if icon_path.is_empty() {
        return None;
    }

    let file_name = icon_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    if file_name.is_empty()
        || !file_name.chars().any(char::is_alphanumeric)
        || !file_name
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        return None;
    }

    Some(file_name.to_owned())
}

/// Imports a plain-text backup. Records are separated by a line containing
/// `---`; blank chunks are ignored and duplicate content is handled by the
/// database's normal content-hash upsert path.
pub fn import_from_plain_text(text: &str, database: &Database) -> Result<ImportSummary, String> {
    let mut imported = 0u64;
    let mut skipped = 0u64;
    let mut errors = Vec::new();

    for (index, chunk) in text.split("\n---\n").enumerate() {
        let content = chunk.trim_matches(['\r', '\n']);
        if content.trim().is_empty() {
            continue;
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        let hash = crate::content::hash::compute_content_hash("text", content, None);
        let item = ClipboardItem {
            id: format!("import-{hash}-{index}"),
            kind: crate::domain::ClipboardKind::Text,
            title: content
                .lines()
                .next()
                .unwrap_or(content)
                .chars()
                .take(120)
                .collect(),
            text_content: Some(content.to_owned()),
            html_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: hash,
            source_app: Some("Plain text import".to_owned()),
            size_bytes: content.len() as u64,
            created_at_ms: now_ms,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        };

        match crate::storage::ClipboardRepository::save_item(database, &item) {
            Ok(_) => imported += 1,
            Err(error) => {
                skipped += 1;
                errors.push(format!("failed to import chunk {index}: {error}"));
            }
        }
    }

    Ok(ImportSummary {
        imported_count: imported,
        skipped_count: skipped,
        errors,
        pending_truncation: 0,
        max_items: 0,
    })
}

pub(crate) fn write_export_file(path: &str, output: &str) -> Result<(), String> {
    let path = Path::new(path);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create export directory: {error}"))?;
    }
    std::fs::write(path, output).map_err(|error| format!("failed to write export: {error}"))
}

fn clipboard_kind_name(kind: crate::domain::ClipboardKind) -> &'static str {
    match kind {
        crate::domain::ClipboardKind::Text => "text",
        crate::domain::ClipboardKind::Link => "link",
        crate::domain::ClipboardKind::Image => "image",
        crate::domain::ClipboardKind::File => "file",
    }
}

fn escape_csv(field: &str) -> String {
    let starts_as_formula = field
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@'));
    if field.contains(',')
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r')
        || starts_as_formula
    {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ClipboardItem, ClipboardKind};
    use crate::storage::ClipboardRepository;

    fn sample_items() -> Vec<ClipboardItem> {
        vec![
            ClipboardItem {
                id: "item-1".to_owned(),
                kind: ClipboardKind::Text,
                title: "Hello".to_owned(),
                text_content: Some("Hello, world!".to_owned()),
                html_content: None,
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
                html_content: None,
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

    #[test]
    fn json_import_stores_only_normalized_icon_file_keys() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        let mut items = sample_items();
        items[0].icon_path =
            Some(r"C:\Users\admin\AppData\Local\Clipboard\icons\Notepad.png".to_owned());
        items[1].icon_path = Some("../../foreign/icons/Chrome.png".to_owned());
        let json = serde_json::to_string(&items).unwrap();

        let summary = import_from_json(&json, &database).unwrap();

        assert_eq!(summary.imported_count, 2);
        assert_eq!(
            database.get_item("item-1").unwrap().unwrap().icon_path,
            Some("Notepad.png".to_owned())
        );
        assert_eq!(
            database.get_item("item-2").unwrap().unwrap().icon_path,
            Some("Chrome.png".to_owned())
        );
    }

    #[test]
    fn imported_icon_key_normalization_preserves_legacy_keys_and_rejects_unsafe_names() {
        assert_eq!(normalize_imported_icon_key(None), None);
        assert_eq!(normalize_imported_icon_key(Some("   ")), None);
        assert_eq!(
            normalize_imported_icon_key(Some("Notepad.png")),
            Some("Notepad.png".to_owned())
        );
        assert_eq!(
            normalize_imported_icon_key(Some("/home/user/.local/share/icons/firefox.png")),
            Some("firefox.png".to_owned())
        );
        assert_eq!(
            normalize_imported_icon_key(Some(r"..\..\icons\微信.png")),
            Some("微信.png".to_owned())
        );
        assert_eq!(normalize_imported_icon_key(Some("../../icons/")), None);
        assert_eq!(normalize_imported_icon_key(Some("..")), None);
        assert_eq!(normalize_imported_icon_key(Some("unsafe?.png")), None);
        assert_eq!(normalize_imported_icon_key(Some("..%2fsecret.png")), None);
    }

    #[test]
    fn export_options_filter_favorites_dates_and_types() {
        let items = sample_items();
        let options = ExportOptions {
            format: ExportFormat::Json,
            include_favorites: false,
            date_from_ms: Some(1500),
            date_to_ms: Some(2500),
            content_types: vec!["link".to_owned()],
        };

        let result = export_items(&items, &options).unwrap();
        assert!(result.contains("item-2"));
        assert!(!result.contains("item-1"));
    }

    #[test]
    fn json_export_import_round_trips_html_content() {
        let mut items = sample_items();
        items[0].html_content = Some("<b>Hello, world!</b>".to_owned());
        let json = serde_json::to_string(&items).unwrap();

        let database = crate::storage::Database::open_in_memory().unwrap();
        let summary = import_from_json(&json, &database).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(
            database.get_item("item-1").unwrap().unwrap().html_content,
            Some("<b>Hello, world!</b>".to_owned())
        );
    }

    #[test]
    fn imports_older_json_without_html_content_field() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        let json = r#"[{"id":"legacy","kind":"text","title":"legacy","textContent":"plain","resourcePath":null,"previewPath":null,"contentHash":"hash","sourceApp":null,"iconPath":null,"sizeBytes":5,"createdAtMs":1,"lastUsedAtMs":null,"isFavorite":false,"metadataJson":null}]"#;

        let summary = import_from_json(json, &database).unwrap();
        assert_eq!(summary.imported_count, 1);
        let item = database.get_item("legacy").unwrap().unwrap();
        assert_eq!(item.html_content, None);
    }

    #[test]
    fn imports_plain_text_chunks() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        let summary = import_from_plain_text("first\n---\nsecond\n", &database).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(database.item_count().unwrap(), 2);
    }

    #[test]
    fn database_export_reads_more_than_one_page() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        for index in 0..510 {
            let item = ClipboardItem {
                id: format!("item-{index}"),
                kind: ClipboardKind::Text,
                title: format!("item-{index}"),
                text_content: Some(format!("text-{index}")),
                html_content: None,
                resource_path: None,
                preview_path: None,
                content_hash: format!("hash-{index}"),
                source_app: None,
                size_bytes: 8,
                created_at_ms: index,
                last_used_at_ms: None,
                is_favorite: false,
                icon_path: None,
                metadata_json: None,
            };
            crate::storage::ClipboardRepository::save_item(&database, &item).unwrap();
        }

        let output = export_database(
            &database,
            &ExportOptions {
                format: ExportFormat::Json,
                include_favorites: true,
                date_from_ms: None,
                date_to_ms: None,
                content_types: vec![],
            },
        )
        .unwrap();
        let exported: Vec<ClipboardItem> = serde_json::from_str(&output).unwrap();
        assert_eq!(exported.len(), 510);
    }

    #[test]
    fn escape_csv_quotes_formula_cells() {
        assert_eq!(escape_csv("plain"), "plain");
        assert_eq!(escape_csv("=SUM(A1)"), "\"=SUM(A1)\"");
        assert_eq!(escape_csv("+123"), "\"+123\"");
        assert_eq!(escape_csv("-danger"), "\"-danger\"");
        assert_eq!(escape_csv("@cmd"), "\"@cmd\"");
        assert_eq!(escape_csv(" =SUM(A1)"), "\" =SUM(A1)\"");
        assert_eq!(escape_csv("a,b"), "\"a,b\"");
        assert_eq!(escape_csv("safe"), "safe");
    }
}
