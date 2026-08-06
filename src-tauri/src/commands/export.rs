use std::sync::Mutex;

use serde::Serialize;
use tauri::Emitter;

use crate::commands::clipboard::ClipboardHistoryInvalidated;
use crate::config::ConfigStore;
use crate::export::{
    export_database, import_from_json, import_from_plain_text, import_from_ppaste_backup,
    write_export_file, ExportFormat, ExportOptions, ImportSummary, BACKUP_EXTENSION,
};
use crate::storage::{ClipboardRepository, Database, StoragePaths};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFileResult {
    path: String,
    format: String,
    byte_count: usize,
}

fn build_export_options(
    format: &str,
    include_favorites: Option<bool>,
    date_from_ms: Option<i64>,
    date_to_ms: Option<i64>,
    content_types: Option<Vec<String>>,
) -> Result<ExportOptions, String> {
    let export_format = match format {
        "json" => ExportFormat::Json,
        "csv" => ExportFormat::Csv,
        "plainText" => ExportFormat::PlainText,
        other => return Err(format!("unknown export format: {other}")),
    };

    Ok(ExportOptions {
        format: export_format,
        include_favorites: include_favorites.unwrap_or(true),
        date_from_ms,
        date_to_ms,
        content_types: content_types.unwrap_or_else(|| {
            vec![
                "text".to_owned(),
                "link".to_owned(),
                "image".to_owned(),
                "file".to_owned(),
            ]
        }),
    })
}

#[tauri::command]
pub fn export_clipboard_items(
    database: tauri::State<'_, Database>,
    format: String,
    include_favorites: Option<bool>,
    date_from_ms: Option<i64>,
    date_to_ms: Option<i64>,
    content_types: Option<Vec<String>>,
) -> Result<String, String> {
    let options = build_export_options(
        &format,
        include_favorites,
        date_from_ms,
        date_to_ms,
        content_types,
    )?;
    export_database(database.inner(), &options)
}

fn export_database_to_path(
    database: &Database,
    path: &str,
    format: &str,
    include_favorites: Option<bool>,
    date_from_ms: Option<i64>,
    date_to_ms: Option<i64>,
    content_types: Option<Vec<String>>,
) -> Result<ExportFileResult, String> {
    let options = build_export_options(
        format,
        include_favorites,
        date_from_ms,
        date_to_ms,
        content_types,
    )?;
    let content = export_database(database, &options)?;
    write_export_file(path, &content)?;
    Ok(ExportFileResult {
        path: path.to_owned(),
        format: format.to_owned(),
        byte_count: content.len(),
    })
}

#[tauri::command]
pub fn export_to_file(
    database: tauri::State<'_, Database>,
    path: String,
    format: String,
    include_favorites: Option<bool>,
    date_from_ms: Option<i64>,
    date_to_ms: Option<i64>,
    content_types: Option<Vec<String>>,
) -> Result<ExportFileResult, String> {
    export_database_to_path(
        database.inner(),
        &path,
        &format,
        include_favorites,
        date_from_ms,
        date_to_ms,
        content_types,
    )
}

#[tauri::command]
pub fn import_clipboard_items(
    database: tauri::State<'_, Database>,
    json: String,
    format: Option<String>,
) -> Result<ImportSummary, String> {
    let normalized = format
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| {
            if json.trim_start().starts_with('[') {
                "json".to_owned()
            } else {
                "plaintext".to_owned()
            }
        });
    match normalized.as_str() {
        "json" => import_from_json(&json, database.inner()),
        "text" | "txt" | "plain" | "plaintext" | "plain-text" => {
            import_from_plain_text(&json, database.inner())
        }
        other => Err(format!("unknown import format: {other}")),
    }
}

