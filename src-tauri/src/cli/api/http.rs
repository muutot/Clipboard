//! HTTP transport for the loopback API: the constants that bound resource use,
//! the accept loop, the per-connection handler, HTTP request parsing, the
//! loopback/auth gate, and response construction plus writing.
//!
//! The items shared with `routes.rs` and `tests.rs` are `pub(super)` so they
//! stay inside the `api` module.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::item_operations::CopyContext;
use crate::storage::Database;

use super::routes::dispatch_with_context;

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

pub(super) struct ServeContext {
    pub(super) database: Arc<Database>,
    pub(super) page_size_limit: u32,
    pub(super) search_page_size_limit: u32,
    pub(super) token: Arc<String>,
    pub(super) port: u16,
    pub(super) active_connections: Arc<AtomicUsize>,
    pub(super) copy_context: CopyContext,
    pub(super) on_change: Arc<dyn Fn() + Send + Sync>,
}

/// Retain sockets as well as joins so shutdown can interrupt blocked IO before
/// waiting for handlers. Drop also drains them if the listener unwinds.
#[derive(Default)]
struct Connections(Vec<(TcpStream, JoinHandle<()>)>);

impl Connections {
    fn reap(&mut self) {
        let mut index = 0;
        while index < self.0.len() {
            if self.0[index].1.is_finished() {
                let (_, handle) = self.0.swap_remove(index);
                if handle.join().is_err() {
                    crate::log_error!("[local-api] connection thread panicked");
                }
            } else {
                index += 1;
            }
        }
    }
}

impl Drop for Connections {
    fn drop(&mut self) {
        for (stream, _) in &self.0 {
            let _ = stream.shutdown(Shutdown::Both);
        }
        for (_, handle) in self.0.drain(..) {
            if handle.join().is_err() {
                crate::log_error!("[local-api] connection thread panicked during shutdown");
            }
        }
    }
}

struct ConnectionSlot(Arc<AtomicUsize>);
impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) fn serve(
    listener: TcpListener,
    stop_receiver: mpsc::Receiver<()>,
    context: ServeContext,
) {
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
    let mut connections = Connections::default();
    loop {
        connections.reap();
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
                let retained_stream = match stream.try_clone() {
                    Ok(stream) => stream,
                    Err(error) => {
                        active_connections.fetch_sub(1, Ordering::SeqCst);
                        crate::log_error!("[local-api] failed to retain connection: {error}");
                        continue;
                    }
                };
                let spawned = thread::Builder::new()
                    .name("clipboard-local-api-conn".to_owned())
                    .spawn(move || {
                        let _slot = ConnectionSlot(connection_counter);
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
                    });
                match spawned {
                    Ok(handle) => connections.0.push((retained_stream, handle)),
                    Err(error) => {
                        active_connections.fetch_sub(1, Ordering::SeqCst);
                        crate::log_error!("[local-api] failed to spawn connection thread: {error}");
                    }
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
pub(super) fn handle_connection(
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
pub(super) fn authorize(request: &HttpRequest, token: &str, port: u16) -> Option<HttpResponse> {
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
pub(super) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
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
pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
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

pub(super) fn read_request_with_deadline(
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

pub(super) struct HttpResponse {
    pub(super) status: u16,
    pub(super) content_type: &'static str,
    pub(super) body: Vec<u8>,
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

pub(super) fn response(status: u16, content_type: &'static str, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status,
        content_type,
        body,
    }
}

pub(super) fn json_response<T: Serialize>(status: u16, value: &T) -> HttpResponse {
    match serde_json::to_vec(value) {
        Ok(body) => response(status, "application/json; charset=utf-8", body),
        Err(error) => error_response(500, &error.to_string()),
    }
}

pub(super) fn error_response(status: u16, message: &str) -> HttpResponse {
    #[derive(Serialize)]
    struct ErrorBody<'a> {
        error: &'a str,
    }
    json_response(status, &ErrorBody { error: message })
}
