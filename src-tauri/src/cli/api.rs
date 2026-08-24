use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::content;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::export::{export_database, ExportFormat, ExportOptions};
use crate::storage::{ClipboardRepository, Database};

const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;

/// A loopback-only HTTP API for scripts and local automation.
///
/// The server is opt-in from the Tauri command layer and never binds a public
/// interface. It owns a separate SQLite connection wrapped in `Arc`, so API
/// requests can safely run alongside the desktop UI.
///
/// Every request must present `Authorization: Bearer <token>` (the token is
/// persisted beside the config at `conf/api.token`), must target
/// `Host: 127.0.0.1:<port>` (or `localhost:<port>`), and must NOT carry an
/// `Origin` header. The last rule is what keeps browser pages 閳?the CSRF and
/// DNS-rebinding threat model 閳?unable to reach the clipboard history even
/// from a malicious website running on the same machine.
pub struct LocalApiServer {
    pub port: u16,
    database: Option<Arc<Database>>,
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
    page_size_limit: u32,
    search_page_size_limit: u32,
    token: Option<Arc<String>>,
}

impl LocalApiServer {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            database: None,
            stop_sender: None,
            handle: None,
            page_size_limit: 500,
            search_page_size_limit: 500,
            token: None,
        }
    }

    pub fn with_limits(mut self, page_size_limit: u32, search_page_size_limit: u32) -> Self {
        self.page_size_limit = page_size_limit;
        self.search_page_size_limit = search_page_size_limit;
        self
    }

    pub fn with_database(port: u16, database: Arc<Database>) -> Self {
        let mut server = Self::new(port);
        server.database = Some(database);
        server
    }

    /// Sets the bearer token clients must present. Required before start.
    pub fn set_token(&mut self, token: String) {
        self.token = Some(Arc::new(token));
    }

    pub fn start(&mut self) -> Result<u16, String> {
        let database = self
            .database
            .clone()
            .ok_or_else(|| "local API database is not configured".to_owned())?;
        self.start_with_database(database)
    }

    pub fn start_with_database(&mut self, database: Arc<Database>) -> Result<u16, String> {
        if self.handle.is_some() {
            return Err("local API server is already running".to_owned());
        }
        let token = self
            .token
            .clone()
            .ok_or_else(|| "local API token is not configured".to_owned())?;

        let listener = TcpListener::bind(("127.0.0.1", self.port))
            .map_err(|error| format!("failed to bind local API: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("failed to configure local API listener: {error}"))?;
        self.port = listener
            .local_addr()
            .map_err(|error| format!("failed to inspect local API listener: {error}"))?
            .port();
        self.database = Some(database.clone());

        let (stop_sender, stop_receiver) = mpsc::channel();
        let page_size_limit = self.page_size_limit;
        let search_page_size_limit = self.search_page_size_limit;
        let port = self.port;
        let handle = thread::Builder::new()
            .name("clipboard-local-api".to_owned())
            .spawn(move || {
                serve(
                    listener,
                    stop_receiver,
                    database,
                    page_size_limit,
                    search_page_size_limit,
                    token,
                    port,
                )
            })
            .map_err(|error| format!("failed to start local API: {error}"))?;
        self.stop_sender = Some(stop_sender);
        self.handle = Some(handle);
        Ok(self.port)
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    pub fn set_port(&mut self, port: u16) -> Result<(), String> {
        if self.is_running() {
            return Err("local API server is already running".to_owned());
        }
        self.port = port;
        Ok(())
    }

    pub fn set_limits(&mut self, page_size_limit: u32, search_page_size_limit: u32) {
        self.page_size_limit = page_size_limit;
        self.search_page_size_limit = search_page_size_limit;
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                handle
                    .join()
                    .map_err(|_| "local API thread terminated with a panic".to_owned())?;
            }
        }
        Ok(())
    }
}

