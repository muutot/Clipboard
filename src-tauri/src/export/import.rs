//! JSON, plain-text and CSV importers.

use super::*;

pub fn import_from_json(json: &str, database: &Database) -> Result<ImportSummary, String> {
    let _resource_publication = database.begin_resource_write();
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
pub(crate) fn sanitize_imported_path(path: Option<&str>) -> Option<String> {
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

pub(crate) fn normalize_imported_icon_key(icon_path: Option<&str>) -> Option<String> {
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
