//! Unit and loopback tests for the `api` module.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::domain::{ClipboardItem, ClipboardKind};
use crate::item_operations::CopyContext;
use crate::state::CaptureState;
use crate::storage::{ClipboardRepository, Database};

use super::http::{
    authorize, constant_time_eq, handle_connection, read_request_with_deadline, serve, HttpRequest,
    ServeContext,
};
use super::routes::{dispatch, dispatch_with_context, parse_query};
use super::LocalApiServer;

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
fn search_accepts_form_encoded_spaces_and_keeps_escaped_plus() {
    let database = Database::open_in_memory().unwrap();
    for (id, text) in [
        ("spaced", "two words"),
        ("pluses", "C++ tutorial"),
        ("unicode", "中文 记录"),
    ] {
        let mut item = super::super::build_text_clipboard_item(
            text.into(),
            ClipboardKind::Text,
            "query",
            "test",
        );
        item.id = id.into();
        database.save_item(&item).unwrap();
    }
    for (query, expected) in [
        ("two+words", "spaced"),
        ("two%20words", "spaced"),
        ("C%2B%2B+tutorial", "pluses"),
        ("%E4%B8%AD%E6%96%87+%E8%AE%B0%E5%BD%95", "unicode"),
    ] {
        let response = dispatch(
            &request("GET", &format!("/search?q={query}"), b""),
            &database,
            500,
            500,
        );
        assert_eq!(response.status, 200);
        let found: Vec<ClipboardItem> = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(found.len(), 1, "query {query} should find the saved text");
        assert_eq!(found[0].id, expected);
    }
}

