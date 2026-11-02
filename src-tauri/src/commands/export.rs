use std::sync::Mutex;

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::commands::clipboard::ClipboardHistoryInvalidated;
use crate::config::ConfigStore;
use crate::export::{
    export_database, export_database_to_file, import_from_csv, import_from_json,
    import_from_plain_text, import_from_ppaste_backup, ExportFormat, ExportOptions, ImportSummary,
    BACKUP_EXTENSION,
};
use crate::storage::{ClipboardRepository, Database, StoragePaths};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFileResult {
    path: String,
    format: String,
    byte_count: usize,
}

fn start_backup(app: &tauri::AppHandle) -> Result<crate::background_operations::Operation, String> {
    app.state::<crate::background_operations::BackgroundOperations>()
        .start(
            crate::background_operations::OperationKind::Backup,
            &clipboard_sync::cancellation::CancellationToken::default(),
        )
}

#[tauri::command]
pub async fn create_resource_backup(
    app: tauri::AppHandle,
    path: String,
) -> Result<ExportFileResult, String> {
    let operation = start_backup(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = (|| {
            let bytes = crate::export::backup::create_with_progress(
                app.state::<Database>().inner(),
                std::path::Path::new(&path),
                &mut |phase, completed, total| operation.progress(phase, completed, total),
            )?;
            Ok(ExportFileResult {
                path,
                format: "clipbackup".into(),
                byte_count: usize::try_from(bytes).map_err(|e| e.to_string())?,
            })
        })();
        operation.finish(outcome)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn preview_resource_backup(
    app: tauri::AppHandle,
    path: String,
) -> Result<crate::export::backup::BackupPreview, String> {
    let operation = start_backup(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = crate::export::backup::preview_with_progress(
            std::path::Path::new(&path),
            app.state::<Database>().inner(),
            &mut |phase, completed, total| operation.progress(phase, completed, total),
        );
        operation.finish(outcome)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn restore_resource_backup(
    app: tauri::AppHandle,
    path: String,
    fingerprint: String,
) -> Result<ImportSummary, String> {
    let operation = start_backup(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = (|| {
            let capture = app.state::<crate::state::CaptureState>();
            let _maintenance = crate::commands::lock::lock_state(
                &capture.storage_maintenance_lock,
                "storage maintenance lock is poisoned",
            )?;
            let database = app.state::<Database>();
            let mut result = crate::export::backup::restore_with_progress(
                std::path::Path::new(&path),
                &fingerprint,
                database.inner(),
                app.state::<StoragePaths>().inner(),
                &mut |phase, completed, total| operation.progress(phase, completed, total),
            )?;
            // Import has committed. A failed diagnostic must not present it as a failed restore.
            if let Err(error) = annotate_truncation_risk(
                &mut result,
                database.inner(),
                app.state::<Mutex<ConfigStore>>().inner(),
            ) {
                result.errors.push(error);
            }
            if result.imported_count > 0 {
                app.state::<crate::commands::clipboard::SearchResultCache>()
                    .clear();
                for event in ["clipboard-history-invalidated", "tags-changed"] {
                    if let Err(error) = app.emit(
                        event,
                        ClipboardHistoryInvalidated {
                            deleted_ids: Vec::new(),
                        },
                    ) {
                        crate::log_warn!("[backup] unable to broadcast {event}: {error}");
                    }
                }
            }
            Ok(result)
        })();
        operation.finish(outcome)
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Async: exporting the full history performs blocking SQLite and
/// serialization work, so it runs on the dedicated blocking pool instead of
/// the main thread (same pattern as `export_to_file`).
#[tauri::command]
pub async fn export_clipboard_items(
    app: tauri::AppHandle,
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
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        export_database(database.inner(), &options)
    })
    .await
    .map_err(|error| format!("export task join failed: {error}"))?
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
    let byte_count = export_database_to_file(database, &options, std::path::Path::new(path))?;
    Ok(ExportFileResult {
        path: path.to_owned(),
        format: format.to_owned(),
        byte_count: usize::try_from(byte_count).map_err(|error| error.to_string())?,
    })
}

/// Async: large exports perform blocking SQLite and file work, so the work runs
/// on the dedicated blocking pool instead of occupying an async worker (which
/// would also stall other commands while the database mutex is held).
#[tauri::command]
pub async fn export_to_file(
    app: tauri::AppHandle,
    path: String,
    format: String,
    include_favorites: Option<bool>,
    date_from_ms: Option<i64>,
    date_to_ms: Option<i64>,
    content_types: Option<Vec<String>>,
) -> Result<ExportFileResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        export_database_to_path(
            database.inner(),
            &path,
            &format,
            include_favorites,
            date_from_ms,
            date_to_ms,
            content_types,
        )
    })
    .await
    .map_err(|error| format!("export task join failed: {error}"))?
}

/// Async: importing a payload up to the 256 MiB cap performs blocking
/// serialization and SQLite work, so it runs on the dedicated blocking pool
/// (same pattern as `import_from_file`).
#[tauri::command]
pub async fn import_clipboard_items(
    app: tauri::AppHandle,
    json: String,
    format: Option<String>,
) -> Result<ImportSummary, String> {
    if json.len() as u64 > MAX_IMPORT_FILE_BYTES {
        return Err(format!(
            "import payload is {} bytes, over the {MAX_IMPORT_FILE_BYTES} byte import limit",
            json.len()
        ));
    }
    let normalized = format
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| {
            if json.trim_start().starts_with('[') {
                "json".to_owned()
            } else if json
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("id,kind,title")
                || json.lines().next().is_some_and(|line| {
                    line.split(',')
                        .any(|column| column.trim().eq_ignore_ascii_case("text_content"))
                })
            {
                "csv".to_owned()
            } else {
                "plaintext".to_owned()
            }
        });
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        match normalized.as_str() {
            "json" => import_from_json(&json, database.inner()),
            "csv" => import_from_csv(&json, database.inner()),
            "text" | "txt" | "plain" | "plaintext" | "plain-text" => {
                import_from_plain_text(&json, database.inner())
            }
            other => Err(format!("unknown import format: {other}")),
        }
    })
    .await
    .map_err(|error| format!("import task join failed: {error}"))?
}

