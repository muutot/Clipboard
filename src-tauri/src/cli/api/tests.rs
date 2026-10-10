//! Unit and loopback tests for the `api` module.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::domain::{ClipboardItem, ClipboardKind};
use crate::item_operations::CopyContext;
use crate::storage::{ClipboardRepository, Database};

use super::http::{
    authorize, constant_time_eq, handle_connection, read_request_with_deadline, serve, HttpRequest,
    ServeContext,
};
use super::routes::{dispatch, parse_query};
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
