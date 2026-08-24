// system.rs — content detection, text transform, diagnostics, and miscellaneous commands.
//
// Backward-compatibility re-exports — items migrated to dedicated modules.
pub use super::api::{
    get_local_api_status, run_cli_command, start_local_api, stop_local_api, LocalApiStatus,
};
pub use super::cleanup::{
    cleanup_orphan_storage_files, cleanup_orphan_storage_files_with_grace, cleanup_storage_files,
    enforce_history_cleanup, enforce_history_cleanup_for, StorageCleanupResult,
};
pub use super::files::{
    delete_icon_files, list_icon_cache, open_external_url, replace_icon_file, reveal_in_explorer,
    IconCacheEntry,
};
pub use super::signal::{stop_signal_requested, wait_for_stop};

use std::sync::Arc;

use crate::content;
use crate::content::{ContentMarkers, QuickAction, TextTransform, TransformOperation};
use crate::performance::{PerformanceSnapshot, PerformanceTracker};
use crate::search::SearchIndex;
use crate::storage::{refresh_database_backup, Database, RepairResult, StoragePaths};

#[tauri::command]
pub fn detect_content_markers(text: String) -> ContentMarkers {
    content::detect_markers(&text)
}

#[tauri::command]
pub fn detect_content_actions(text: String) -> Vec<QuickAction> {
    let markers = content::detect_markers(&text);
    content::detect_actions(&markers)
}

#[tauri::command]
pub fn transform_text(input: String, operation: String) -> Result<TextTransform, String> {
    let op = match operation.as_str() {
        "stripWhitespace" => TransformOperation::StripWhitespace,
        "stripNewlines" => TransformOperation::StripNewlines,
        "toUpperCase" => TransformOperation::ToUpperCase,
        "toLowerCase" => TransformOperation::ToLowerCase,
        "jsonFormat" => TransformOperation::JsonFormat,
        "base64Encode" => TransformOperation::Base64Encode,
        "base64Decode" => TransformOperation::Base64Decode,
        "urlEncode" => TransformOperation::UrlEncode,
        "urlDecode" => TransformOperation::UrlDecode,
        "md5" => TransformOperation::Md5,
        "sha256" => TransformOperation::Sha256,
        "sha512" => TransformOperation::Sha512,
        "trimWhitespace" => TransformOperation::TrimWhitespace,
        "collapseWhitespace" => TransformOperation::CollapseWhitespace,
        "stripUrlTrackingParams" => TransformOperation::StripUrlTrackingParams,
        "cleanPaste" => TransformOperation::CleanPaste,
        _ => return Err(format!("unknown transform operation: {}", operation)),
    };

    let result = op.apply(&input);

    Ok(TextTransform {
        input,
        operation: op,
        result,
    })
}

#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

#[tauri::command]
pub fn get_performance_metrics(
    performance_tracker: tauri::State<'_, PerformanceTracker>,
) -> Result<PerformanceSnapshot, String> {
    Ok(performance_tracker.snapshot())
}

#[tauri::command]
pub fn repair_database(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
) -> Result<RepairResult, String> {
    let result = database.repair().map_err(|e| e.to_string())?;
    if result.integrity_ok {
        refresh_database_backup(database.inner(), &paths.database).map_err(|e| e.to_string())?;
    }
    Ok(result)
}

#[tauri::command]
pub fn validate_search_index(
    search_index: tauri::State<'_, Arc<SearchIndex>>,
) -> Result<bool, String> {
    Ok(search_index.validate())
}