const MAX_IMPORT_FILE_BYTES: u64 = 256 * 1024 * 1024;

fn read_import_file(path: &str) -> Result<String, String> {
    let source =
        std::fs::File::open(path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let metadata = source
        .metadata()
        .map_err(|error| format!("failed to inspect {path}: {error}"))?;
    read_import_file_contents(source, path, metadata.len(), MAX_IMPORT_FILE_BYTES)
}

fn read_import_file_contents(
    source: impl std::io::Read,
    path: &str,
    declared_bytes: u64,
    max_bytes: u64,
) -> Result<String, String> {
    if declared_bytes > max_bytes {
        return Err(format!(
            "{path} is {declared_bytes} bytes, over the {max_bytes} byte import limit"
        ));
    }
    use std::io::Read;
    // Metadata is only an early rejection hint: an open file may still grow.
    // Read at most one sentinel byte beyond the cap, before UTF-8 decoding.
    let mut bytes = Vec::new();
    source
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {path}: {error}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!(
            "{path} exceeds the {max_bytes} byte import limit while being read"
        ));
    }
    String::from_utf8(bytes).map_err(|error| format!("failed to read {path}: {error}"))
}

fn import_database_from_path(database: &Database, path: &str) -> Result<ImportSummary, String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".csv") {
        let content = read_import_file(path)?;
        return import_from_csv(&content, database);
    }
    let content = read_import_file(path)?;
    let is_json = lower.ends_with(".json") || content.trim_start().starts_with('[');
    if is_json {
        import_from_json(&content, database)
    } else {
        import_from_plain_text(&content, database)
    }
}

fn import_from_path_with_summary(
    database: &Database,
    config: &Mutex<ConfigStore>,
    paths: &StoragePaths,
    path: &str,
) -> Result<ImportSummary, String> {
    let mut summary = if path.to_ascii_lowercase().ends_with(BACKUP_EXTENSION) {
        import_from_ppaste_backup(path, database, paths)?
    } else {
        import_database_from_path(database, path)?
    };
    // Rows have committed. Keep the success summary and invalidation path even
    // if the optional capacity diagnostic cannot be read.
    if let Err(error) = annotate_truncation_risk(&mut summary, database, config) {
        summary.errors.push(error);
    }
    Ok(summary)
}

