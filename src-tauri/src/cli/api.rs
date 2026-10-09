use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::content;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::export::{export_database, ExportFormat, ExportOptions};
use crate::item_operations::{change_membership, copy_item_with, CopyContext, MembershipAction};
use crate::storage::{ClipboardRepository, Database};

const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
/// Guardrail against local thread-exhaustion: beyond this many concurrent
/// connection threads new connections receive an immediate 503.
const MAX_CONCURRENT_CONNECTIONS: usize = 32;
/// Writes must finish within this budget so a client that never reads its
/// response cannot pin a connection thread forever.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Total budget for reading one request (headers + body). The per-`read()`
/// socket timeout above restarts on every chunk, so without an overall
/// deadline a drip-feed client could pin one of the 32 connection threads
/// forever without ever presenting a token (authorization only runs after
/// the full request is read); 32 such connections would 503 every
/// legitimate caller.
const MAX_READ_DURATION: Duration = Duration::from_secs(10);

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
    copy_context: CopyContext,
    on_change: Arc<dyn Fn() + Send + Sync>,
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
            copy_context: CopyContext::default(),
            on_change: Arc::new(|| {}),
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
    pub fn set_copy_context(&mut self, context: CopyContext) {
        self.copy_context = context;
    }
    pub fn set_change_callback(&mut self, callback: Arc<dyn Fn() + Send + Sync>) {
        self.on_change = callback;
    }
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
        let active_connections = Arc::new(AtomicUsize::new(0));
        let context = ServeContext {
            database,
            page_size_limit: self.page_size_limit,
            search_page_size_limit: self.search_page_size_limit,
            token,
            port: self.port,
            active_connections,
            copy_context: self.copy_context.clone(),
            on_change: self.on_change.clone(),
        };
        let handle = thread::Builder::new()
            .name("clipboard-local-api".to_owned())
            .spawn(move || serve(listener, stop_receiver, context))
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

struct ServeContext {
    database: Arc<Database>,
    page_size_limit: u32,
    search_page_size_limit: u32,
    token: Arc<String>,
    port: u16,
    active_connections: Arc<AtomicUsize>,
    copy_context: CopyContext,
    on_change: Arc<dyn Fn() + Send + Sync>,
}

