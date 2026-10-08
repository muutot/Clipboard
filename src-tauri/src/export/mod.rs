use serde::{Deserialize, Serialize};

use crate::domain::ClipboardItem;
use crate::storage::Database;

mod ppaste;
pub(crate) use ppaste::{import_from_ppaste_backup, BACKUP_EXTENSION};

const CSV_TEXT_ENCODING: &str = "apostrophe-v1";

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

mod stream;
pub use stream::{export_database_to_file, export_database_to_writer};

pub fn export_items(items: &[ClipboardItem], options: &ExportOptions) -> Result<String, String> {
    stream::export_slice(items, options)
}

/// IPC compatibility wrapper; file exports use the bounded writer directly.
pub fn export_database(database: &Database, options: &ExportOptions) -> Result<String, String> {
    let mut output = Vec::new();
    export_database_to_writer(database, options, &mut output).map_err(|error| error.to_string())?;
    String::from_utf8(output).map_err(|error| error.to_string())
}
pub fn import_from_json(json: &str, database: &Database) -> Result<ImportSummary, String> {
    let items: Vec<ClipboardItem> =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;

    // Route through the transactional bulk path so re-importing a record with
    // the same `(kind, content_hash)` counts as skipped instead of rewriting
    // the stored row (title/timestamps/resurrection) via the single-item
    // upsert. This matches the `.Pastebackup` import semantics.
    let mut entries = Vec::with_capacity(items.len());
    for mut item in items {
        item.icon_path = normalize_imported_icon_key(item.icon_path.as_deref());
        item.resource_path = sanitize_imported_path(item.resource_path.as_deref());
        item.preview_path = sanitize_imported_path(item.preview_path.as_deref());
        let label = item.id.clone();
        entries.push((label, item));
    }
    let summary = database
        .save_items_transactional(&entries)
        .map_err(|error| error.to_string())?;

    Ok(ImportSummary {
        imported_count: summary.imported_count,
        skipped_count: summary.skipped_count,
        errors: summary.errors,
        pending_truncation: 0,
        max_items: 0,
    })
}

/// Drops resource/preview paths from a foreign backup that cannot name a real
/// file on this machine. A crafted backup could otherwise point `resource_path`
/// at an arbitrary absolute path and have downstream workers (OCR, thumbnails)
/// read it.
fn sanitize_imported_path(path: Option<&str>) -> Option<String> {
    let trimmed = path?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = std::path::Path::new(trimmed);
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    if candidate.is_file() {
        Some(trimmed.to_owned())
    } else {
        None
    }
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
    let mut entries = Vec::new();
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    // Recognize delimiter lines in both LF and CRLF files without normalizing
    // line endings inside the clipboard content itself.
    for line in text.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            chunks.push(&text[start..offset]);
            start = offset + line.len();
        }
        offset += line.len();
    }
    chunks.push(&text[start..]);

    for (index, chunk) in chunks.into_iter().enumerate() {
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
            rtf_content: None,
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

        entries.push((format!("chunk {index}"), item));
    }

    // Same duplicate-skip contract as the JSON path: re-imported content must
    // not rewrite the stored rows.
    let summary = database
        .save_items_transactional(&entries)
        .map_err(|error| error.to_string())?;

    Ok(ImportSummary {
        imported_count: summary.imported_count,
        skipped_count: summary.skipped_count,
        errors: summary.errors,
        pending_truncation: 0,
        max_items: 0,
    })
}