#[test]
fn path_plus_stays_literal_when_query_spaces_are_decoded() {
    for path in ["/items/record+id", "/items/record%2Bid"] {
        let database = Database::open_in_memory().unwrap();
        let mut item = super::super::build_text_clipboard_item(
            "path plus".into(),
            ClipboardKind::Text,
            "query",
            "test",
        );
        item.id = "record+id".into();
        database.save_item(&item).unwrap();
        let response = dispatch(&request("DELETE", path, b""), &database, 500, 500);
        assert_eq!(response.status, 200);
        assert_eq!(database.item_count().unwrap(), 0);
    }
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
fn paste_returns_the_actual_deduplicated_record() {
    for text in ["existing API note", "https://example.com/existing"] {
        let database = Database::open_in_memory().unwrap();
        let kind = if text.starts_with("https://") {
            ClipboardKind::Link
        } else {
            ClipboardKind::Text
        };
        let mut original =
            super::super::build_text_clipboard_item(text.into(), kind, "existing", "original app");
        original.id = "original-id".into();
        original.created_at_ms = 100;
        original.last_used_at_ms = Some(100);
        original.title = "User-authored title".into();
        original.is_favorite = true;
        original.html_content = Some("<p>retained formatting</p>".into());
        original.rtf_content = Some("{\\rtf1 retained formatting}".into());
        original.icon_path = Some("retained-icon.png".into());
        original.metadata_json = Some(
            serde_json::json!({
                "customTitle": true,
                "tags": ["manual"],
                "codeLanguage": "Rust",
                "web": {"url": "https://example.com/old"}
            })
            .to_string(),
        );
        database.save_item(&original).unwrap();

        let response = dispatch(
            &request("POST", "/paste", text.as_bytes()),
            &database,
            500,
            500,
        );
        assert_eq!(response.status, 201);
        let returned: ClipboardItem = serde_json::from_slice(&response.body).unwrap();
        let stored = database.get_item(&original.id).unwrap().unwrap();
        assert_eq!(database.item_count().unwrap(), 1);
        assert!(stored.is_favorite);
        assert_eq!(stored.title, original.title);
        assert_eq!(stored.created_at_ms, 100);
        assert_eq!(stored.source_app.as_deref(), Some("Local API"));
        assert_eq!(
            returned, stored,
            "API response must describe the row it saved"
        );
    }
}

fn gated_capture_state(paused: bool, text_limit: u64) -> CaptureState {
    let capture = CaptureState::new(
        &crate::privacy::PrivacyManager::new(),
        Vec::new(),
        1024 * 1024,
        text_limit,
    );
    if paused {
        capture.set_paused(true);
    }
    capture
}

#[test]
fn gated_paste_endpoint_refuses_text_matching_a_sensitive_pattern() {
    let database = Database::open_in_memory().unwrap();
    let mut privacy = crate::privacy::PrivacyManager::new();
    privacy.sensitive_patterns = vec![regex_lite::Regex::new("secret").unwrap()];
    let capture = CaptureState::new(&privacy, Vec::new(), 1024 * 1024, 500_000);
    let paste = request("POST", "/paste", br#"{"text":"the secret is out"}"#);

    let response = dispatch_with_context(
        &paste,
        &database,
        500,
        500,
        &CopyContext::default(),
        Some(&capture),
    );

    assert_eq!(response.status, 403);
    assert_eq!(database.item_count().unwrap(), 0);
}

#[test]
fn gated_paste_endpoint_refuses_text_above_the_capture_ceiling() {
    let database = Database::open_in_memory().unwrap();
    let capture = gated_capture_state(false, 8);
    let paste = request("POST", "/paste", br#"{"text":"more than eight"}"#);

    let response = dispatch_with_context(
        &paste,
        &database,
        500,
        500,
        &CopyContext::default(),
        Some(&capture),
    );

    assert_eq!(response.status, 413);
    assert_eq!(database.item_count().unwrap(), 0);
}

#[test]
fn gated_paste_endpoint_refuses_everything_while_capture_is_paused() {
    let database = Database::open_in_memory().unwrap();
    let capture = gated_capture_state(true, 500_000);
    let paste = request("POST", "/paste", br#"{"text":"plain text"}"#);

    let response = dispatch_with_context(
        &paste,
        &database,
        500,
        500,
        &CopyContext::default(),
        Some(&capture),
    );

    assert_eq!(response.status, 403);
    assert_eq!(database.item_count().unwrap(), 0);
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

fn run_framed_paste(headers: &str, body: &str) -> (Result<u16, String>, Vec<ClipboardItem>, usize) {
    run_framed_paste_bytes(headers, body.as_bytes())
}

fn run_framed_paste_bytes(
    headers: &str,
    body: &[u8],
) -> (Result<u16, String>, Vec<ClipboardItem>, usize) {
    let database = Database::open_in_memory().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(client, "POST /paste HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer test\r\n{headers}\r\n").unwrap();
    client.write_all(body).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let changes = AtomicUsize::new(0);
    let result = handle_connection(
        stream,
        &database,
        500,
        500,
        "test",
        port,
        &CopyContext::default(),
        Some(&gated_capture_state(false, 500_000)),
        &|| {
            changes.fetch_add(1, Ordering::SeqCst);
        },
    );
    let result = result.map(|()| {
        let mut reply = BufReader::new(client);
        let mut line = String::new();
        reply.read_line(&mut line).unwrap();
        let status = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        let mut length = None;
        loop {
            line.clear();
            assert!(reply.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let mut body = vec![0; length.unwrap()];
        reply.read_exact(&mut body).unwrap();
        let _: serde_json::Value = serde_json::from_slice(&body).unwrap();
        status
    });
    let items = database
        .list_recent(10, 0, &crate::storage::HistoryFilter::default())
        .unwrap();
    (result, items, changes.load(Ordering::SeqCst))
}

#[test]
fn request_framing_rejects_duplicate_lengths_before_paste_mutates() {
    for headers in [
        "Content-Length: 6\r\nContent-Length: 3\r\n",
        "Content-Length: 3\r\ncOnTeNt-LeNgTh: 6\r\n",
        "Content-Length: 6\r\nContent-Length: 6\r\n",
    ] {
        let (result, items, changes) = run_framed_paste(headers, "sample");
        assert!(
            items.is_empty(),
            "ambiguous framing saved {:?} with status {result:?}",
            items[0].text_content
        );
        assert!(
            result.is_err(),
            "ambiguous framing was accepted: {headers:?}"
        );
        assert_eq!(changes, 0);
    }
}

#[test]
fn request_framing_rejects_transfer_encoding_before_paste_mutates() {
    for headers in [
        "Transfer-Encoding: chunked\r\nContent-Length: 3\r\n",
        "Transfer-Encoding: chunked\r\n",
        "transfer-encoding: identity\r\nContent-Length: 6\r\n",
    ] {
        let (result, items, changes) = run_framed_paste(headers, "6\r\nsample\r\n0\r\n\r\n");
        assert!(
            items.is_empty(),
            "encoded framing saved {:?} with status {result:?}",
            items[0].text_content
        );
        assert!(
            result.is_err(),
            "unsupported framing was accepted: {headers:?}"
        );
        assert_eq!(changes, 0);
    }
}

#[test]
fn request_framing_rejects_invalid_lengths_and_malformed_headers() {
    for headers in [
        "Content-Length: +6\r\n",
        "Content-Length: -1\r\n",
        "Content-Length: invalid\r\n",
        "Content-Length: \r\n",
        "Content-Length: 999999999999999999999999999999999\r\n",
        "Content-Length: 6, 6\r\n",
        "Content-Length : 6\r\n",
        "Content-Length 6\r\n",
    ] {
        let (result, items, changes) = run_framed_paste(headers, "sample");
        assert!(
            result.is_err(),
            "malformed framing was accepted: {headers:?}"
        );
        assert!(items.is_empty());
        assert_eq!(changes, 0);
    }
}

#[test]
fn request_framing_keeps_valid_lengths_and_body_bytes() {
    for body in [
        "\n  中文 note\r\n".to_owned(),
        "中文 payload\r\n".repeat(1000),
    ] {
        let (result, items, changes) =
            run_framed_paste(&format!("cOnTeNt-LeNgTh: {}\r\n", body.len()), &body);
        assert_eq!(result.unwrap(), 201);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text_content.as_deref(), Some(body.as_str()));
        assert_eq!(changes, 1);
    }

    // Zero-length and absent-length requests still reach the endpoint's empty-body rejection.
    for headers in ["Content-Length: 0\r\n", ""] {
        let (result, items, changes) = run_framed_paste(headers, "");
        assert_eq!(result.unwrap(), 400);
        assert!(items.is_empty());
        assert_eq!(changes, 0);
    }
}

#[test]
fn request_framing_rejects_truncated_body_before_paste_mutates() {
    let (result, items, changes) = run_framed_paste("Content-Length: 10\r\n", "sample");
    assert!(result.unwrap_err().contains("before sending the body"));
    assert!(items.is_empty());
    assert_eq!(changes, 0);
}

#[test]
fn paste_encoding_rejects_invalid_utf8_before_mutating() {
    for body in [
        b"caf\xe9".as_slice(),
        b"incomplete \xe4\xb8".as_slice(),
        b"{\"text\":\"caf\xe9\"}".as_slice(),
    ] {
        let (result, items, changes) =
            run_framed_paste_bytes(&format!("Content-Length: {}\r\n", body.len()), body);
        assert!(
            items.is_empty(),
            "invalid UTF-8 was saved with status {result:?}"
        );
        assert_eq!(result.unwrap(), 400);
        assert_eq!(changes, 0);
    }
}

#[test]
fn paste_encoding_preserves_valid_unicode_in_raw_and_json_bodies() {
    let text = "\n café 中文 \u{fffd}\r\n";
    for body in [
        text.to_owned(),
        serde_json::json!({"text": text}).to_string(),
    ] {
        let (result, items, changes) =
            run_framed_paste(&format!("Content-Length: {}\r\n", body.len()), &body);
        assert_eq!(result.unwrap(), 201);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text_content.as_deref(), Some(text));
        assert_eq!(changes, 1);
    }
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
    for (method, path, body, expected_status, expected) in [
        ("GET", "/items", "", 200, 0),
        ("DELETE", "/items/absent", "", 404, 0),
        ("POST", "/paste", "sample", 201, 1),
    ] {
        let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(socket, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer test\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        // Content-Length frames a complete HTTP reply. Windows may reset the
        // connection after those bytes arrive; waiting for EOF tests socket
        // teardown rather than whether the mutation produced a valid reply.
        let mut response = BufReader::new(socket);
        let mut line = String::new();
        response.read_line(&mut line).unwrap();
        assert!(line.starts_with(&format!("HTTP/1.1 {expected_status} ")));
        let mut content_length = None;
        loop {
            line.clear();
            assert!(response.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let mut body = vec![0; content_length.expect("reply must declare its body length")];
        response.read_exact(&mut body).unwrap();
        let _: serde_json::Value = serde_json::from_slice(&body).unwrap();
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
                    None,
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
fn stop_closes_accepted_clients_before_they_can_submit_a_write() {
    let database = Arc::new(Database::open_in_memory().unwrap());
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let active = Arc::new(AtomicUsize::new(0));
    let (stop_tx, stop_rx) = mpsc::channel();
    let context = ServeContext {
        database: Arc::clone(&database),
        page_size_limit: 500,
        search_page_size_limit: 500,
        token: Arc::new("fixture-token".into()),
        port,
        active_connections: Arc::clone(&active),
        copy_context: CopyContext::default(),
        capture: None,
        on_change: Arc::new(|| {}),
    };
    let server = thread::spawn(move || serve(listener, stop_rx, context));
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let body = br#"{"text":"late write"}"#;
    client.write_all(format!(
        "POST /paste HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer fixture-token\r\nContent-Length: {}\r\n\r\n",
        body.len(),
    ).as_bytes()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while active.load(Ordering::SeqCst) != 1 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    stop_tx.send(()).unwrap();
    server.join().unwrap();
    let _ = client.write_all(body);
    let mut response = Vec::new();
    let _ = client.read_to_end(&mut response);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(database.item_count().unwrap(), 0);
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

#[test]
fn api_paste_blocks_default_structured_credentials_before_storage() {
    let database = Database::open_in_memory().unwrap();
    let capture = gated_capture_state(false, 500_000);
    for text in [
        "PASSWORD=synthetic",
        r#"{"api_key":"synthetic"}"#,
        "-----BEGIN ENCRYPTED PRIVATE KEY-----",
    ] {
        let body = serde_json::to_vec(&serde_json::json!({"text": text})).unwrap();
        let paste = request("POST", "/paste", &body);
        let response = dispatch_with_context(
            &paste,
            &database,
            500,
            500,
            &CopyContext::default(),
            Some(&capture),
        );
        assert_eq!(response.status, 403);
        assert_eq!(database.item_count().unwrap(), 0);
        assert_eq!(database.count_sync_outbox().unwrap(), 0);
    }
}
