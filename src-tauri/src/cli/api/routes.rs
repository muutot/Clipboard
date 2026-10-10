//! Endpoint dispatch for the loopback API: URL routing to domain/database calls
//! plus the query, paste-body, and export-content-type helpers the endpoints use.

use serde::Serialize;

use crate::cli::{build_text_clipboard_item, search_items_by_scanning};
use crate::content;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::export::{export_database, ExportFormat, ExportOptions};
use crate::item_operations::{change_membership, copy_item_with, CopyContext, MembershipAction};
use crate::storage::{ClipboardRepository, Database};

use super::http::{error_response, json_response, response, HttpRequest, HttpResponse};

#[cfg(test)]
pub(super) fn dispatch(
    request: &HttpRequest,
    database: &Database,
    page_size_limit: u32,
    search_page_size_limit: u32,
) -> HttpResponse {
    dispatch_with_context(
        request,
        database,
        page_size_limit,
        search_page_size_limit,
        &CopyContext::default(),
    )
}

pub(super) fn dispatch_with_context(
    request: &HttpRequest,
    database: &Database,
    page_size_limit: u32,
    search_page_size_limit: u32,
    copy_context: &CopyContext,
) -> HttpResponse {
    if request.method == "OPTIONS" {
        return response(204, "", Vec::new());
    }

    let (path, query) = request
        .target
        .split_once('?')
        .map_or((request.target.as_str(), ""), |(path, query)| (path, query));
    let path_parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .map(decode_component)
        .collect::<Result<Vec<_>, _>>();
    let Ok(path_parts) = path_parts else {
        return error_response(400, "invalid URL encoding");
    };
    let query = match parse_query(query) {
        Ok(query) => query,
        Err(error) => return error_response(400, &error),
    };

    match (request.method.as_str(), path_parts.as_slice()) {
        ("GET", [segment]) if segment == "health" => {
            json_response(200, &HealthResponse { status: "ok" })
        }
        ("GET", [segment]) if segment == "items" => {
            let limit = query_limit(&query, page_size_limit);
            let offset = query
                .get("offset")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            match database.list_recent(limit, offset, &crate::storage::HistoryFilter::default()) {
                Ok(items) => json_response(200, &items),
                Err(error) => error_response(500, &error.to_string()),
            }
        }
        ("GET", [segment, deleted]) if segment == "items" && deleted == "deleted" => {
            let limit = query_limit(&query, page_size_limit);
            let offset = query
                .get("offset")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            match database.list_deleted(limit, offset) {
                Ok(items) => json_response(200, &items),
                Err(error) => error_response(500, &error.to_string()),
            }
        }
        ("GET", [segment]) if segment == "search" => {
            let search = query.get("q").map(String::as_str).unwrap_or("");
            match search_items(
                database,
                search,
                query_limit(&query, search_page_size_limit) as usize,
                page_size_limit as usize,
            ) {
                Ok(items) => json_response(200, &items),
                Err(error) => error_response(500, &error),
            }
        }
        ("GET", [segment]) if segment == "export" => {
            let format = match query.get("format").map(String::as_str).unwrap_or("json") {
                "json" => ExportFormat::Json,
                "csv" => ExportFormat::Csv,
                "text" | "txt" | "plain" | "plaintext" => ExportFormat::PlainText,
                _ => return error_response(400, "unknown export format"),
            };
            let options = ExportOptions {
                format,
                include_favorites: true,
                date_from_ms: None,
                date_to_ms: None,
                content_types: Vec::new(),
            };
            match export_database(database, &options) {
                Ok(output) => response(200, content_type_for(format), output.into_bytes()),
                Err(error) => error_response(500, &error),
            }
        }
        ("POST", [segment]) if segment == "paste" => {
            let body = String::from_utf8_lossy(&request.body).into_owned();
            let text = parse_paste_body(&body);
            if text.is_empty() {
                return error_response(400, "paste body is empty");
            }
            match save_text(database, &text) {
                Ok(item) => json_response(201, &item),
                Err(error) => error_response(500, &error),
            }
        }
        ("POST", [segment, id]) if segment == "copy" => {
            match copy_item_with(database, id, |item| copy_context.write(database, item)) {
                Ok((item, _)) => json_response(200, &item),
                Err(error) => error_response(
                    if error.to_string().starts_with("item not found:") {
                        404
                    } else {
                        500
                    },
                    &error.to_string(),
                ),
            }
        }
        ("DELETE", [segment, id]) if segment == "items" => {
            match change_membership(database, id, MembershipAction::Delete) {
                Ok(true) => json_response(200, &ActionResponse { changed: true }),
                Ok(false) => error_response(404, "item not found"),
                Err(error) => error_response(500, &error.to_string()),
            }
        }
        ("DELETE", [segment, id, permanent]) if segment == "items" && permanent == "permanent" => {
            match change_membership(database, id, MembershipAction::Remove) {
                Ok(true) => json_response(200, &ActionResponse { changed: true }),
                Ok(false) => error_response(404, "deleted item not found"),
                Err(error) => error_response(500, &error.to_string()),
            }
        }
        ("POST", [segment, id, restore]) if segment == "items" && restore == "restore" => {
            match change_membership(database, id, MembershipAction::Restore) {
                Ok(true) => json_response(200, &ActionResponse { changed: true }),
                Ok(false) => error_response(404, "deleted item not found"),
                Err(error) => error_response(500, &error.to_string()),
            }
        }
        _ => error_response(404, "endpoint not found"),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ActionResponse {
    changed: bool,
}

fn content_type_for(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Json => "application/json; charset=utf-8",
        ExportFormat::Csv => "text/csv; charset=utf-8",
        ExportFormat::PlainText => "text/plain; charset=utf-8",
    }
}

pub(super) fn parse_query(
    query: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    let mut parsed = std::collections::HashMap::new();
    for part in query.split('&').filter(|part| !part.is_empty()) {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        // Reject a malformed component instead of silently treating the
        // parameter as absent and returning unrelated results.
        let key = decode_component(key)?;
        let value = decode_component(value)?;
        parsed.insert(key, value);
    }
    Ok(parsed)
}

fn decode_component(value: &str) -> Result<String, String> {
    urlencoding::decode(value)
        .map(|decoded| decoded.into_owned())
        .map_err(|error| format!("invalid URL component: {error}"))
}

fn query_limit(query: &std::collections::HashMap<String, String>, max_limit: u32) -> u32 {
    query
        .get("limit")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(100)
        .clamp(1, max_limit)
}

fn search_items(
    database: &Database,
    query: &str,
    limit: usize,
    scan_page_size: usize,
) -> Result<Vec<ClipboardItem>, String> {
    search_items_by_scanning(database, query, limit, scan_page_size)
}

fn parse_paste_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(text) = value.get("text").and_then(serde_json::Value::as_str) {
                return text.to_owned();
            }
        }
    }
    body.to_owned()
}

fn save_text(database: &Database, text: &str) -> Result<ClipboardItem, String> {
    let markers = content::detect_markers(text);
    let kind = if markers.is_link {
        ClipboardKind::Link
    } else {
        ClipboardKind::Text
    };
    let item = build_text_clipboard_item(text.to_owned(), kind, "api", "Local API");
    let saved_id = database
        .save_item(&item)
        .map_err(|error| error.to_string())?;
    let mut saved = item;
    saved.id = saved_id;
    Ok(saved)
}