/// Imports the CSV produced by `export_items` (and any CSV with the same
/// columns). The format does not carry binary resources, so image/file rows
/// are reconstructed as placeholder items; text and link rows round-trip
/// exactly.
pub fn import_from_csv(csv: &str, database: &Database) -> Result<ImportSummary, String> {
    let mut reader = CsvReader::new(csv);
    let header = match reader.next_record() {
        Some(record) => record,
        None => {
            return Ok(ImportSummary {
                imported_count: 0,
                skipped_count: 0,
                errors: vec![],
                pending_truncation: 0,
                max_items: 0,
            });
        }
    };

    let column_index = |name: &str| -> Option<usize> {
        header
            .iter()
            .position(|column| column.trim().eq_ignore_ascii_case(name))
    };

    let id_idx = column_index("id");
    let kind_idx = column_index("kind");
    let title_idx = column_index("title");
    let text_idx = column_index("text_content");
    let source_idx = column_index("source_app");
    let created_idx = column_index("created_at_ms");
    let favorite_idx = column_index("is_favorite");
    let content_hash_idx = column_index("content_hash");
    let text_encoding_idx = column_index("clipboard_text_encoding");

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64;

    let mut entries = Vec::new();
    let mut row_index = 0u64;

    while let Some(record) = reader.next_record() {
        if record.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        row_index += 1;

        let encoded_text = text_encoding_idx
            .and_then(|index| record.get(index))
            .is_some_and(|encoding| encoding == CSV_TEXT_ENCODING);

        let field = |index: Option<usize>| -> Option<&str> {
            index.and_then(|index| record.get(index)).map(|value| {
                if encoded_text {
                    value.strip_prefix('\'').unwrap_or(value)
                } else {
                    value.as_str()
                }
            })
        };

        let kind = match field(kind_idx)
            .unwrap_or("text")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "link" => crate::domain::ClipboardKind::Link,
            "image" => crate::domain::ClipboardKind::Image,
            "file" => crate::domain::ClipboardKind::File,
            _ => crate::domain::ClipboardKind::Text,
        };

        let text_content = field(text_idx).map(|value| value.to_owned());
        let size_bytes = text_content
            .as_ref()
            .map(|value| value.len() as u64)
            .unwrap_or(0);
        let title = field(title_idx)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_owned())
            .or_else(|| {
                text_content
                    .as_ref()
                    .map(|value| value.chars().take(120).collect())
            })
            .unwrap_or_else(|| "Imported item".to_owned());

        let id = field(id_idx)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_owned())
            .unwrap_or_else(|| {
                let hash = crate::content::hash::compute_content_hash(
                    clipboard_kind_name(kind),
                    text_content.as_deref().unwrap_or(""),
                    None,
                );
                format!("import-csv-{hash}-{row_index}")
            });

        let source_app = field(source_idx)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_owned());

        let created_at_ms = field(created_idx)
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(now_ms);

        let is_favorite = field(favorite_idx)
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "true" | "1" | "yes" | "y"
                )
            })
            .unwrap_or(false);

        // Prefer the exported identity. Media rows carry no text, so deriving
        // the hash from `text_content` alone gives every image/file row the
        // same hash and the `UNIQUE(kind, content_hash)` upsert silently
        // collapses them into one row. Older CSVs without the column keep the
        // derived fallback for backward compatibility.
        let content_hash = field(content_hash_idx)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_owned())
            .unwrap_or_else(|| {
                crate::content::hash::compute_content_hash(
                    clipboard_kind_name(kind),
                    text_content.as_deref().unwrap_or(""),
                    None,
                )
            });

        let item = crate::domain::ClipboardItem {
            id,
            kind,
            title,
            text_content,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash,
            source_app,
            size_bytes,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite,
            icon_path: None,
            metadata_json: None,
        };

        entries.push((format!("row {row_index}"), item));
    }

    // Same duplicate-skip contract as the JSON path: re-imported content must
    // not rewrite the stored rows.
    let summary = database
        .save_items_transactional(&entries)
        .map_err(|error| error.to_string())?;

    Ok(ImportSummary {
        imported_count: summary.imported_count,
        skipped_count: summary.skipped_count,
        errors: summary.errors,
        pending_truncation: 0,
        max_items: 0,
    })
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
    // CSV quotes delimit a cell; they do not make its contents literal in a
    // spreadsheet. Prefix formula-looking text, and escape an original leading
    // apostrophe too so our versioned importer can reverse this exactly.
    let literal = if starts_as_formula || field.starts_with('\'') {
        std::borrow::Cow::Owned(format!("'{field}"))
    } else {
        std::borrow::Cow::Borrowed(field)
    };
    let field = literal.as_ref();
    if field.contains(',')
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r')
        || field.starts_with('\'')
    {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// Minimal RFC 4180-style CSV reader matching the quoting rules used by
/// `escape_csv`. Operates on `char`s so multi-byte UTF-8 content (Chinese
/// titles, emoji, etc.) is preserved without splitting code points.
struct CsvReader {
    chars: Vec<char>,
    pos: usize,
}

impl CsvReader {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    /// Returns the next record as a list of (unquoted) field values, or `None`
    /// at end of input.
    fn next_record(&mut self) -> Option<Vec<String>> {
        if self.pos >= self.chars.len() {
            return None;
        }

        let mut fields = Vec::new();
        loop {
            fields.push(self.read_field());
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('\n') => {
                    self.pos += 1;
                    break;
                }
                Some('\r') => {
                    self.pos += 1;
                    if self.peek() == Some('\n') {
                        self.pos += 1;
                    }
                    break;
                }
                _ => break,
            }
        }
        Some(fields)
    }

    fn read_field(&mut self) -> String {
        if self.peek() == Some('"') {
            self.pos += 1;
            let mut value = String::new();
            while let Some(character) = self.peek() {
                if character == '"' {
                    if self.peek_next() == Some('"') {
                        value.push('"');
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                        break;
                    }
                } else {
                    value.push(character);
                    self.pos += 1;
                }
            }
            value
        } else {
            let mut value = String::new();
            while let Some(character) = self.peek() {
                if matches!(character, ',' | '\r' | '\n') {
                    break;
                }
                value.push(character);
                self.pos += 1;
            }
            value
        }
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
                rtf_content: None,
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
                rtf_content: None,
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
    fn reimporting_same_json_skips_without_rewriting_stored_rows() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        let json = serde_json::to_string(&sample_items()).unwrap();

        let first = import_from_json(&json, &database).unwrap();
        assert_eq!(first.imported_count, 2);
        assert_eq!(first.skipped_count, 0);

        // A crafted re-import carries the same identities but hostile content:
        // it must be skipped, never overwrite the stored rows.
        let hostile = r#"[{"id":"item-1","kind":"text","title":"HOSTILE","textContent":"Hello, world!","htmlContent":null,"rtfContent":null,"resourcePath":"C:\\evil.txt","previewPath":null,"contentHash":"abc","sourceApp":"Evil","iconPath":null,"sizeBytes":13,"createdAtMs":999999,"lastUsedAtMs":null,"isFavorite":true,"metadataJson":null}]"#;
        let second = import_from_json(hostile, &database).unwrap();
        assert_eq!(second.imported_count, 0);
        assert_eq!(second.skipped_count, 1);

        let stored = database.get_item("item-1").unwrap().unwrap();
        assert_eq!(stored.title, "Hello");
        assert_eq!(stored.created_at_ms, 1000);
        assert_eq!(stored.resource_path, None);
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
    fn imported_resource_path_is_sanitized() {
        assert_eq!(sanitize_imported_path(None), None);
        assert_eq!(sanitize_imported_path(Some("   ")), None);
        assert_eq!(sanitize_imported_path(Some("../escape.png")), None);
        assert_eq!(
            sanitize_imported_path(Some("C:/definitely/not/existing/x.png")),
            None
        );
        let temp =
            std::env::temp_dir().join(format!("clipboard-import-path-{}", std::process::id()));
        std::fs::write(&temp, b"x").unwrap();
        assert_eq!(
            sanitize_imported_path(Some(temp.to_str().unwrap())),
            Some(temp.to_string_lossy().into_owned())
        );
        let _ = std::fs::remove_file(&temp);
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
        items[0].rtf_content = Some("{\\rtf1\\b Hello, world!}".to_owned());
        let json = serde_json::to_string(&items).unwrap();

        let database = crate::storage::Database::open_in_memory().unwrap();
        let summary = import_from_json(&json, &database).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(
            database.get_item("item-1").unwrap().unwrap().html_content,
            Some("<b>Hello, world!</b>".to_owned())
        );
        assert_eq!(
            database.get_item("item-1").unwrap().unwrap().rtf_content,
            Some("{\\rtf1\\b Hello, world!}".to_owned())
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
        assert_eq!(item.rtf_content, None);
    }

    #[test]
    fn imports_plain_text_chunks() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        let summary = import_from_plain_text("first\n---\nsecond\n", &database).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(database.item_count().unwrap(), 2);
    }

    #[test]
    fn imports_crlf_plain_text_separators_without_changing_record_line_endings() {
        let database = Database::open_in_memory().unwrap();
        let summary =
            import_from_plain_text("first\r\ncontinued\r\n---\r\nsecond\r\n", &database).unwrap();
        assert_eq!(summary.imported_count, 2);
        let items = database
            .list_recent(10, 0, &crate::storage::HistoryFilter::default())
            .unwrap();
        assert!(items
            .iter()
            .any(|item| item.text_content.as_deref() == Some("first\r\ncontinued")));
        assert!(items
            .iter()
            .any(|item| item.text_content.as_deref() == Some("second")));
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
                rtf_content: None,
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
    fn database_export_keeps_one_snapshot_during_concurrent_usage_updates() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };

        let directory =
            std::env::temp_dir().join(format!("clipboard-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("history.sqlite3");
        let database = Database::open(&path).unwrap();
        let entries: Vec<_> = (0..510)
            .map(|index| {
                let mut item = sample_items().remove(0);
                item.id = format!("export-{index}");
                item.content_hash = item.id.clone();
                item.created_at_ms = index;
                item.last_used_at_ms = Some(index);
                (item.id.clone(), item)
            })
            .collect();
        assert_eq!(
            database
                .save_items_transactional(&entries)
                .unwrap()
                .imported_count,
            510
        );

        // Commit on a separate real WAL connection while the export's SELECT
        // is already reading. The oldest record jumps across the page boundary.
        let writer = Database::open(&path).unwrap();
        let wrote = Arc::new(AtomicBool::new(false));
        let did_write = wrote.clone();
        database
            .with_connection(|connection| {
                connection.progress_handler(
                    1000,
                    Some(move || {
                        if !did_write.swap(true, Ordering::SeqCst) {
                            assert!(writer.set_last_used("export-0").unwrap());
                        }
                        false
                    }),
                )?;
                Ok(())
            })
            .unwrap();

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
        assert!(
            wrote.load(Ordering::SeqCst),
            "concurrent update must actually run"
        );
        assert_eq!(exported.len(), 510);
        let unique: std::collections::HashSet<_> = exported.iter().map(|item| &item.id).collect();
        assert_eq!(
            unique.len(),
            510,
            "export must neither duplicate nor omit records"
        );
        assert_eq!(
            exported
                .iter()
                .find(|item| item.id == "export-0")
                .unwrap()
                .last_used_at_ms,
            Some(0)
        );
        database
            .with_connection(|connection| {
                connection.progress_handler(0, None::<fn() -> bool>)?;
                Ok(())
            })
            .unwrap();
        drop(database);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn escape_csv_encodes_formula_cells_as_literal_text() {
        assert_eq!(escape_csv("plain"), "plain");
        assert_eq!(escape_csv("=SUM(A1)"), "\"'=SUM(A1)\"");
        assert_eq!(escape_csv("+123"), "\"'+123\"");
        assert_eq!(escape_csv("-danger"), "\"'-danger\"");
        assert_eq!(escape_csv("@cmd"), "\"'@cmd\"");
        assert_eq!(escape_csv(" =SUM(A1)"), "\"' =SUM(A1)\"");
        assert_eq!(escape_csv("a,b"), "\"a,b\"");
        assert_eq!(escape_csv("safe"), "safe");
    }

    #[test]
    fn csv_export_keeps_formula_content_as_text_and_round_trips_it() {
        let mut items = sample_items();
        items[0].title = "=1+1".to_owned();
        items[0].text_content = Some("\t=2+2".to_owned());
        items[0].source_app = Some("'literal source".to_owned());
        items[1].text_content = Some("'=literal text".to_owned());
        let output = export_items(
            &items,
            &ExportOptions {
                format: ExportFormat::Csv,
                include_favorites: true,
                date_from_ms: None,
                date_to_ms: None,
                content_types: vec![],
            },
        )
        .unwrap();
        let mut reader = CsvReader::new(&output);
        reader.next_record().unwrap();
        let row = reader.next_record().unwrap();
        assert!(
            row[2].starts_with('\''),
            "CSV quoting alone still exposes a formula cell"
        );
        assert!(row[3].starts_with('\''));
        let database = Database::open_in_memory().unwrap();
        assert_eq!(
            import_from_csv(&output, &database).unwrap().imported_count,
            2
        );
        for original in items {
            let restored = database.get_item(&original.id).unwrap().unwrap();
            assert_eq!(restored.title, original.title);
            assert_eq!(restored.text_content, original.text_content);
            assert_eq!(restored.source_app, original.source_app);
        }
    }

    #[test]
    fn csv_without_encoding_marker_preserves_a_literal_apostrophe() {
        let database = Database::open_in_memory().unwrap();
        let csv = "id,kind,title,text_content\nlegacy,text,'title,'=literal\n";
        assert_eq!(import_from_csv(csv, &database).unwrap().imported_count, 1);
        let item = database.get_item("legacy").unwrap().unwrap();
        assert_eq!(item.title, "'title");
        assert_eq!(item.text_content.as_deref(), Some("'=literal"));
    }

    #[test]
    fn csv_reader_preserves_quoted_fields_with_separators_and_newlines() {
        let mut reader = CsvReader::new("a,\"b,c\",\"line1\nline2\",d\n");
        let record = reader.next_record().unwrap();
        assert_eq!(record, vec!["a", "b,c", "line1\nline2", "d"]);
        assert!(reader.next_record().is_none());
    }

    #[test]
    fn csv_reader_unescapes_doubled_quotes() {
        let mut reader = CsvReader::new("\"he said \"\"hi\"\"\",end\n");
        let record = reader.next_record().unwrap();
        assert_eq!(record, vec!["he said \"hi\"", "end"]);
    }

    #[test]
    fn csv_export_import_round_trips_text_and_link_items() {
        let items = sample_items();
        let csv = export_items(
            &items,
            &ExportOptions {
                format: ExportFormat::Csv,
                include_favorites: true,
                date_from_ms: None,
                date_to_ms: None,
                content_types: vec![],
            },
        )
        .unwrap();

        let database = crate::storage::Database::open_in_memory().unwrap();
        let summary = import_from_csv(&csv, &database).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(summary.skipped_count, 0);
        assert!(summary.errors.is_empty());

        let text_item = database.get_item("item-1").unwrap().unwrap();
        assert_eq!(text_item.kind, ClipboardKind::Text);
        assert_eq!(text_item.text_content.as_deref(), Some("Hello, world!"));
        assert_eq!(text_item.source_app.as_deref(), Some("Notepad"));
        assert!(text_item.is_favorite);

        let link_item = database.get_item("item-2").unwrap().unwrap();
        assert_eq!(link_item.kind, ClipboardKind::Link);
        assert_eq!(
            link_item.text_content.as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn csv_export_import_keeps_distinct_media_rows() {
        let items = vec![
            ClipboardItem {
                id: "img-1".to_owned(),
                kind: ClipboardKind::Image,
                title: "first".to_owned(),
                text_content: None,
                html_content: None,
                rtf_content: None,
                resource_path: None,
                preview_path: None,
                content_hash: "hash-image-1".to_owned(),
                source_app: None,
                size_bytes: 10,
                created_at_ms: 1,
                last_used_at_ms: None,
                is_favorite: false,
                icon_path: None,
                metadata_json: None,
            },
            ClipboardItem {
                id: "img-2".to_owned(),
                kind: ClipboardKind::Image,
                title: "second".to_owned(),
                text_content: None,
                html_content: None,
                rtf_content: None,
                resource_path: None,
                preview_path: None,
                content_hash: "hash-image-2".to_owned(),
                source_app: None,
                size_bytes: 20,
                created_at_ms: 2,
                last_used_at_ms: None,
                is_favorite: false,
                icon_path: None,
                metadata_json: None,
            },
        ];
        let csv = export_items(
            &items,
            &ExportOptions {
                format: ExportFormat::Csv,
                include_favorites: true,
                date_from_ms: None,
                date_to_ms: None,
                content_types: vec![],
            },
        )
        .unwrap();

        let database = crate::storage::Database::open_in_memory().unwrap();
        let summary = import_from_csv(&csv, &database).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(database.item_count().unwrap(), 2);
        assert_eq!(
            database.get_item("img-1").unwrap().unwrap().content_hash,
            "hash-image-1"
        );
        assert_eq!(
            database.get_item("img-2").unwrap().unwrap().content_hash,
            "hash-image-2"
        );
    }
}
