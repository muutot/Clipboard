//! S3 transport tests, moved verbatim with the module.

use super::*;

const AKID: &str = "AKIDEXAMPLE";
const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
const EMPTY_PAYLOAD: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[test]
fn sigv4_matches_official_get_vanilla() {
    // AWS SigV4 test-suite vector "get-vanilla": GET / with no query.
    // Verified against the official suite (service = "service", 20150830T123600Z).
    let signer = SigV4::new(AKID, SECRET, "us-east-1", "service", 1440938160000);
    let auth = signer.sign(
        "GET",
        "/",
        "",
        &[
            ("host".to_string(), "example.amazonaws.com".to_string()),
            ("x-amz-date".to_string(), "20150830T123600Z".to_string()),
        ],
        EMPTY_PAYLOAD,
    );
    assert_eq!(
        auth,
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
    );
}

#[test]
fn sigv4_matches_official_get_vanilla_query() {
    // AWS SigV4 test-suite vector "get-vanilla-query-order-key".
    // Canonical query must order repeated keys by value (Value1 before value2).
    let signer = SigV4::new(AKID, SECRET, "us-east-1", "service", 1440938160000);
    let auth = signer.sign(
        "GET",
        "/",
        "Param1=Value1&Param1=value2",
        &[
            ("host".to_string(), "example.amazonaws.com".to_string()),
            ("x-amz-date".to_string(), "20150830T123600Z".to_string()),
        ],
        EMPTY_PAYLOAD,
    );
    assert_eq!(
        auth,
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=eedbc4e291e521cf13422ffca22be7d2eb8146eecf653089df300a15b2382bd1"
    );
}

#[test]
fn sigv4_matches_official_get_unreserved() {
    // Official vector "get-unreserved": URI stays raw (RFC 3986 unreserved chars).
    let signer = SigV4::new(AKID, SECRET, "us-east-1", "service", 1440938160000);
    let auth = signer.sign(
        "GET",
        "/-._~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        "",
        &[
            ("host".to_string(), "example.amazonaws.com".to_string()),
            ("x-amz-date".to_string(), "20150830T123600Z".to_string()),
        ],
        EMPTY_PAYLOAD,
    );
    assert_eq!(
        auth,
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=07ef7494c76fa4850883e2b006601f940f8a34d404d0cfa977f52a65bbf5f24f"
    );
}

#[test]
fn s3_endpoint_parses_scheme_and_host() {
    assert_eq!(
        parse_endpoint("s3.amazonaws.com"),
        ("https".to_string(), "s3.amazonaws.com".to_string())
    );
    assert_eq!(
        parse_endpoint("http://127.0.0.1:9000"),
        ("http".to_string(), "127.0.0.1:9000".to_string())
    );
    assert_eq!(
        parse_endpoint("https://minio.example.com"),
        ("https".to_string(), "minio.example.com".to_string())
    );
}

#[test]
fn list_query_uses_percent_encoded_prefix() {
    let q = build_list_query(Some("clipboard-backup/"), None, None);
    assert_eq!(q, "list-type=2&prefix=clipboard-backup%2F");
}

#[test]
fn paginated_list_query_is_canonical_and_uses_only_one_cursor() {
    assert_eq!(
        build_list_query(
            Some("v1/segments/device-a/"),
            Some("v1/segments/device-a/0002"),
            None,
        ),
        "list-type=2&prefix=v1%2Fsegments%2Fdevice-a%2F&start-after=v1%2Fsegments%2Fdevice-a%2F0002"
    );
    assert_eq!(
        build_list_query(
            Some("v1/segments/device-a/"),
            Some("ignored"),
            Some("next+/="),
        ),
        "continuation-token=next%2B%2F%3D&list-type=2&prefix=v1%2Fsegments%2Fdevice-a%2F"
    );
}

#[test]
fn content_md5_uses_the_s3_required_base64_encoding() {
    assert_eq!(content_md5_base64(b"hello"), "XUFAKrxLKna5cZ2REBfFkg==");
}

#[test]
fn head_metadata_reads_the_content_length_header() {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_LENGTH, HeaderValue::from_static("1048593"));

    assert_eq!(
        response_content_length_header(&headers).unwrap(),
        Some(1_048_593)
    );
}