fn import_database_from_path(database: &Database, path: &str) -> Result<ImportSummary, String> {
    let content =
        std::fs::read_to_string(path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let is_json =
        path.to_ascii_lowercase().ends_with(".json") || content.trim_start().starts_with('[');
    if is_json {
        import_from_json(&content, database)
    } else {
        import_from_plain_text(&content, database)
    }
}

#[tauri::command]
pub fn import_from_file(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    paths: tauri::State<'_, StoragePaths>,
    app: tauri::AppHandle,
    path: String,
) -> Result<ImportSummary, String> {
    let lower = path.to_ascii_lowercase();
    let mut summary = if lower.ends_with(BACKUP_EXTENSION) {
        import_from_ppaste_backup(&path, database.inner(), paths.inner())
    } else {
        import_database_from_path(database.inner(), &path)
    }?;
    annotate_truncation_risk(&mut summary, &database, &config)?;
    if summary.imported_count > 0 {
        let _ = app.emit(
            "clipboard-history-invalidated",
            ClipboardHistoryInvalidated {
                deleted_ids: Vec::new(),
            },
        );
    }
    Ok(summary)
}

fn annotate_truncation_risk(
    summary: &mut ImportSummary,
    database: &Database,
    config: &tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<(), String> {
    let guard = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?;
    let max_items = guard.max_items();
    let active = database.item_count().map_err(|error| error.to_string())?;
    summary.max_items = max_items;
    summary.pending_truncation = active.saturating_sub(u64::from(max_items));
    Ok(())
}

#[tauri::command]
pub fn get_export_formats() -> Result<Vec<ExportFormatInfo>, String> {
    Ok(vec![
        ExportFormatInfo {
            id: "json".to_owned(),
            label: "JSON".to_owned(),
            extension: ".json".to_owned(),
        },
        ExportFormatInfo {
            id: "csv".to_owned(),
            label: "CSV".to_owned(),
            extension: ".csv".to_owned(),
        },
        ExportFormatInfo {
            id: "plainText".to_owned(),
            label: "Plain Text".to_owned(),
            extension: ".txt".to_owned(),
        },
    ])
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFormatInfo {
    id: String,
    label: String,
    extension: String,
}

#[tauri::command]
pub fn get_import_formats() -> Result<Vec<ImportFormatInfo>, String> {
    Ok(vec![
        ImportFormatInfo {
            id: "pastebackup".to_owned(),
            label: "PPaste Backup".to_owned(),
            extension: BACKUP_EXTENSION.to_owned(),
        },
        ImportFormatInfo {
            id: "json".to_owned(),
            label: "JSON".to_owned(),
            extension: ".json".to_owned(),
        },
        ImportFormatInfo {
            id: "plainText".to_owned(),
            label: "Plain Text".to_owned(),
            extension: ".txt".to_owned(),
        },
    ])
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFormatInfo {
    id: String,
    label: String,
    extension: String,
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
    fn build_export_options_maps_formats_and_defaults() {
        let options = build_export_options("json", None, None, None, None).unwrap();
        assert_eq!(options.format, ExportFormat::Json);
        assert!(options.include_favorites);
        assert_eq!(
            options.content_types,
            vec![
                "text".to_owned(),
                "link".to_owned(),
                "image".to_owned(),
                "file".to_owned()
            ]
        );
        assert!(options.date_from_ms.is_none());
        assert!(options.date_to_ms.is_none());

        assert_eq!(
            build_export_options("csv", None, None, None, None)
                .unwrap()
                .format,
            ExportFormat::Csv
        );
        assert_eq!(
            build_export_options("plainText", None, None, None, None)
                .unwrap()
                .format,
            ExportFormat::PlainText
        );
        assert!(build_export_options("yaml", None, None, None, None).is_err());
    }

    #[test]
    fn export_to_path_writes_the_selected_format() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        for item in sample_items() {
            crate::storage::ClipboardRepository::save_item(&database, &item).unwrap();
        }
        let path =
            std::env::temp_dir().join(format!("clipboard-export-test-{}.json", std::process::id()));

        let result = export_database_to_path(
            &database,
            path.to_str().unwrap(),
            "json",
            Some(true),
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(result.format, "json");
        assert_eq!(
            result.byte_count,
            std::fs::metadata(&path).unwrap().len() as usize
        );

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("item-1"));
        assert!(content.contains("Hello, world!"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_from_path_detects_json_by_extension() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        let json = serde_json::to_string(&sample_items()).unwrap();
        let path =
            std::env::temp_dir().join(format!("clipboard-import-test-{}.json", std::process::id()));
        std::fs::write(&path, json).unwrap();

        let summary = import_database_from_path(&database, path.to_str().unwrap()).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(summary.skipped_count, 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_from_path_falls_back_to_plain_text() {
        let database = crate::storage::Database::open_in_memory().unwrap();
        let path =
            std::env::temp_dir().join(format!("clipboard-import-test-{}.txt", std::process::id()));
        std::fs::write(&path, "first chunk\n---\nsecond chunk\n").unwrap();

        let summary = import_database_from_path(&database, path.to_str().unwrap()).unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(database.item_count().unwrap(), 2);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_formats_cover_ppaste_json_and_plain_text() {
        let formats = get_import_formats().unwrap();
        assert_eq!(formats.len(), 3);
        assert_eq!(formats[0].id, "pastebackup");
        assert_eq!(formats[0].extension, BACKUP_EXTENSION);
        assert_eq!(formats[1].id, "json");
        assert_eq!(formats[1].extension, ".json");
        assert_eq!(formats[2].id, "plainText");
        assert_eq!(formats[2].extension, ".txt");
    }
}