impl Drop for LocalApiServer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn serve(
    listener: TcpListener,
    stop_receiver: mpsc::Receiver<()>,
    database: Arc<Database>,
    page_size_limit: u32,
    search_page_size_limit: u32,
    token: Arc<String>,
    port: u16,
) {
    loop {
        if matches!(
            stop_receiver.try_recv(),
            Ok(()) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            break;
        }

        match listener.accept() {
            Ok((stream, _peer)) => {
                // One thread per connection so a slow or idle client cannot
                // stall health checks and other callers behind it (the 2s
                // read timeout plus the request-size cap bound each thread).
                let database = Arc::clone(&database);
                let token = Arc::clone(&token);
                let spawned = thread::Builder::new()
                    .name("clipboard-local-api-conn".to_owned())
                    .spawn(move || {
                        if let Err(error) = handle_connection(
                            stream,
                            &database,
                            page_size_limit,
                            search_page_size_limit,
                            &token,
                            port,
                        ) {
                            crate::log_event!("[local-api] request failed: {error}");
                        }
                    });
                if let Err(error) = spawned {
                    crate::log_event!("[local-api] failed to spawn connection thread: {error}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                crate::log_event!("[local-api] listener failed: {error}");
                break;
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    database: &Database,
    page_size_limit: u32,
    search_page_size_limit: u32,
    token: &str,
    port: u16,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let request = read_request(&mut stream)?;
    if let Some(rejection) = authorize(&request, token, port) {
        return write_response(&mut stream, &rejection);
    }
    let response = dispatch(&request, database, page_size_limit, search_page_size_limit);
    write_response(&mut stream, &response)
}

/// Loopback hardening applied before any endpoint logic:
///
/// 1. Requests carrying an `Origin` header come from a browser context 閳?///    exactly the CSRF/DNS-rebinding threat model 閳?and are rejected even
///    with a valid token.
/// 2. `Host` must name the loopback address the server actually bound, which
///    defeats DNS-rebinding where a public hostname resolves to 127.0.0.1.
/// 3. Everything except `/health` requires `Authorization: Bearer <token>`.
fn authorize(request: &HttpRequest, token: &str, port: u16) -> Option<HttpResponse> {
    if request.header("origin").is_some() {
        return Some(error_response(
            403,
            "cross-origin browser requests are not allowed",
        ));
    }

    match request.header("host") {
        Some(host) => {
            let loopback =
                host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}");
            if !loopback {
                return Some(error_response(
                    400,
                    "host header must target the loopback API",
                ));
            }
        }
        None => return Some(error_response(400, "missing host header")),
    }

    let path = request.target.split('?').next().unwrap_or("");
    if path == "/health" {
        return None;
    }

    let expected = format!("Bearer {token}");
    let authorized = request
        .header("authorization")
        .is_some_and(|value| constant_time_eq(value.trim().as_bytes(), expected.as_bytes()));
    if !authorized {
        return Some(error_response(
            401,
            "missing or invalid bearer token (see conf/api.token)",
        ));
    }
    None
}

/// Length-independent comparison so response timing cannot leak token bytes.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    /// Case-insensitive header lookup; returns the first matching value.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut bytes = Vec::new();
    let header_end;
    loop {
        let mut chunk = [0u8; 4096];
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("read request: {error}"))?;
        if read == 0 {
            return Err("client closed the request before sending headers".to_owned());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err("request is too large".to_owned());
        }
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = position + 4;
            break;
        }
    }

    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "request headers are not valid UTF-8".to_owned())?;
    let mut lines = headers.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "missing request line".to_owned())?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| "missing HTTP method".to_owned())?
        .to_ascii_uppercase();
    let target = request_parts
        .next()
        .ok_or_else(|| "missing HTTP target".to_owned())?
        .to_owned();
    let mut header_fields = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().unwrap_or(0);
        }
        header_fields.push((name.trim().to_owned(), value.to_owned()));
    }
    if content_length > MAX_REQUEST_BYTES {
        return Err("request body is too large".to_owned());
    }

    while bytes.len() < header_end + content_length {
        let mut chunk = [0u8; 4096];
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("read request body: {error}"))?;
        if read == 0 {
            return Err("client closed the request before sending the body".to_owned());
        }
        bytes.extend_from_slice(&chunk[..read]);
    }

    Ok(HttpRequest {
        method,
        target,
        headers: header_fields,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

struct HttpResponse {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

fn write_response(stream: &mut TcpStream, response: &HttpResponse) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Response",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|_| stream.write_all(&response.body))
        .map_err(|error| format!("write response: {error}"))
}

fn dispatch(
    request: &HttpRequest,
    database: &Database,
    page_size_limit: u32,
    search_page_size_limit: u32,
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
    let query = parse_query(query);

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
            let items = match database.get_items_by_ids(std::slice::from_ref(id)) {
                Ok(items) => items,
                Err(error) => return error_response(500, &error.to_string()),
            };
            let Some(item) = items.into_iter().next() else {
                return error_response(404, "item not found");
            };
            let text = item
                .text_content
                .as_deref()
                .filter(|value| !value.is_empty())
                .unwrap_or(&item.title);
            match super::write_system_clipboard_text(text) {
                Ok(()) => json_response(200, &item),
                Err(error) => error_response(500, &error),
            }
        }
        ("DELETE", [segment, id]) if segment == "items" => match database.soft_delete(id) {
            Ok(true) => json_response(200, &ActionResponse { changed: true }),
            Ok(false) => error_response(404, "item not found"),
            Err(error) => error_response(500, &error.to_string()),
        },
        ("DELETE", [segment, id, permanent]) if segment == "items" && permanent == "permanent" => {
            match database.permanently_delete(id) {
                Ok(true) => json_response(200, &ActionResponse { changed: true }),
                Ok(false) => error_response(404, "deleted item not found"),
                Err(error) => error_response(500, &error.to_string()),
            }
        }
        ("POST", [segment, id, restore]) if segment == "items" && restore == "restore" => {
            match database.restore_deleted(id) {
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

fn response(status: u16, content_type: &'static str, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status,
        content_type,
        body,
    }
}

fn json_response<T: Serialize>(status: u16, value: &T) -> HttpResponse {
    match serde_json::to_vec(value) {
        Ok(body) => response(status, "application/json; charset=utf-8", body),
        Err(error) => error_response(500, &error.to_string()),
    }
}

fn error_response(status: u16, message: &str) -> HttpResponse {
    #[derive(Serialize)]
    struct ErrorBody<'a> {
        error: &'a str,
    }
    json_response(status, &ErrorBody { error: message })
}

fn content_type_for(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Json => "application/json; charset=utf-8",
        ExportFormat::Csv => "text/csv; charset=utf-8",
        ExportFormat::PlainText => "text/plain; charset=utf-8",
    }
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Some((decode_component(key).ok()?, decode_component(value).ok()?))
        })
        .collect()
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
    super::search_items_by_scanning(database, query, limit, scan_page_size)
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
    let item = super::build_text_clipboard_item(text.to_owned(), kind, "api", "Local API");
    let saved_id = database
        .save_item(&item)
        .map_err(|error| error.to_string())?;
    let mut saved = item;
    saved.id = saved_id;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ClipboardKind;

    fn request(method: &str, target: &str, body: &[u8]) -> HttpRequest {
        HttpRequest {
            method: method.to_owned(),
            target: target.to_owned(),
            headers: Vec::new(),
            body: body.to_vec(),
        }
    }

    fn authorized_request(method: &str, target: &str, token: &str, port: u16) -> HttpRequest {
        let mut http = request(method, target, b"");
        http.headers
            .push(("Host".to_owned(), format!("127.0.0.1:{port}")));
        if target != "/health" {
            http.headers
                .push(("Authorization".to_owned(), format!("Bearer {token}")));
        }
        http
    }

    #[test]
    fn health_endpoint_is_real_json() {
        let database = Database::open_in_memory().unwrap();
        let http = request("GET", "/health", b"");
        let response = dispatch(&http, &database, 500, 500);
        assert_eq!(response.status, 200);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["status"],
            "ok"
        );
    }

    #[test]
    fn paste_list_search_and_delete_endpoints_use_database() {
        let database = Database::open_in_memory().unwrap();
        let paste = request("POST", "/paste", br#"{"text":"api note"}"#);
        let response = dispatch(&paste, &database, 500, 500);
        assert_eq!(response.status, 201);
        let item: ClipboardItem = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(item.kind, ClipboardKind::Text);

        let search = request("GET", "/search?q=api%20note", b"");
        let response = dispatch(&search, &database, 500, 500);
        assert_eq!(response.status, 200);
        let results: Vec<ClipboardItem> = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(results.len(), 1);

        let delete = request("DELETE", &format!("/items/{}", item.id), b"");
        assert_eq!(dispatch(&delete, &database, 500, 500).status, 200);
        assert!(database
            .list_recent(10, 0, &crate::storage::HistoryFilter::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn requests_without_token_are_unauthorized() {
        let mut http = request("GET", "/items?limit=1", b"");
        http.headers
            .push(("Host".to_owned(), "127.0.0.1:8123".to_owned()));
        let rejection = authorize(&http, "secret-token", 8123).expect("must reject");
        assert_eq!(rejection.status, 401);
    }

    #[test]
    fn valid_token_and_loopback_host_are_accepted() {
        let http = authorized_request("GET", "/items?limit=1", "secret-token", 8123);
        assert!(authorize(&http, "secret-token", 8123).is_none());
    }

    #[test]
    fn non_loopback_host_headers_are_rejected() {
        let mut http = authorized_request("GET", "/items", "secret-token", 8123);
        http.headers[0] = ("Host".to_owned(), "evil.example.com:80".to_owned());
        let rejection = authorize(&http, "secret-token", 8123).expect("must reject");
        assert_eq!(rejection.status, 400);
    }

    #[test]
    fn browser_origin_requests_are_rejected_even_with_valid_token() {
        let mut http = authorized_request("GET", "/export", "secret-token", 8123);
        http.headers
            .push(("Origin".to_owned(), "https://evil.example".to_owned()));
        let rejection = authorize(&http, "secret-token", 8123).expect("must reject");
        assert_eq!(rejection.status, 403);
    }

    #[test]
    fn wrong_token_is_unauthorized() {
        let http = authorized_request("GET", "/items", "wrong-token", 8123);
        let rejection = authorize(&http, "secret-token", 8123).expect("must reject");
        assert_eq!(rejection.status, 401);
    }

    #[test]
    fn server_binds_loopback_and_stops_cleanly() {
        let database = Arc::new(Database::open_in_memory().unwrap());
        let mut server = LocalApiServer::with_database(0, database);
        server.set_token("test-token".to_owned());
        let port = server.start().unwrap();
        assert!(server.is_running());
        assert!(port > 0);
        server.stop().unwrap();
        assert!(!server.is_running());
    }
}