#[test]
fn conditional_put_headers_are_included_in_the_signature() {
    let client = Client::new();
    let body = b"checkpoint";
    let req = S3Request {
        method: "PUT",
        scheme: "https",
        endpoint_host: "s3.example.test",
        bucket: "clipboard",
        key: "v1/checkpoint.bin",
        query: None,
        payload: Some(body),
        access_key: AKID,
        secret_key: SECRET,
        region: "us-east-1",
        extra_headers: &[("if-none-match", "*")],
    };
    let request = signed_request(&client, &req)
        .unwrap()
        .body(body.to_vec())
        .build()
        .unwrap();
    let authorization = request
        .headers()
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();

    assert!(
        authorization.contains("SignedHeaders=host;if-none-match;x-amz-content-sha256;x-amz-date")
    );
}

/// A `PUT` request with the credential and condition header under test.
fn put_request(
    access_key: &'static str,
    region: &'static str,
    extra_headers: &'static [(&'static str, &'static str)],
) -> S3Request<'static> {
    S3Request {
        method: "PUT",
        scheme: "https",
        endpoint_host: "s3.example.test",
        bucket: "clipboard",
        key: "v1/checkpoint.bin",
        query: None,
        payload: Some(b"checkpoint"),
        access_key,
        secret_key: SECRET,
        region,
        extra_headers,
    }
}

/// `HeaderValue::from_str` accepts non-ASCII as opaque octets, so an unsafe
/// credential silently produced an Authorization header the endpoint could
/// never match — sync failing forever with an opaque 403. A CR/LF in the
/// credential is rejected outright, and the old `unwrap` turned that into a
/// panic that killed the auto-sync worker thread for good.
#[test]
fn unsafe_signing_components_are_rejected_before_signing() {
    let client = Client::new();

    for (access_key, region) in [
        ("AKID\r\nx-evil: 1", "us-east-1"),
        ("\u{5bc6}\u{94a5}", "us-east-1"),
        ("", "us-east-1"),
        (AKID, "us-east-1\n"),
        (AKID, "\u{00e9}"),
        (AKID, ""),
    ] {
        let error = signed_request(&client, &put_request(access_key, region, &[]))
            .expect_err("unsafe credential must be reported");
        assert!(
            error.contains("access key") || error.contains("region"),
            "expected the offending field to be named, got {error:?}"
        );
    }
}

/// A hostile endpoint answering with a header-injection ETag must produce a
/// sync error, not a panic that takes down the sync worker.
/// Accepts one connection, consumes the request, and leaves the stream closed.
fn drain_request(stream: &mut std::net::TcpStream) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                buffer.extend_from_slice(&chunk[..read]);
                if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                    break end + 4;
                }
            }
        }
    };
    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
    let declared = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut remaining = declared.saturating_sub(buffer.len() - header_end);
    while remaining > 0 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => remaining = remaining.saturating_sub(read),
        }
    }
}

/// Serves `statuses` in order, then 200 forever, and counts the requests it
/// actually received. Returns the port and the counter.
fn spawn_scripted_server(
    statuses: Vec<u16>,
) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let port = listener.local_addr().expect("read the bound port").port();
    let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            drain_request(&mut stream);
            let index = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // The final status sticks, so a single-element list scripts an
            // unrelenting failure.
            let status = statuses
                .get(index)
                .or_else(|| statuses.last())
                .copied()
                .unwrap_or(200);
            let response = format!(
                "HTTP/1.1 {status} Scripted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (port, requests)
}