/// Async: importing a large backup performs blocking SQLite and file work, so
/// it runs on the dedicated blocking pool instead of an async worker.
#[tauri::command]
pub async fn import_from_file(
    app: tauri::AppHandle,
    path: String,
) -> Result<ImportSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        let config = app.state::<Mutex<ConfigStore>>();
        let paths = app.state::<StoragePaths>();
        let summary =
            import_from_path_with_summary(database.inner(), config.inner(), paths.inner(), &path)?;
        if summary.imported_count > 0 {
            if let Err(error) = app.emit(
                "clipboard-history-invalidated",
                ClipboardHistoryInvalidated {
                    deleted_ids: Vec::new(),
                },
            ) {
                crate::log_event!("[import] failed to emit history-invalidated: {error}");
            }
        }
        Ok(summary)
    })
    .await
    .map_err(|error| format!("import task join failed: {error}"))?
}

fn annotate_truncation_risk(
    summary: &mut ImportSummary,
    database: &Database,
    config: &Mutex<ConfigStore>,
) -> Result<(), String> {
    let max_items = {
        // Read the config value, then release the config lock before touching
        // the database so the lock order stays one-directional.
        let guard = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
        guard.max_items()
    };
    let active = database
        .evictable_item_count()
        .map_err(|error| error.to_string())?;
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
            id: "csv".to_owned(),
            label: "CSV".to_owned(),
            extension: ".csv".to_owned(),
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

    #[test]
    fn import_diagnostic_failure_preserves_a_committed_success_summary() {
        let root = std::env::temp_dir().join(format!(
            "clipboard-import-diagnostic-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = StoragePaths::initialize(root.clone()).unwrap();
        let database = Database::open_in_memory().unwrap();
        let config = Mutex::new(ConfigStore::load(&root).unwrap());
        let path = root.join("input.txt");
        std::fs::write(&path, "diagnostic failure must not hide this record").unwrap();
        // A real poisoned config mutex fails the post-commit capacity query.
        // Import reads no configuration until after the transaction commits.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = config.lock().unwrap();
            panic!("inject poisoned configuration lock");
        }));
        let outcome =
            import_from_path_with_summary(&database, &config, &paths, path.to_str().unwrap());
        assert_eq!(
            database.item_count().unwrap(),
            1,
            "the import already committed even if the diagnostic failed"
        );
        let summary =
            outcome.expect("a post-commit diagnostic must not turn an import into an IPC error");
        assert_eq!(summary.imported_count, 1);
        assert_eq!(summary.skipped_count, 0);
        assert_eq!(summary.errors, vec!["configuration lock is poisoned"]);
        assert_eq!(summary.pending_truncation, 0);
        assert_eq!(summary.max_items, 0);
        // A retry sees the same persisted row rather than another successful insert.
        let retry =
            import_from_path_with_summary(&database, &config, &paths, path.to_str().unwrap())
                .unwrap();
        assert_eq!(retry.imported_count, 0);
        assert_eq!(retry.skipped_count, 1);
        assert_eq!(database.item_count().unwrap(), 1);
        drop(database);
        drop(config);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn import_diagnostic_success_reports_capacity_and_preserves_parser_errors() {
        let root =
            std::env::temp_dir().join(format!("clipboard-import-summary-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths::initialize(root.clone()).unwrap();
        let database = Database::open_in_memory().unwrap();
        let config = Mutex::new(ConfigStore::load(&root).unwrap());
        let path = root.join("input.json");
        let mut items = sample_items();
        let mut invalid = items[0].clone();
        invalid.id = "invalid-title".into();
        invalid.title = "x".repeat(4097);
        invalid.content_hash = "invalid-title-hash".into();
        items.push(invalid);
        std::fs::write(&path, serde_json::to_string(&items).unwrap()).unwrap();
        let summary =
            import_from_path_with_summary(&database, &config, &paths, path.to_str().unwrap())
                .unwrap();
        assert_eq!(summary.imported_count, 2);
        assert_eq!(summary.skipped_count, 1);
        assert_eq!(summary.max_items, config.lock().unwrap().max_items());
        assert_eq!(summary.pending_truncation, 0);
        assert_eq!(
            summary.errors.len(),
            1,
            "row-validation errors must remain visible"
        );
        assert!(summary.errors[0].contains("title"));
        assert_eq!(database.item_count().unwrap(), 2);
        std::fs::write(&path, "[").unwrap();
        assert!(
            import_from_path_with_summary(&database, &config, &paths, path.to_str().unwrap())
                .is_err(),
            "a parser failure before commit must still fail the command"
        );
        assert_eq!(database.item_count().unwrap(), 2);
        drop(database);
        drop(config);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn import_file_growth_after_size_snapshot_cannot_bypass_the_read_budget() {
        use std::io::{Seek, Write};
        let root =
            std::env::temp_dir().join(format!("clipboard-import-growth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("growing.txt");
        std::fs::write(&path, b"small").unwrap();
        let declared = std::fs::metadata(&path).unwrap().len();
        let mut source = std::fs::File::open(&path).unwrap();
        // An independent file handle appends after the production size snapshot.
        let mut writer = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writer.write_all(&[b'x'; 64]).unwrap();
        writer.flush().unwrap();
        let result = read_import_file_contents(&mut source, "growing.txt", declared, 32);
        assert!(
            result.is_err(),
            "growth must fail before an oversized payload reaches the parser: {result:?}"
        );
        assert_eq!(
            source.stream_position().unwrap(),
            33,
            "the rejection must consume only the cap and one sentinel byte"
        );
        drop(source);
        drop(writer);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn import_file_replacement_after_size_snapshot_cannot_bypass_the_read_budget() {
        let root =
            std::env::temp_dir().join(format!("clipboard-import-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("replaced.txt");
        std::fs::write(&path, b"small").unwrap();
        let declared = std::fs::metadata(&path).unwrap().len();
        let replacement = root.join("other.txt");
        std::fs::write(&replacement, [b'x'; 64]).unwrap();
        crate::storage::replace_file(&replacement, &path).unwrap();
        let source = std::fs::File::open(&path).unwrap();
        let result = read_import_file_contents(source, "replaced.txt", declared, 32);
        assert!(
            result.is_err(),
            "replacement must not inherit the former file's size permission: {result:?}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn import_file_read_budget_accepts_exact_utf8_and_reports_invalid_text() {
        let text = "\u{FEFF}中文\r\n";
        assert_eq!(
            read_import_file_contents(
                text.as_bytes(),
                "unicode.txt",
                text.len() as u64,
                text.len() as u64
            )
            .unwrap(),
            text
        );
        assert!(read_import_file_contents(&[0xff][..], "bad.txt", 1, 1)
            .unwrap_err()
            .contains("failed to read"));
        assert!(read_import_file_contents(&b"large"[..], "large.txt", 5, 4)
            .unwrap_err()
            .contains("over the 4 byte import limit"));
    }

    #[test]
    fn import_file_read_budget_rejects_growth_before_utf8_decoding() {
        // The sentinel lands inside the final multi-byte code point. A size
        // violation must remain a size error, rather than a UTF-8 parse error.
        let result = read_import_file_contents("abc中文".as_bytes(), "grown.txt", 3, 4);
        assert!(result
            .unwrap_err()
            .contains("exceeds the 4 byte import limit"));
        assert_eq!(
            read_import_file_contents(&b""[..], "empty.txt", 0, 0).unwrap(),
            ""
        );
    }

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
    fn import_formats_cover_ppaste_json_csv_and_plain_text() {
        let formats = get_import_formats().unwrap();
        assert_eq!(formats.len(), 4);
        assert_eq!(formats[0].id, "pastebackup");
        assert_eq!(formats[0].extension, BACKUP_EXTENSION);
        assert_eq!(formats[1].id, "json");
        assert_eq!(formats[1].extension, ".json");
        assert_eq!(formats[2].id, "csv");
        assert_eq!(formats[2].extension, ".csv");
        assert_eq!(formats[3].id, "plainText");
        assert_eq!(formats[3].extension, ".txt");
    }
}