fn serve(listener: TcpListener, stop_receiver: mpsc::Receiver<()>, context: ServeContext) {
    let ServeContext {
        database,
        page_size_limit,
        search_page_size_limit,
        token,
        port,
        active_connections,
        copy_context,
        on_change,
    } = context;
    loop {
        if matches!(
            stop_receiver.try_recv(),
            Ok(()) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            break;
        }

        match listener.accept() {
            Ok((stream, _peer)) => {
                // Reserve the slot atomically: the previous check-then-add
                // could overshoot the cap when connections arrived in bursts.
                if active_connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONCURRENT_CONNECTIONS {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    let mut stream = stream;
                    let rejection = error_response(503, "too many concurrent connections");
                    let _ = write_response(&mut stream, &rejection);
                    continue;
                }
                // One thread per connection so a slow or idle client cannot
                // stall health checks and other callers behind it (the read/
                // write timeouts, the request-size cap, and the concurrency
                // limit bound each thread).
                let database = Arc::clone(&database);
                let token = Arc::clone(&token);
                let copy_context = copy_context.clone();
                let on_change = on_change.clone();
                let connection_counter = Arc::clone(&active_connections);
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
                            &copy_context,
                            on_change.as_ref(),
                        ) {
                            crate::log_error!("[local-api] request failed: {error}");
                        }
                        connection_counter.fetch_sub(1, Ordering::SeqCst);
                    });
                if let Err(error) = spawned {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    crate::log_error!("[local-api] failed to spawn connection thread: {error}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                crate::log_error!("[local-api] listener failed: {error}");
                break;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_connection(
    mut stream: TcpStream,
    database: &Database,
    page_size_limit: u32,
    search_page_size_limit: u32,
    token: &str,
    port: u16,
    copy_context: &CopyContext,
    on_change: &(dyn Fn() + Send + Sync),
) -> Result<(), String> {
    // Accepted sockets may inherit the listener's nonblocking flag on Windows.
    // Connection workers use bounded blocking reads; otherwise a delayed first
    // packet fails with WouldBlock instead of observing READ_TIMEOUT.
    stream
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(WRITE_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let request = read_request(&mut stream)?;
    if let Some(rejection) = authorize(&request, token, port) {
        return write_response(&mut stream, &rejection);
    }
    let response = dispatch_with_context(
        &request,
        database,
        page_size_limit,
        search_page_size_limit,
        copy_context,
    );
    if (200..300).contains(&response.status) && matches!(request.method.as_str(), "POST" | "DELETE")
    {
        on_change();
    }
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

/// Length-independent comparison so response timing cannot leak token bytes
/// or the expected token length. The loop always runs `max(left, right)`
/// iterations; a length difference is folded into the accumulator instead of
/// returning early.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let max = left.len().max(right.len());
    for index in 0..max {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(a ^ b);
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
    read_request_with_deadline(stream, Instant::now() + MAX_READ_DURATION)
}

fn read_request_with_deadline(
    stream: &mut TcpStream,
    deadline: Instant,
) -> Result<HttpRequest, String> {
    let mut bytes = Vec::new();
    let header_end;
    loop {
        if Instant::now() >= deadline {
            return Err("request read timed out".to_owned());
        }
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
        if Instant::now() >= deadline {
            return Err("request read timed out".to_owned());
        }
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

#[cfg(test)]
fn dispatch(
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
fn dispatch_with_context(
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

fn parse_query(query: &str) -> Result<std::collections::HashMap<String, String>, String> {
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
    fn constant_time_eq_matches_only_equal_slices() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"token", b"toke"));
        assert!(!constant_time_eq(b"toke", b"token"));
        assert!(!constant_time_eq(b"token", b"tokens"));
        assert!(!constant_time_eq(b"", b"x"));
    }

    #[test]
    fn parse_query_rejects_malformed_components() {
        let parsed = parse_query("limit=10&q=hello").unwrap();
        assert_eq!(parsed.get("limit").map(String::as_str), Some("10"));
        assert_eq!(parsed.get("q").map(String::as_str), Some("hello"));
        // `%FF` is not valid UTF-8 once decoded, so the component must be
        // rejected rather than silently dropped.
        assert!(parse_query("q=%FF").is_err());
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
    fn expired_read_deadline_rejects_without_blocking() {
        // A pre-expired deadline must fail fast even when a full request is
        // already waiting: this is the drip-feed guardrail. Use a real
        // loopback pair so the test exercises the socket read path.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        client
            .write_all(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        let (mut server_side, _) = listener.accept().unwrap();
        let error =
            read_request_with_deadline(&mut server_side, Instant::now() - Duration::from_secs(1))
                .expect_err("expired deadline must reject the request");
        assert!(error.contains("timed out"), "unexpected error: {error}");
    }

    #[test]
    fn media_copy_has_the_same_missing_resource_error_as_the_cli() {
        let db = Database::open_in_memory().unwrap();
        let mut item = super::super::build_text_clipboard_item(
            "not a media payload".into(),
            ClipboardKind::File,
            "test",
            "test",
        );
        item.last_used_at_ms = Some(1);
        db.save_item(&item).unwrap();
        let response = dispatch(
            &request("POST", &format!("/copy/{}", item.id), b""),
            &db,
            500,
            500,
        );
        assert_eq!(response.status, 500);
        assert!(String::from_utf8(response.body)
            .unwrap()
            .contains("resource missing"));
        let args = crate::cli::CliArgs {
            command: crate::cli::CliCommand::Copy,
            query: Some(item.id.clone()),
            limit: None,
            format: None,
            output_path: None,
        };
        assert!(crate::cli::run_cli_command(&args, &db, 500, 500)
            .unwrap_err()
            .contains("resource missing"));
        assert_eq!(
            db.get_item(&item.id).unwrap().unwrap().last_used_at_ms,
            Some(1)
        );
    }

    #[test]
    fn successful_api_mutation_notifies_but_reads_and_errors_do_not() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let mut server = LocalApiServer::with_database(0, db);
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        server.set_change_callback(Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        server.set_token("test".into());
        let port = server.start().unwrap();
        for (method, path, body, expected) in [
            ("GET", "/items", "", 0),
            ("DELETE", "/items/absent", "", 0),
            ("POST", "/paste", "sample", 1),
        ] {
            let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(socket, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer test\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).unwrap();
            assert_eq!(count.load(Ordering::SeqCst), expected);
        }
        server.stop().unwrap();
    }

    #[test]
    fn accepted_nonblocking_connection_waits_for_delayed_request_bytes() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let (stream, _) = listener.accept().unwrap();
        // Windows accepted sockets can inherit nonblocking mode from the listener.
        // Force it explicitly so every platform exercises the same precondition.
        stream.set_nonblocking(true).unwrap();
        thread::scope(|scope| {
            let (finished, result) = mpsc::channel();
            let (ready, started) = mpsc::channel();
            let worker = scope.spawn(move || {
                let database = Database::open_in_memory().unwrap();
                ready.send(()).unwrap();
                finished
                    .send(handle_connection(
                        stream,
                        &database,
                        500,
                        500,
                        "test",
                        port,
                        &CopyContext::default(),
                        &|| {},
                    ))
                    .unwrap();
            });
            started.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(
                matches!(
                    result.recv_timeout(Duration::from_millis(100)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ),
                "the server must wait for bytes instead of immediately returning WouldBlock"
            );
            write!(
                client,
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 200"));
            result
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap();
            worker.join().unwrap();
        });
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
