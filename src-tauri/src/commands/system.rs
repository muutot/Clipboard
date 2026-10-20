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
    copy_clipboard_item_files, delete_icon_files, list_icon_cache, open_external_url,
    replace_icon_file, reveal_in_explorer, save_clipboard_item_file, IconCacheEntry,
};
pub use super::signal::{stop_signal_requested, wait_for_stop};

use std::sync::Arc;

use crate::content;
use crate::content::{ContentMarkers, QuickAction, TextTransform, TransformOperation};
use crate::performance::{PerformanceSnapshot, PerformanceTracker};
use crate::search::SearchIndex;
use crate::storage::{refresh_database_backup, Database, RepairResult, StoragePaths};

/// Appends one frontend diagnostic line to the application log.
///
/// The webview can only write to its own console, which the user has to open
/// with developer tools to reach — so a failure reported in a toast could not be
/// diagnosed from the shipped log or a bug report. This is the same redaction
/// boundary the rest of the logging obeys: **ids, paths, counts, config keys and
/// error text only, never clipboard content**, so the caller is responsible for
/// passing a reason rather than the payload that produced it.
#[tauri::command]
pub fn log_frontend_message(level: String, message: String) {
    let line = format!("[frontend] {message}");
    match level.as_str() {
        "error" => crate::log_error!("{line}"),
        "warn" => crate::log_warn!("{line}"),
        _ => crate::log_event!("{line}"),
    }
}

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
pub fn restart_app(app: tauri::AppHandle) -> Result<(), String> {
    // A self-restart inside `tauri dev` orphans the app: the CLI tears down
    // the Vite dev server when the original process exits, so the restarted
    // process keeps already-loaded windows but every newly created webview
    // (settings, float panel) fails with a localhost connection-refused
    // page. Debug builds therefore refuse and tell the caller to restart
    // from the terminal; release builds restart for real.
    if cfg!(debug_assertions) {
        return Err("restart_blocked_in_dev".to_owned());
    }
    // `restart` diverges (terminates the process), so it doubles as the
    // function's tail without a trailing `Ok(())`.
    app.restart()
}

#[tauri::command]
pub fn record_search_interaction_latency(
    performance_tracker: tauri::State<'_, PerformanceTracker>,
    duration_ms: u64,
) {
    performance_tracker.record_search_input(duration_ms);
}

#[tauri::command]
pub fn get_performance_metrics(
    performance_tracker: tauri::State<'_, PerformanceTracker>,
) -> Result<PerformanceSnapshot, String> {
    Ok(performance_tracker.snapshot())
}

/// Async: the integrity check plus the full VACUUM INTO backup refresh can
/// block for seconds to minutes on a large database. The work runs on the
/// dedicated blocking pool so the window event loop stays responsive while
/// the repair action runs.
#[tauri::command]
pub async fn repair_database(app: tauri::AppHandle) -> Result<RepairResult, String> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let database = app.state::<Database>();
        let paths = app.state::<StoragePaths>();
        let result = database.repair().map_err(|e| e.to_string())?;
        if result.integrity_ok {
            refresh_database_backup(&database, &paths.database).map_err(|e| e.to_string())?;
        }
        Ok(result)
    })
    .await
    .map_err(|error| format!("repair task join failed: {error}"))?
}

#[tauri::command]
pub fn validate_search_index(
    search_index: tauri::State<'_, Arc<SearchIndex>>,
) -> Result<bool, String> {
    Ok(search_index.validate())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::TransformOperation;

    /// The dispatch table is the unit under test here: every wire name the
    /// frontend may send must resolve to its operation. The operation
    /// implementations themselves are covered by `content::transform` tests.
    #[test]
    fn transform_text_dispatches_every_supported_operation() {
        for (name, input, expected) in [
            ("stripWhitespace", "a b\tc", "abc"),
            ("stripNewlines", "a\nb\r\nc", "a b c"),
            ("toUpperCase", "aB", "AB"),
            ("toLowerCase", "aB", "ab"),
            ("trimWhitespace", "  ab  ", "ab"),
            ("collapseWhitespace", " a   b ", "a b"),
            ("base64Encode", "hi", "aGk="),
            ("base64Decode", "aGk=", "hi"),
            ("urlEncode", "a b", "a%20b"),
            ("urlDecode", "a%20b", "a b"),
        ] {
            let transform = transform_text(input.to_owned(), name.to_owned())
                .unwrap_or_else(|e| panic!("{name} must dispatch: {e}"));
            assert_eq!(transform.result, expected, "{name}");
            assert_eq!(transform.input, input, "{name} must echo the input");
        }
    }

    #[test]
    fn transform_text_maps_hash_and_format_operations() {
        for name in [
            "jsonFormat",
            "md5",
            "sha256",
            "sha512",
            "stripUrlTrackingParams",
            "cleanPaste",
        ] {
            transform_text("input".to_owned(), name.to_owned())
                .unwrap_or_else(|e| panic!("{name} must dispatch: {e}"));
        }
        let upper = transform_text("x".to_owned(), "toUpperCase".to_owned()).unwrap();
        assert_eq!(upper.operation, TransformOperation::ToUpperCase);
    }

    #[test]
    fn transform_text_rejects_unknown_operations() {
        let error = transform_text("x".to_owned(), "rot13".to_owned()).unwrap_err();
        assert!(error.contains("rot13"), "{error}");
    }

    #[test]
    fn content_marker_commands_mirror_direct_detection() {
        let markers = detect_content_markers("reach me at me@example.com".to_owned());
        assert!(markers.has_email);
        assert_eq!(markers.emails, vec!["me@example.com".to_owned()]);

        let empty = detect_content_markers("   ".to_owned());
        assert_eq!(empty, content::ContentMarkers::default());

        let actions = detect_content_actions("https://example.com/docs".to_owned());
        assert!(!actions.is_empty(), "a URL must surface quick actions");
    }

    #[test]
    fn log_frontend_message_accepts_every_level() {
        log_frontend_message("error".to_owned(), "synthetic error line".to_owned());
        log_frontend_message("warn".to_owned(), "synthetic warn line".to_owned());
        log_frontend_message("info".to_owned(), "synthetic info line".to_owned());
        log_frontend_message("anything".to_owned(), "synthetic fallback line".to_owned());
    }
}