#[test]
fn cancellation_interrupts_stalled_headers_download_and_upload() {
    use crate::cancellation::CancellationToken;
    use std::{net::TcpListener, sync::mpsc, time::Instant};
    for mode in ["headers", "download", "upload"] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                if stream.read(&mut byte).unwrap() == 0 {
                    return;
                }
                request.push(byte[0]);
            }
            if mode == "download" {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8192\r\n\r\ndata")
                    .unwrap();
            }
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        let token = CancellationToken::default();
        let worker_token = token.clone();
        let dir = std::env::temp_dir().join(format!("clipboard-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let destination = dir.join("partial.bin");
        let worker_path = destination.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = worker_token.run(|| {
                if mode == "upload" {
                    put_s3_object(
                        &endpoint,
                        "region",
                        "bucket",
                        "object",
                        vec![7; 8 * 1024 * 1024],
                        "key",
                        "secret",
                        S3PutCondition::IfAbsent,
                    )
                    .map(|_| ())
                } else {
                    get_s3_object_to_file(
                        &endpoint,
                        "region",
                        "bucket",
                        "object",
                        &worker_path,
                        8192,
                        "key",
                        "secret",
                    )
                    .map(|_| ())
                }
            });
            done_tx.send(result).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        if mode == "download" {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !destination.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(destination.exists(), "exercise cleanup after creation");
        }
        token.cancel();
        let result = done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("network cancellation should not wait for the request timeout");
        assert!(result.unwrap_err().contains("sync cancelled"));
        assert!(!destination.exists(), "partial downloads must be removed");
        let _ = release_tx.send(());
        worker.join().unwrap();
        server.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn cancellation_interrupts_retry_backoff() {
    let token = crate::cancellation::CancellationToken::default();
    let cancel = token.clone();
    let trigger = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    assert_eq!(
        token
            .run(|| cancellation::sleep(Duration::from_secs(4)))
            .unwrap_err(),
        "sync cancelled"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    trigger.join().unwrap();
}

fn retry_test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build the test client")
}

/// The point of the whole change: one 503 used to abort a run, and this is
/// the end-to-end proof that a scripted failure is survived.
#[test]
fn a_transient_server_error_is_retried_until_it_succeeds() {
    let (port, requests) = spawn_scripted_server(vec![503, 500, 200]);
    let client = retry_test_client();
    let url = format!("http://127.0.0.1:{port}/bucket/key");

    let response =
        send_with_retry(|| Ok(client.get(&url)), "test").expect("the third attempt succeeds");
    assert!(response.status().is_success());
    assert_eq!(
        requests.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "both failures must have been retried"
    );
}

/// The budget is finite, and the caller still learns the real status.
#[test]
fn an_exhausted_retry_budget_reports_the_last_status() {
    let (port, requests) = spawn_scripted_server(vec![503]);
    let client = retry_test_client();
    let url = format!("http://127.0.0.1:{port}/bucket/key");

    let error = send_with_retry(|| Ok(client.get(&url)), "test")
        .expect_err("an unrelenting 503 must eventually surface");
    assert!(error.contains("503"), "the status must survive: {error}");
    assert_eq!(
        requests.load(std::sync::atomic::Ordering::SeqCst),
        MAX_SEND_ATTEMPTS as usize,
        "every allowed attempt must be used, and no more"
    );
}

/// A precondition failure is a decision, not a transient fault. Retrying it
/// would hide the ETag conflict the caller has to act on.
#[test]
fn a_precondition_failure_is_not_retried() {
    let (port, requests) = spawn_scripted_server(vec![412]);
    let client = retry_test_client();
    let url = format!("http://127.0.0.1:{port}/bucket/key");

    let response = send_with_retry(|| Ok(client.get(&url)), "test")
        .expect("a 412 is returned, not treated as an error");
    assert_eq!(response.status().as_u16(), 412);
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// Absence is a normal answer that callers branch on, so it is returned once.
#[test]
fn a_missing_object_is_not_retried() {
    let (port, requests) = spawn_scripted_server(vec![404]);
    let client = retry_test_client();
    let url = format!("http://127.0.0.1:{port}/bucket/key");

    let response = send_with_retry(|| Ok(client.get(&url)), "test").expect("404 is an answer");
    assert_eq!(response.status().as_u16(), 404);
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// A refused connection is the other retryable class, and it must not be
/// confused with a permanent failure.
#[test]
fn a_refused_connection_is_retried_then_reported() {
    // Bind and immediately drop, so the port is almost certainly free.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let client = retry_test_client();
    let url = format!("http://127.0.0.1:{port}/bucket/key");

    let error = send_with_retry(|| Ok(client.get(&url)), "test")
        .expect_err("nothing is listening, so every attempt must fail");
    assert!(
        error.contains("test failed"),
        "the label must be reported: {error}"
    );
}

/// `reqwest`'s `Body` needs a `'static` payload, so the retry path re-sends
/// through a `SharedBuffer` over the same `Arc`. If the second attempt read
/// from where the first stopped, every retry would upload a truncated
/// object — the failure mode this indirection exists to avoid.
#[test]
fn shared_buffer_rereads_its_payload_from_the_start() {
    let bytes = Arc::new(b"the quick brown fox".to_vec());
    let mut view = SharedBuffer {
        bytes: Arc::clone(&bytes),
        offset: 0,
    };

    let mut first = [0u8; 8];
    assert_eq!(view.read(&mut first).unwrap(), 8);
    assert_eq!(&first, b"the quic");
    // A short read must not lose the remainder.
    let mut tiny = [0u8; 4];
    assert_eq!(view.read(&mut tiny).unwrap(), 4);
    assert_eq!(&tiny, b"k br");

    // A fresh attempt over the same buffer replays the whole payload.
    let mut retry = SharedBuffer {
        bytes: Arc::clone(&bytes),
        offset: 0,
    };
    let mut replayed = Vec::new();
    retry.read_to_end(&mut replayed).unwrap();
    assert_eq!(replayed, *bytes);
    assert_eq!(retry.offset, bytes.len());

    // Draining the rest terminates rather than looping.
    let mut sink = Vec::new();
    view.read_to_end(&mut sink).unwrap();
    assert_eq!(&sink, b"own fox");
}

/// The retry set is the whole point of the change, and its exclusions are
/// load-bearing: retrying 412 would hide a precondition failure the caller
/// must see, and retrying 403 or 404 would only burn the backoff.
#[test]
fn only_transient_statuses_are_retried() {
    for status in [429, 500, 502, 503, 504] {
        assert!(is_retryable_status(status), "{status} must be retried");
    }
    for status in [
        200, 201, 204, 206, 301, 400, 401, 403, 404, 409, 412, 416, 501, 505,
    ] {
        assert!(
            !is_retryable_status(status),
            "{status} must reach the caller unchanged"
        );
    }
}

/// A `Retry-After` hint is honoured, capped, and must not make the backoff
/// unbounded. An HTTP-date form is ignored rather than misparsed as seconds.
#[test]
fn retry_after_hints_are_capped_and_validated() {
    let mut headers = HeaderMap::new();
    assert_eq!(retry_after_delay(&headers), None);

    headers.insert("retry-after", HeaderValue::from_static("1"));
    assert_eq!(retry_after_delay(&headers), Some(Duration::from_secs(1)));

    headers.insert("retry-after", HeaderValue::from_static("3600"));
    assert_eq!(
        retry_after_delay(&headers),
        Some(SEND_RETRY_MAX_DELAY),
        "a one-hour hint must be capped at the backoff ceiling"
    );

    headers.insert(
        "retry-after",
        HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT"),
    );
    assert_eq!(retry_after_delay(&headers), None);

    headers.insert("retry-after", HeaderValue::from_static("-5"));
    assert_eq!(retry_after_delay(&headers), None);
}

#[test]
fn a_malicious_etag_is_reported_instead_of_panicking() {
    let client = Client::new();
    let error = signed_request(
        &client,
        &put_request(AKID, "us-east-1", &[("if-match", "\"abc\"\r\nx-evil: 1")]),
    )
    .expect_err("a CRLF ETag must be reported");
    assert!(
        error.contains("if-match"),
        "expected the if-match header to be named, got {error:?}"
    );
}

/// An invalid header *name* is reported the same way, so adding a
/// caller-supplied header can never panic the worker either.
#[test]
fn an_invalid_header_name_is_reported_instead_of_panicking() {
    let client = Client::new();
    let error = signed_request(
        &client,
        &put_request(AKID, "us-east-1", &[("if match", "value")]),
    )
    .expect_err("a malformed header name must be reported");
    assert!(
        error.contains("header name"),
        "expected the invalid header name to be reported, got {error:?}"
    );
}

#[test]
fn prehashed_streaming_request_uses_the_supplied_payload_digest() {
    let client = Client::new();
    let digest = "a".repeat(64);
    let req = S3Request {
        method: "PUT",
        scheme: "https",
        endpoint_host: "s3.example.test",
        bucket: "clipboard",
        key: "v1/resources/file/sha256-a.bin",
        query: None,
        payload: None,
        access_key: AKID,
        secret_key: SECRET,
        region: "us-east-1",
        extra_headers: &[("if-none-match", "*")],
    };
    let request = signed_request_with_payload_hash(&client, &req, &digest)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        request
            .headers()
            .get("x-amz-content-sha256")
            .unwrap()
            .to_str()
            .unwrap(),
        digest
    );
}

#[test]
fn streaming_payload_digest_must_be_canonical_sha256() {
    assert!(validate_payload_sha256(&"a".repeat(64)).is_ok());
    assert!(validate_payload_sha256(&"A".repeat(64)).is_err());
    assert!(validate_payload_sha256("abc").is_err());
}

#[test]
fn streaming_timeout_scales_with_size_and_is_bounded() {
    assert_eq!(streaming_timeout(0), Duration::from_secs(60));
    assert_eq!(streaming_timeout(64 * 1024), Duration::from_secs(61));
    assert_eq!(streaming_timeout(u64::MAX), Duration::from_secs(30 * 60));
}

#[test]
fn request_metrics_are_shared_and_resettable() {
    let metrics = S3RequestMetrics::default();
    let shared = metrics.clone();
    metrics.record_put(11, Duration::from_nanos(13));
    shared.record_get(17, Duration::from_nanos(19));
    metrics.record_head(Duration::from_nanos(23));
    metrics.record_list_page(29);
    metrics.record_list_elapsed(Duration::from_nanos(31));
    metrics.record_delete(Duration::from_nanos(37));

    assert_eq!(
        shared.snapshot(),
        S3RequestMetricsSnapshot {
            put_requests: 1,
            get_requests: 1,
            head_requests: 1,
            list_requests: 1,
            delete_requests: 1,
            uploaded_bytes: 11,
            downloaded_bytes: 46,
            put_elapsed_ns: 13,
            get_elapsed_ns: 19,
            head_elapsed_ns: 23,
            list_elapsed_ns: 31,
            delete_elapsed_ns: 37,
        }
    );

    shared.reset();
    assert_eq!(metrics.snapshot(), S3RequestMetricsSnapshot::default());
}

#[test]
#[ignore = "requires an explicitly configured disposable S3-compatible server"]
fn disposable_s3_round_trip_and_conditional_writes() {
    let endpoint = std::env::var("CLIPBOARD_S3_TEST_ENDPOINT")
        .expect("CLIPBOARD_S3_TEST_ENDPOINT must be set");
    let region =
        std::env::var("CLIPBOARD_S3_TEST_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    let bucket =
        std::env::var("CLIPBOARD_S3_TEST_BUCKET").expect("CLIPBOARD_S3_TEST_BUCKET must be set");
    let access_key = std::env::var("CLIPBOARD_S3_TEST_ACCESS_KEY")
        .expect("CLIPBOARD_S3_TEST_ACCESS_KEY must be set");
    let secret_key = std::env::var("CLIPBOARD_S3_TEST_SECRET_KEY")
        .expect("CLIPBOARD_S3_TEST_SECRET_KEY must be set");

    ensure_test_bucket(&endpoint, &region, &bucket, &access_key, &secret_key).unwrap();

    let prefix = format!("transport-test-{}/", uuid::Uuid::new_v4());
    let key = format!("{prefix}object.bin");
    let first = put_s3_object(
        &endpoint,
        &region,
        &bucket,
        &key,
        b"first".to_vec(),
        &access_key,
        &secret_key,
        S3PutCondition::IfAbsent,
    )
    .unwrap();
    let S3PutOutcome::Stored { etag: first_etag } = first else {
        panic!("first conditional PUT unexpectedly lost its precondition");
    };
    let first_etag = first_etag.expect("RustFS-compatible server must return an ETag");

    assert_eq!(
        put_s3_object(
            &endpoint,
            &region,
            &bucket,
            &key,
            b"duplicate".to_vec(),
            &access_key,
            &secret_key,
            S3PutCondition::IfAbsent,
        )
        .unwrap(),
        S3PutOutcome::PreconditionFailed
    );

    let downloaded = get_s3_object(&endpoint, &region, &bucket, &key, &access_key, &secret_key)
        .unwrap()
        .unwrap();
    assert_eq!(downloaded.bytes, b"first");
    assert_eq!(downloaded.etag.as_deref(), Some(first_etag.as_str()));

    let updated = put_s3_object(
        &endpoint,
        &region,
        &bucket,
        &key,
        b"second".to_vec(),
        &access_key,
        &secret_key,
        S3PutCondition::IfMatch(first_etag.clone()),
    )
    .unwrap();
    let S3PutOutcome::Stored { etag: updated_etag } = updated else {
        panic!("conditional update unexpectedly lost its precondition");
    };
    let updated_etag = updated_etag.expect("S3-compatible server must return an ETag");
    assert_eq!(
        put_s3_object(
            &endpoint,
            &region,
            &bucket,
            &key,
            b"stale".to_vec(),
            &access_key,
            &secret_key,
            S3PutCondition::IfMatch(first_etag),
        )
        .unwrap(),
        S3PutOutcome::PreconditionFailed
    );

    let listed = list_s3_objects(
        &endpoint,
        &region,
        &bucket,
        Some(&prefix),
        &access_key,
        &secret_key,
    )
    .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].object_key, key);
    assert_eq!(listed[0].etag.as_deref(), Some(updated_etag.as_str()));

    let streamed_key = format!("{prefix}streamed.bin");
    let streamed_source = std::env::temp_dir().join(format!(
        "clipboard-s3-stream-source-{}-{}.bin",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let streamed_destination = std::env::temp_dir().join(format!(
        "clipboard-s3-stream-destination-{}-{}.bin",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let streamed_bytes = vec![0x5a; 1024 * 1024 + 17];
    std::fs::write(&streamed_source, &streamed_bytes).unwrap();
    let streamed_sha256 = hex::encode(Sha256::digest(&streamed_bytes));
    assert!(matches!(
        put_s3_file(
            &endpoint,
            &region,
            &bucket,
            &streamed_key,
            &streamed_source,
            &streamed_sha256,
            streamed_bytes.len() as u64,
            &access_key,
            &secret_key,
            S3PutCondition::IfAbsent,
        )
        .unwrap(),
        S3PutOutcome::Stored { .. }
    ));
    let streamed_head = head_s3_object(
        &endpoint,
        &region,
        &bucket,
        &streamed_key,
        &access_key,
        &secret_key,
    )
    .unwrap()
    .unwrap();
    assert_eq!(streamed_head.size_bytes, Some(streamed_bytes.len() as u64));
    let streamed_download = get_s3_object_to_file(
        &endpoint,
        &region,
        &bucket,
        &streamed_key,
        &streamed_destination,
        2 * 1024 * 1024,
        &access_key,
        &secret_key,
    )
    .unwrap()
    .unwrap();
    assert_eq!(streamed_download.sha256, streamed_sha256);
    assert_eq!(streamed_download.size_bytes, streamed_bytes.len() as u64);
    assert_eq!(
        std::fs::read(&streamed_destination).unwrap(),
        streamed_bytes
    );

    delete_from_s3(&endpoint, &region, &bucket, &key, &access_key, &secret_key).unwrap();
    delete_from_s3(
        &endpoint,
        &region,
        &bucket,
        &streamed_key,
        &access_key,
        &secret_key,
    )
    .unwrap();
    let _ = std::fs::remove_file(streamed_source);
    let _ = std::fs::remove_file(streamed_destination);
    assert!(
        get_s3_object(&endpoint, &region, &bucket, &key, &access_key, &secret_key,)
            .unwrap()
            .is_none()
    );
}

#[test]
fn list_page_exposes_continuation_and_decodes_object_names() {
    let page = parse_s3_list_page(
        r#"<ListBucketResult>
            <IsTruncated>true</IsTruncated>
            <NextContinuationToken>next&amp;token</NextContinuationToken>
            <Contents>
                <Key>v1/heads/device&amp;a.bin</Key>
                <LastModified>2026-08-10T12:00:00Z</LastModified>
                <ETag>&quot;abc123&quot;</ETag>
                <Size>42</Size>
            </Contents>
        </ListBucketResult>"#,
    );
    assert!(page.is_truncated);
    assert_eq!(page.next_continuation_token.as_deref(), Some("next&token"));
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].object_key, "v1/heads/device&a.bin");
    assert_eq!(page.entries[0].name, "device&a.bin");
    assert_eq!(page.entries[0].size_bytes, Some(42));
    assert_eq!(page.entries[0].etag.as_deref(), Some("\"abc123\""));
}

#[test]
fn xml_text_decodes_named_and_numeric_entities() {
    assert_eq!(
        decode_xml_text("&quot;&#34;&#x22;&amp;&lt;&gt;&apos;"),
        "\"\"\"&<>'"
    );
    assert_eq!(
        decode_xml_text("keep-&unknown;-literal"),
        "keep-&unknown;-literal"
    );
}
