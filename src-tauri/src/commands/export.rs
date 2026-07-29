use serde::Serialize;

use crate::export::{
    export_database, import_from_json, import_from_plain_text, ExportFormat, ExportOptions,
    ImportSummary,
};
use crate::storage::Database;

#[tauri::command]
pub fn export_clipboard_items(
    database: tauri::State<'_, Database>,
    format: String,
    include_favorites: Option<bool>,
    date_from_ms: Option<i64>,
    date_to_ms: Option<i64>,
    content_types: Option<Vec<String>>,
) -> Result<String, String> {
    let export_format = match format.as_str() {
        "json" => ExportFormat::Json,
        "csv" => ExportFormat::Csv,
        "plainText" => ExportFormat::PlainText,
        other => return Err(format!("unknown export format: {other}")),
    };

    let options = ExportOptions {
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
    };

    export_database(database.inner(), &options)
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
