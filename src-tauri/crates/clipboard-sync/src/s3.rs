use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use crate::{cancellation, transport};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, ETAG};
use reqwest::{Client, RequestBuilder};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Entry {
    /// Complete bucket-relative key used by nested v1 namespaces.
    #[serde(skip_serializing)]
    pub object_key: String,
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: Option<u64>,
    pub modified_ms: Option<i64>,
    /// Raw ETag text from ListObjectsV2, including quotes when supplied.
    #[serde(skip_serializing)]
    pub etag: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct S3TestResult {
    pub success: bool,
    pub message: String,
    pub status_code: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3DownloadedObject {
    pub bytes: Vec<u8>,
    /// Raw HTTP ETag value, including quotes when supplied by the server.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3ObjectMetadata {
    pub size_bytes: Option<u64>,
    /// Raw HTTP ETag value, including quotes when supplied by the server.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3DownloadedFile {
    pub size_bytes: u64,
    pub sha256: String,
    /// Raw HTTP ETag value, including quotes when supplied by the server.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum S3PutCondition {
    #[default]
    Unconditional,
    IfAbsent,
    /// Raw HTTP ETag value returned by a previous GET/PUT.
    IfMatch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3PutOutcome {
    Stored { etag: Option<String> },
    PreconditionFailed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct S3RequestMetricsSnapshot {
    pub put_requests: u64,
    pub get_requests: u64,
    pub head_requests: u64,
    pub list_requests: u64,
    pub delete_requests: u64,
    pub uploaded_bytes: u64,
    pub downloaded_bytes: u64,
    pub put_elapsed_ns: u64,
    pub get_elapsed_ns: u64,
    pub head_elapsed_ns: u64,
    pub list_elapsed_ns: u64,
    pub delete_elapsed_ns: u64,
}

#[derive(Debug, Default)]
struct S3RequestMetricsInner {
    put_requests: AtomicU64,
    get_requests: AtomicU64,
    head_requests: AtomicU64,
    list_requests: AtomicU64,
    delete_requests: AtomicU64,
    uploaded_bytes: AtomicU64,
    downloaded_bytes: AtomicU64,
    put_elapsed_ns: AtomicU64,
    get_elapsed_ns: AtomicU64,
    head_elapsed_ns: AtomicU64,
    list_elapsed_ns: AtomicU64,
    delete_elapsed_ns: AtomicU64,
}

/// Optional transport diagnostics for benchmarks and runtime observability.
/// Clones share the same atomics. A normal `S3ObjectStore` does not allocate or
/// update these counters unless metrics are explicitly attached.
#[derive(Debug, Clone, Default)]
pub struct S3RequestMetrics {
    inner: Arc<S3RequestMetricsInner>,
}

impl S3RequestMetrics {
    pub fn snapshot(&self) -> S3RequestMetricsSnapshot {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        S3RequestMetricsSnapshot {
            put_requests: load(&self.inner.put_requests),
            get_requests: load(&self.inner.get_requests),
            head_requests: load(&self.inner.head_requests),
            list_requests: load(&self.inner.list_requests),
            delete_requests: load(&self.inner.delete_requests),
            uploaded_bytes: load(&self.inner.uploaded_bytes),
            downloaded_bytes: load(&self.inner.downloaded_bytes),
            put_elapsed_ns: load(&self.inner.put_elapsed_ns),
            get_elapsed_ns: load(&self.inner.get_elapsed_ns),
            head_elapsed_ns: load(&self.inner.head_elapsed_ns),
            list_elapsed_ns: load(&self.inner.list_elapsed_ns),
            delete_elapsed_ns: load(&self.inner.delete_elapsed_ns),
        }
    }

    pub fn reset(&self) {
        for counter in [
            &self.inner.put_requests,
            &self.inner.get_requests,
            &self.inner.head_requests,
            &self.inner.list_requests,
            &self.inner.delete_requests,
            &self.inner.uploaded_bytes,
            &self.inner.downloaded_bytes,
            &self.inner.put_elapsed_ns,
            &self.inner.get_elapsed_ns,
            &self.inner.head_elapsed_ns,
            &self.inner.list_elapsed_ns,
            &self.inner.delete_elapsed_ns,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_list_page(&self, downloaded_bytes: u64) {
        self.inner.list_requests.fetch_add(1, Ordering::Relaxed);
        self.inner
            .downloaded_bytes
            .fetch_add(downloaded_bytes, Ordering::Relaxed);
    }

    pub(crate) fn record_list_elapsed(&self, elapsed: Duration) {
        add_duration(&self.inner.list_elapsed_ns, elapsed);
    }

    pub(crate) fn record_get(&self, downloaded_bytes: u64, elapsed: Duration) {
        self.inner.get_requests.fetch_add(1, Ordering::Relaxed);
        self.inner
            .downloaded_bytes
            .fetch_add(downloaded_bytes, Ordering::Relaxed);
        add_duration(&self.inner.get_elapsed_ns, elapsed);
    }

    pub(crate) fn record_head(&self, elapsed: Duration) {
        self.inner.head_requests.fetch_add(1, Ordering::Relaxed);
        add_duration(&self.inner.head_elapsed_ns, elapsed);
    }

    pub(crate) fn record_put(&self, uploaded_bytes: u64, elapsed: Duration) {
        self.inner.put_requests.fetch_add(1, Ordering::Relaxed);
        self.inner
            .uploaded_bytes
            .fetch_add(uploaded_bytes, Ordering::Relaxed);
        add_duration(&self.inner.put_elapsed_ns, elapsed);
    }

    pub(crate) fn record_delete(&self, elapsed: Duration) {
        self.inner.delete_requests.fetch_add(1, Ordering::Relaxed);
        add_duration(&self.inner.delete_elapsed_ns, elapsed);
    }
}

fn add_duration(counter: &AtomicU64, elapsed: Duration) {
    counter.fetch_add(
        elapsed.as_nanos().min(u64::MAX as u128) as u64,
        Ordering::Relaxed,
    );
}

/// A shared request-signing client. Rebuilt only once, then reused so every
/// S3 call does not pay connection/header overhead.
fn shared_client() -> Result<Client, String> {
    static CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
    Ok(CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("failed to build S3 client")
        })
        .clone())
}

fn test_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("failed to build S3 test client: {error}"))
}

/// Parses the user-provided endpoint into (scheme, host).
/// Accepts `s3.amazonaws.com`, `https://s3.amazonaws.com`, `localhost:9000`,
/// `http://127.0.0.1:9000`. Defaults to https when no scheme is present.
fn parse_endpoint(endpoint: &str) -> (String, String) {
    let endpoint = endpoint.trim();
    if let Some(rest) = endpoint.strip_prefix("https://") {
        ("https".to_string(), rest.to_string())
    } else if let Some(rest) = endpoint.strip_prefix("http://") {
        ("http".to_string(), rest.to_string())
    } else {
        ("https".to_string(), endpoint.to_string())
    }
}

/// AWS SigV4 signing. Computes the canonical request, string-to-sign, signing
/// key chain, and the Authorization header for an S3 request.
struct SigV4<'a> {
    access_key: &'a str,
    secret_key: &'a str,
    region: &'a str,
    service: &'a str,
    amz_date: String,
    date_stamp: String,
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, KeyInit, Mac};
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("hmac accepts any key len");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

impl<'a> SigV4<'a> {
    fn new(
        access_key: &'a str,
        secret_key: &'a str,
        region: &'a str,
        service: &'a str,
        now_ms: i64,
    ) -> Self {
        let dt = chrono::DateTime::from_timestamp(now_ms / 1000, 0).unwrap_or_default();
        SigV4 {
            access_key,
            secret_key,
            region,
            service,
            amz_date: dt.format("%Y%m%dT%H%M%SZ").to_string(),
            date_stamp: dt.format("%Y%m%d").to_string(),
        }
    }

    fn sign(
        &self,
        method: &str,
        canonical_uri: &str,
        canonical_query: &str,
        canonical_headers: &[(String, String)],
        payload_hash: &str,
    ) -> String {
        // Sort header entries by name.
        let mut headers: Vec<(String, String)> = canonical_headers.to_vec();
        headers.sort_by(|a, b| a.0.cmp(&b.0));

        let mut canonical_headers_str = String::new();
        let mut signed_headers = Vec::new();
        for (name, value) in &headers {
            canonical_headers_str.push_str(&format!("{name}:{}\n", value.trim()));
            signed_headers.push(name.clone());
        }
        let signed_headers = signed_headers.join(";");
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers_str}\n{signed_headers}\n{payload_hash}"
        );

        let algorithm = "AWS4-HMAC-SHA256";
        let scope = format!(
            "{}/{}/{}/aws4_request",
            self.date_stamp, self.region, self.service
        );
        let string_to_sign = format!(
            "{algorithm}\n{}\n{scope}\n{}",
            self.amz_date,
            sha256_hex(canonical_request.as_bytes())
        );

        let k_date = hmac_sha256(
            format!("AWS4{}", self.secret_key).as_bytes(),
            self.date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hmac_sha256(&k_region, self.service.as_bytes());
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        format!(
            "{algorithm} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        )
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Builds the request URL. Chooses path style for custom/minIO-style endpoints
/// and legacy path style for `s3.amazonaws.com`-style curly-FQDN endpoints.
fn s3_url(
    scheme: &str,
    endpoint_host: &str,
    bucket: &str,
    key: &str,
    query: Option<String>,
) -> String {
    let mut url = format!("{scheme}://{endpoint_host}/{bucket}/{key}");
    if let Some(q) = query {
        url.push('?');
        url.push_str(&q);
    }
    url
}

/// Parameters for an S3 request to sign and send.
struct S3Request<'a> {
    method: &'a str,
    scheme: &'a str,
    endpoint_host: &'a str,
    bucket: &'a str,
    key: &'a str,
    query: Option<String>,
    payload: Option<&'a [u8]>,
    access_key: &'a str,
    secret_key: &'a str,
    region: &'a str,
    extra_headers: &'a [(&'a str, &'a str)],
}

/// Signs a request and returns a `RequestBuilder`.
fn signed_request(client: &reqwest::Client, req: &S3Request) -> Result<RequestBuilder, String> {
    let data = req.payload.unwrap_or(&[]);
    let payload_hash = sha256_hex(data);
    signed_request_with_payload_hash(client, req, &payload_hash)
}

fn signed_request_with_payload_hash(
    client: &reqwest::Client,
    req: &S3Request,
    payload_hash: &str,
) -> Result<RequestBuilder, String> {
    // The access key and the region are embedded verbatim in the `Authorization`
    // header and the credential scope, so both must be header-safe before we
    // sign. `HeaderValue::from_str` happily accepts non-ASCII as opaque bytes,
    // which silently produces a signature the endpoint cannot match; a control
    // character is rejected outright, and the old `unwrap` turned that rejection
    // into a panic that killed the auto-sync worker thread for good.
    validate_signing_component("access key", req.access_key)?;
    validate_signing_component("region", req.region)?;

    let signer = SigV4::new(req.access_key, req.secret_key, req.region, "s3", now_ms());

    let url = s3_url(
        req.scheme,
        req.endpoint_host,
        req.bucket,
        req.key,
        req.query.clone(),
    );
    let url_parsed = reqwest::Url::parse(&url).map_err(|e| format!("invalid URL: {e}"))?;
    let host = url_parsed
        .host_str()
        .ok_or_else(|| "endpoint host is empty".to_string())?;
    let host_header = match url_parsed.port() {
        Some(p) if p != 443 && p != 80 => format!("{host}:{p}"),
        _ => host.to_string(),
    };

    // Build canonical URI/query. For S3 the canonical path is the URL path
    // (already percent-free) and the query is the raw query string.
    let canonical_uri = url_parsed.path().to_string();
    let canonical_query = req.query.clone().unwrap_or_default();

    // Headers to sign, always including host, x-amz-date, x-amz-content-sha256.
    let mut headers: Vec<(String, String)> = vec![
        ("host".to_string(), host_header.clone()),
        ("x-amz-date".to_string(), signer.amz_date.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
    ];
    for (name, value) in req.extra_headers {
        headers.push((name.to_string(), value.to_string()));
    }

    let authorization = signer.sign(
        req.method,
        &canonical_uri,
        &canonical_query,
        &headers,
        payload_hash,
    );

    let mut header_map = HeaderMap::new();
    insert_header(&mut header_map, "x-amz-date", &signer.amz_date)?;
    insert_header(&mut header_map, "x-amz-content-sha256", payload_hash)?;
    for (name, value) in req.extra_headers {
        insert_header(&mut header_map, name, value)?;
    }
    insert_header(&mut header_map, "authorization", &authorization)?;

    let req_builder = match req.method {
        "GET" => client.get(&url).headers(header_map),
        "HEAD" => client.head(&url).headers(header_map),
        // The caller attaches the owned body after signing so a large pack is
        // not cloned solely to construct the request builder.
        "PUT" => client.put(&url).headers(header_map),
        "DELETE" => client.delete(&url).headers(header_map),
        _ => return Err(format!("unsupported S3 method {}", req.method)),
    };
    Ok(req_builder)
}

/// Rejects a signing component that cannot survive a header round-trip.
///
/// Visible ASCII only: no empty value, no control characters, and no non-ASCII
/// bytes (which `HeaderValue` would accept as opaque octets and then mismatch on
/// the wire).
fn validate_signing_component(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("S3 {field} must not be empty"));
    }
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(format!(
            "S3 {field} contains characters that cannot be sent in a request header"
        ));
    }
    Ok(())
}

/// Inserts one header, reporting invalid names and values instead of panicking.
///
/// Every signed value is attacker- or user-reachable: `if-match` carries an ETag
/// a remote endpoint chose, and `authorization` embeds the configured access key
/// and region. A hostile endpoint answering with a CR/LF in its ETag used to
/// abort the sync worker with a panic instead of a sync error.
fn insert_header(map: &mut HeaderMap, name: &str, value: &str) -> Result<(), String> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| format!("invalid S3 request header name {name:?}"))?;
    let value = HeaderValue::from_str(value)
        .map_err(|_| format!("invalid S3 request header value for {name:?}"))?;
    map.insert(name, value);
    Ok(())
}

/// A `Read` view over a shared buffer.
///
/// `reqwest`'s `Body` only accepts a `'static` payload, and the in-memory upload
/// path can carry the full 256 MiB protocol ceiling, so a retry must re-send the
/// same bytes without copying them. Each attempt gets a fresh view over the same
/// `Arc`.
struct SharedBuffer {
    bytes: Arc<Vec<u8>>,
    offset: usize,
}

impl Read for SharedBuffer {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let start = self.offset.min(self.bytes.len());
        let remaining = &self.bytes[start..];
        let count = remaining.len().min(buffer.len());
        buffer[..count].copy_from_slice(&remaining[..count]);
        self.offset += count;
        Ok(count)
    }
}

/// Attempts allowed for one request/response exchange.
///
/// The retry covers only the exchange, never a partially applied body: every
/// streaming site opens its source inside the builder closure, and the
/// streaming download creates its destination only after a response arrives.
/// So an attempt either transferred an object completely or transferred
/// nothing, and re-issuing it is safe.
const MAX_SEND_ATTEMPTS: u32 = 4;

/// First backoff step; each further attempt doubles it up to the cap.
const SEND_RETRY_BASE_DELAY: Duration = Duration::from_millis(250);
const SEND_RETRY_MAX_DELAY: Duration = Duration::from_secs(4);

/// Statuses worth another attempt: the request was understood but the server
/// could not serve it at that moment.
///
/// Deliberately absent:
/// - 412, which the conditional-put path models as `PreconditionFailed` and
///   which must reach the caller so it can re-read the ETag and re-decide;
/// - 403, an authorization decision that will not change on a retry;
/// - 404, a normal "absent" answer that callers branch on;
/// - 501 and 505, which are permanent server configuration problems.
fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// Transport failures that another attempt can plausibly survive.
///
/// `is_body` is excluded on purpose: a body error means the request was already
/// on the wire, so this stays a conservative list — connect, request, and
/// timeout only.
fn is_retryable_transport(error: &transport::SendError) -> bool {
    error.retryable()
}

/// Reads a `Retry-After` delta-seconds hint, capped at the backoff ceiling.
fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get("retry-after")?.to_str().ok()?.trim().to_owned();
    let seconds: u64 = value.parse().ok()?;
    Some(Duration::from_secs(seconds).min(SEND_RETRY_MAX_DELAY))
}

/// Sends one request, retrying transient failures with exponential backoff.
///
/// `build` is called once per attempt so a streaming body can be re-created;
/// a failure inside it is returned immediately, because a request that cannot
/// even be signed is not going to become signable on a second try. The response
/// is returned for *any* non-retryable status, leaving the caller's existing
/// status handling — 412, 404, and the rest — exactly as it was.
fn send_with_retry(
    build: impl Fn() -> Result<reqwest::RequestBuilder, String>,
    label: &str,
) -> Result<transport::Response, String> {
    let mut backoff = SEND_RETRY_BASE_DELAY;
    let mut last_error = format!("{label} failed");
    for attempt in 0..MAX_SEND_ATTEMPTS {
        if attempt > 0 {
            cancellation::sleep(backoff)?;
            backoff = (backoff * 2).min(SEND_RETRY_MAX_DELAY);
        }
        cancellation::check()?;
        match transport::send(build()?) {
            Ok(response) if !is_retryable_status(response.status().as_u16()) => {
                return Ok(response);
            }
            Ok(response) => {
                last_error = format!("{label} failed: HTTP {}", response.status().as_u16());
                if attempt + 1 == MAX_SEND_ATTEMPTS {
                    break;
                }
                if let Some(hint) = retry_after_delay(response.headers()) {
                    backoff = hint;
                }
            }
            Err(error) if is_retryable_transport(&error) => {
                last_error = format!("{label} failed: {error}");
                if attempt + 1 == MAX_SEND_ATTEMPTS {
                    break;
                }
            }
            Err(error) => return Err(format!("{label} failed: {error}")),
        }
    }
    Err(last_error)
}

/// Hard ceiling for one object buffered fully in memory.
///
/// Only three kinds of object take the in-memory read path: device heads, the
/// checkpoint pointer, and peer segments. All three are a single sync envelope
/// written by a conforming publisher, which never exceeds
/// [`MAX_IN_MEMORY_PROTOCOL_OBJECT_BYTES`]. Everything larger (snapshots,
/// checkpoints, resources) streams to a file with its own caller-supplied limit.
///
/// The ceiling is what stops a broken, misconfigured, or hostile peer from
/// choosing this process's peak memory allocation. It is deliberately much
/// larger than the 16 MiB that sized the transfer budget historically, so a
/// legitimately large segment batch still syncs, and the budget is now derived
/// from this constant instead of being a separate assumption.
pub const MAX_IN_MEMORY_PROTOCOL_OBJECT_BYTES: u64 = 256 * 1024 * 1024;

/// Ceiling for a ListObjectsV2 response body. A full 1000-key page is well
/// under 1 MiB, so this is ~40x headroom and cannot reject a legitimate page.
const MAX_S3_LIST_BODY_BYTES: u64 = 8 * 1024 * 1024;

/// Error bodies are only echoed (first 300 chars) into an error string.
const MAX_S3_ERROR_BODY_BYTES: u64 = 64 * 1024;

/// Reads a response body into memory, refusing to buffer more than `limit`
/// bytes (early-rejecting via `Content-Length` when present, and hard-capping
/// the stream otherwise).
fn read_body_bounded(resp: transport::Response, limit: u64, op: &str) -> Result<Vec<u8>, String> {
    if let Some(len) = resp.content_length() {
        if len > limit {
            return Err(format!("{op} response body exceeds the {limit}-byte limit"));
        }
    }
    let mut bytes = Vec::new();
    resp.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{op} response body read failed: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{op} response body exceeds the {limit}-byte limit"));
    }
    Ok(bytes)
}

fn err_from_response(resp: transport::Response, op: &str) -> String {
    let status = resp.status();
    let body = read_body_bounded(resp, MAX_S3_ERROR_BODY_BYTES, op)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    format!(
        "{op} failed: HTTP {status}: {}",
        body.chars().take(300).collect::<String>()
    )
}

/// Ensures an explicitly configured benchmark bucket exists. Tests must still
/// isolate every run below a random object prefix and delete only that prefix.
#[cfg(any(test, feature = "engine-test-support"))]
pub fn ensure_test_bucket(
    endpoint: &str,
    region: &str,
    bucket: &str,
    access_key: &str,
    secret_key: &str,
) -> Result<(), String> {
    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let mut last_error = "S3 test server did not become ready".to_string();
    for attempt in 0..40 {
        let empty = Vec::new();
        let req = S3Request {
            method: "PUT",
            scheme: &scheme,
            endpoint_host: &host,
            bucket,
            key: "",
            query: None,
            payload: Some(&empty),
            access_key,
            secret_key,
            region,
            extra_headers: &[],
        };
        match transport::send(signed_request(&client, &req)?.body(empty)) {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) if response.status().as_u16() == 409 => {
                let body = String::from_utf8_lossy(&read_body_bounded(
                    response,
                    MAX_S3_ERROR_BODY_BYTES,
                    "create test bucket",
                )?)
                .into_owned();
                if body.contains("<Code>BucketAlreadyOwnedByYou</Code>")
                    || body.contains("<Code>BucketAlreadyExists</Code>")
                {
                    return Ok(());
                }
                return Err(format!(
                    "create test bucket failed: HTTP 409 Conflict: {}",
                    body.chars().take(300).collect::<String>()
                ));
            }
            Ok(response) => {
                let should_retry = response.status().as_u16() == 503;
                last_error = err_from_response(response, "create test bucket");
                if !should_retry {
                    return Err(last_error);
                }
            }
            Err(error) => {
                last_error = format!("create test bucket failed: {error}");
            }
        }
        if attempt + 1 < 40 {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    Err(last_error)
}

pub fn test_s3_connection(
    endpoint: &str,
    region: &str,
    bucket: &str,
    access_key: &str,
    secret_key: &str,
) -> S3TestResult {
    let client = match test_client() {
        Ok(c) => c,
        Err(e) => {
            return S3TestResult {
                success: false,
                message: e,
                status_code: None,
            }
        }
    };
    let (scheme, host) = parse_endpoint(endpoint);
    let query = Some("list-type=2".to_string());
    let req = S3Request {
        method: "GET",
        scheme: &scheme,
        endpoint_host: &host,
        bucket,
        key: "",
        query,
        payload: None,
        access_key,
        secret_key,
        region,
        extra_headers: &[],
    };
    match signed_request(&client, &req).and_then(|r| transport::send(r).map_err(|e| e.to_string()))
    {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                S3TestResult {
                    success: true,
                    message: "Connection successful".to_string(),
                    status_code: Some(status.as_u16()),
                }
            } else if status.as_u16() == 403 {
                S3TestResult {
                    success: false,
                    message: "Authentication failed".to_string(),
                    status_code: Some(403),
                }
            } else {
                S3TestResult {
                    success: false,
                    message: format!("Server returned HTTP {status}"),
                    status_code: Some(status.as_u16()),
                }
            }
        }
        Err(e) => S3TestResult {
            success: false,
            message: format!("Network error: {e}"),
            status_code: None,
        },
    }
}

pub fn upload_to_s3(
    endpoint: &str,
    region: &str,
    bucket: &str,
    key: &str,
    data: Vec<u8>,
    access_key: &str,
    secret_key: &str,
) -> Result<(), String> {
    match put_s3_object(
        endpoint,
        region,
        bucket,
        key,
        data,
        access_key,
        secret_key,
        S3PutCondition::Unconditional,
    )? {
        S3PutOutcome::Stored { .. } => Ok(()),
        S3PutOutcome::PreconditionFailed => {
            Err("unconditional S3 upload failed its precondition".to_string())
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn put_s3_object(
    endpoint: &str,
    region: &str,
    bucket: &str,
    key: &str,
    data: Vec<u8>,
    access_key: &str,
    secret_key: &str,
    condition: S3PutCondition,
) -> Result<S3PutOutcome, String> {
    // Refuse locally rather than publish an object that this protocol version's
    // readers would reject: every object written through the in-memory path is
    // also read back through the in-memory path, so exceeding the ceiling has to
    // be a visible error here, not a remote rejection on the next device.
    if data.len() as u64 > MAX_IN_MEMORY_PROTOCOL_OBJECT_BYTES {
        return Err(format!(
            "refusing to publish a {key:?} object of {} bytes: the in-memory protocol ceiling is {MAX_IN_MEMORY_PROTOCOL_OBJECT_BYTES} bytes",
            data.len()
        ));
    }
    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let content_md5 = content_md5_base64(&data);
    let payload_len = data.len() as u64;
    let mut extra_headers = vec![
        ("content-type", "application/octet-stream"),
        ("content-md5", content_md5.as_str()),
    ];
    match &condition {
        S3PutCondition::Unconditional => {}
        S3PutCondition::IfAbsent => extra_headers.push(("if-none-match", "*")),
        S3PutCondition::IfMatch(etag) => extra_headers.push(("if-match", etag.as_str())),
    }
    // Hash the payload up front so the request no longer borrows `data`; the
    // buffer then moves into the shared reader the retry closure rebuilds from.
    let payload_hash = sha256_hex(&data);
    let req = S3Request {
        method: "PUT",
        scheme: &scheme,
        endpoint_host: &host,
        bucket,
        key,
        query: None,
        payload: None,
        access_key,
        secret_key,
        region,
        extra_headers: &extra_headers,
    };
    let payload = Arc::new(data);
    let resp = send_with_retry(
        || {
            let view = SharedBuffer {
                bytes: Arc::clone(&payload),
                offset: 0,
            };
            Ok(
                signed_request_with_payload_hash(&client, &req, &payload_hash)?
                    .timeout(streaming_timeout(payload_len))
                    .header(CONTENT_LENGTH, payload_len)
                    .body(transport::stream_body(view)),
            )
        },
        "upload",
    )?;

    if resp.status().is_success() {
        Ok(S3PutOutcome::Stored {
            etag: response_etag(&resp)?,
        })
    } else if resp.status().as_u16() == 412 {
        Ok(S3PutOutcome::PreconditionFailed)
    } else {
        Err(err_from_response(resp, "upload"))
    }
}

/// Uploads a file without buffering it into a `Vec<u8>`. `payload_sha256`
/// must be the lowercase SHA-256 digest from the caller's fingerprint pass;
/// S3 verifies the same digest while receiving the streamed request body.
#[allow(clippy::too_many_arguments)]
pub fn put_s3_file(
    endpoint: &str,
    region: &str,
    bucket: &str,
    key: &str,
    path: &Path,
    payload_sha256: &str,
    size_bytes: u64,
    access_key: &str,
    secret_key: &str,
    condition: S3PutCondition,
) -> Result<S3PutOutcome, String> {
    validate_payload_sha256(payload_sha256)?;
    let file =
        File::open(path).map_err(|error| format!("failed to open S3 upload file: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect S3 upload file: {error}"))?;
    if !metadata.is_file() {
        return Err("S3 upload source is not a regular file".to_string());
    }
    if metadata.len() != size_bytes {
        return Err(format!(
            "S3 upload source size changed: expected {size_bytes}, got {}",
            metadata.len()
        ));
    }

    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let mut extra_headers = vec![("content-type", "application/octet-stream")];
    match &condition {
        S3PutCondition::Unconditional => {}
        S3PutCondition::IfAbsent => extra_headers.push(("if-none-match", "*")),
        S3PutCondition::IfMatch(etag) => extra_headers.push(("if-match", etag.as_str())),
    }
    let req = S3Request {
        method: "PUT",
        scheme: &scheme,
        endpoint_host: &host,
        bucket,
        key,
        query: None,
        payload: None,
        access_key,
        secret_key,
        region,
        extra_headers: &extra_headers,
    };
    let response = send_with_retry(
        || {
            // Each attempt needs its own handle: the previous one is owned by the
            // request body that just failed.
            let source = File::open(path)
                .map_err(|error| format!("failed to open S3 upload file: {error}"))?;
            Ok(
                signed_request_with_payload_hash(&client, &req, payload_sha256)?
                    .timeout(streaming_timeout(size_bytes))
                    .header(CONTENT_LENGTH, size_bytes)
                    .body(transport::stream_body(source)),
            )
        },
        "streaming upload",
    )?;

    if response.status().is_success() {
        Ok(S3PutOutcome::Stored {
            etag: response_etag(&response)?,
        })
    } else if response.status().as_u16() == 412 {
        Ok(S3PutOutcome::PreconditionFailed)
    } else {
        Err(err_from_response(response, "streaming upload"))
    }
}

pub fn download_from_s3(
    endpoint: &str,
    region: &str,
    bucket: &str,
    key: &str,
    access_key: &str,
    secret_key: &str,
) -> Result<Vec<u8>, String> {
    get_s3_object(endpoint, region, bucket, key, access_key, secret_key)?
        .map(|object| object.bytes)
        .ok_or_else(|| format!("download failed: S3 object not found: {key}"))
}

pub fn get_s3_object(
    endpoint: &str,
    region: &str,
    bucket: &str,
    key: &str,
    access_key: &str,
    secret_key: &str,
) -> Result<Option<S3DownloadedObject>, String> {
    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let req = S3Request {
        method: "GET",
        scheme: &scheme,
        endpoint_host: &host,
        bucket,
        key,
        query: None,
        payload: None,
        access_key,
        secret_key,
        region,
        extra_headers: &[],
    };
    let resp = send_with_retry(
        || {
            Ok(signed_request(&client, &req)?
                .timeout(streaming_timeout(MAX_IN_MEMORY_PROTOCOL_OBJECT_BYTES)))
        },
        "download",
    )?;

    if resp.status().is_success() {
        let etag = response_etag(&resp)?;
        let bytes = read_body_bounded(resp, MAX_IN_MEMORY_PROTOCOL_OBJECT_BYTES, "download")?;
        Ok(Some(S3DownloadedObject { bytes, etag }))
    } else if resp.status().as_u16() == 404 {
        Ok(None)
    } else {
        Err(err_from_response(resp, "download"))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn head_s3_object(
    endpoint: &str,
    region: &str,
    bucket: &str,
    key: &str,
    access_key: &str,
    secret_key: &str,
) -> Result<Option<S3ObjectMetadata>, String> {
    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let req = S3Request {
        method: "HEAD",
        scheme: &scheme,
        endpoint_host: &host,
        bucket,
        key,
        query: None,
        payload: None,
        access_key,
        secret_key,
        region,
        extra_headers: &[],
    };
    let response = send_with_retry(|| signed_request(&client, &req), "metadata request")?;

    if response.status().is_success() {
        Ok(Some(S3ObjectMetadata {
            size_bytes: response_content_length_header(response.headers())?,
            etag: response_etag(&response)?,
        }))
    } else if response.status().as_u16() == 404 {
        Ok(None)
    } else {
        Err(err_from_response(response, "metadata request"))
    }
}

/// Streams one S3 object into a newly-created destination file while hashing
/// it. Partial files are removed on every error and the caller remains
/// responsible for atomically renaming a verified download into place.
#[allow(clippy::too_many_arguments)]
pub fn get_s3_object_to_file(
    endpoint: &str,
    region: &str,
    bucket: &str,
    key: &str,
    destination: &Path,
    max_bytes: u64,
    access_key: &str,
    secret_key: &str,
) -> Result<Option<S3DownloadedFile>, String> {
    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let req = S3Request {
        method: "GET",
        scheme: &scheme,
        endpoint_host: &host,
        bucket,
        key,
        query: None,
        payload: None,
        access_key,
        secret_key,
        region,
        extra_headers: &[],
    };
    let mut response = send_with_retry(
        || Ok(signed_request(&client, &req)?.timeout(streaming_timeout(max_bytes))),
        "streaming download",
    )?;

    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(err_from_response(response, "streaming download"));
    }
    let expected_size = response.content_length();
    if expected_size.is_some_and(|size| size > max_bytes) {
        return Err(format!(
            "streaming download exceeds the {max_bytes}-byte limit"
        ));
    }
    let etag = response_etag(&response)?;
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("failed to create streaming download file: {error}"))?;

    let download_result = (|| {
        let mut hasher = Sha256::new();
        let mut size_bytes = 0u64;
        let mut buffer = [0u8; 128 * 1024];
        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|error| format!("failed to read streaming download: {error}"))?;
            if read == 0 {
                break;
            }
            size_bytes = size_bytes
                .checked_add(read as u64)
                .ok_or_else(|| "streaming download size overflowed".to_string())?;
            if size_bytes > max_bytes {
                return Err(format!(
                    "streaming download exceeds the {max_bytes}-byte limit"
                ));
            }
            destination_file
                .write_all(&buffer[..read])
                .map_err(|error| format!("failed to write streaming download: {error}"))?;
            hasher.update(&buffer[..read]);
        }
        if expected_size.is_some_and(|size| size != size_bytes) {
            return Err(format!(
                "streaming download size mismatch: expected {}, got {size_bytes}",
                expected_size.unwrap_or_default()
            ));
        }
        destination_file
            .flush()
            .map_err(|error| format!("failed to flush streaming download: {error}"))?;
        Ok(S3DownloadedFile {
            size_bytes,
            sha256: hex::encode(hasher.finalize()),
            etag,
        })
    })();

    drop(destination_file);
    if download_result.is_err() {
        let _ = std::fs::remove_file(destination);
    }
    download_result.map(Some)
}

pub fn list_s3_objects(
    endpoint: &str,
    region: &str,
    bucket: &str,
    prefix: Option<&str>,
    access_key: &str,
    secret_key: &str,
) -> Result<Vec<S3Entry>, String> {
    list_s3_objects_after(
        endpoint, region, bucket, prefix, None, access_key, secret_key,
    )
}

/// Lists every object below `prefix`, optionally beginning strictly after a
/// known object key. S3 caps one ListObjectsV2 response at 1000 keys, so the
/// continuation token must be followed until the server reports a complete
/// page set.
pub fn list_s3_objects_after(
    endpoint: &str,
    region: &str,
    bucket: &str,
    prefix: Option<&str>,
    start_after: Option<&str>,
    access_key: &str,
    secret_key: &str,
) -> Result<Vec<S3Entry>, String> {
    list_s3_objects_after_with_metrics(
        endpoint,
        region,
        bucket,
        prefix,
        start_after,
        access_key,
        secret_key,
        None,
    )
}

/// Upper bound on objects buffered by one listing call. The listing is
/// paginated but accumulated in memory; a namespace beyond this fails loudly
/// instead of exhausting memory. Sized well above realistic clipboard counts.
const MAX_LISTED_OBJECTS: usize = 1_000_000;

#[allow(clippy::too_many_arguments)]
pub(crate) fn list_s3_objects_after_with_metrics(
    endpoint: &str,
    region: &str,
    bucket: &str,
    prefix: Option<&str>,
    start_after: Option<&str>,
    access_key: &str,
    secret_key: &str,
    metrics: Option<&S3RequestMetrics>,
) -> Result<Vec<S3Entry>, String> {
    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let mut entries = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let query = build_list_query(
            prefix,
            continuation_token
                .is_none()
                .then_some(start_after)
                .flatten(),
            continuation_token.as_deref(),
        );
        let req = S3Request {
            method: "GET",
            scheme: &scheme,
            endpoint_host: &host,
            bucket,
            key: "",
            query: Some(query),
            payload: None,
            access_key,
            secret_key,
            region,
            extra_headers: &[],
        };
        let resp = send_with_retry(|| signed_request(&client, &req), "list")?;

        if !resp.status().is_success() {
            return Err(err_from_response(resp, "list"));
        }

        // Decode in place: `from_utf8_lossy().into_owned()` would keep the
        // bounded `Vec` and the `String` alive at the same time, doubling the
        // peak allocation for no benefit.
        let xml =
            String::from_utf8_lossy(&read_body_bounded(resp, MAX_S3_LIST_BODY_BYTES, "list")?)
                .into_owned();
        if let Some(metrics) = metrics {
            metrics.record_list_page(xml.len() as u64);
        }
        let page = parse_s3_list_page(&xml);
        entries.extend(page.entries);
        if entries.len() > MAX_LISTED_OBJECTS {
            return Err(format!(
                "S3 listing exceeded {MAX_LISTED_OBJECTS} objects; refusing to buffer more"
            ));
        }
        if !page.is_truncated {
            return Ok(entries);
        }

        let next = page.next_continuation_token.ok_or_else(|| {
            "S3 returned a truncated object listing without a continuation token".to_string()
        })?;
        if continuation_token.as_deref() == Some(next.as_str()) {
            return Err("S3 repeated an object-list continuation token".to_string());
        }
        continuation_token = Some(next);
    }
}

pub fn delete_from_s3(
    endpoint: &str,
    region: &str,
    bucket: &str,
    key: &str,
    access_key: &str,
    secret_key: &str,
) -> Result<(), String> {
    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let req = S3Request {
        method: "DELETE",
        scheme: &scheme,
        endpoint_host: &host,
        bucket,
        key,
        query: None,
        payload: None,
        access_key,
        secret_key,
        region,
        extra_headers: &[],
    };
    let resp = send_with_retry(|| signed_request(&client, &req), "delete")?;

    if resp.status().is_success() || resp.status().as_u16() == 404 {
        Ok(())
    } else {
        Err(err_from_response(resp, "delete"))
    }
}

fn percent_encode(input: &str) -> String {
    input
        .as_bytes()
        .iter()
        .map(|&b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

fn build_list_query(
    prefix: Option<&str>,
    start_after: Option<&str>,
    continuation_token: Option<&str>,
) -> String {
    let mut parameters = vec![("list-type", "2")];
    if let Some(token) = continuation_token.filter(|value| !value.is_empty()) {
        parameters.push(("continuation-token", token));
    } else if let Some(start_after) = start_after.filter(|value| !value.is_empty()) {
        parameters.push(("start-after", start_after));
    }
    if let Some(prefix) = prefix.filter(|value| !value.is_empty()) {
        parameters.push(("prefix", prefix));
    }
    parameters.sort_unstable_by_key(|(name, _)| *name);
    parameters
        .into_iter()
        .map(|(name, value)| format!("{name}={}", percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn content_md5_base64(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(data);
    STANDARD.encode(hasher.finalize())
}

fn validate_payload_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("S3 payload SHA-256 must be 64 lowercase hexadecimal characters".to_string());
    }
    Ok(())
}

/// Size budget used to size the in-memory (non-file-streamed) object transfer
/// timeout. Segments and checkpoint/device pointers must not inherit the fixed
/// 60 s shared-client timeout, or a slow link aborts a legitimate transfer that
/// the size-scaled streaming paths would have completed. The budget is derived
/// from the same constant that bounds the buffered body, so the two cannot
/// disagree.
fn streaming_timeout(size_limit_bytes: u64) -> Duration {
    const ASSUMED_MIN_BYTES_PER_SECOND: u64 = 64 * 1024;
    const BASE_SECONDS: u64 = 60;
    const MAX_SECONDS: u64 = 30 * 60;

    let transfer_seconds = size_limit_bytes.saturating_add(ASSUMED_MIN_BYTES_PER_SECOND - 1)
        / ASSUMED_MIN_BYTES_PER_SECOND;
    Duration::from_secs(
        BASE_SECONDS
            .saturating_add(transfer_seconds)
            .min(MAX_SECONDS),
    )
}

fn response_etag(response: &transport::Response) -> Result<Option<String>, String> {
    response
        .headers()
        .get(ETAG)
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|error| format!("S3 returned an invalid ETag header: {error}"))
        })
        .transpose()
}

fn response_content_length_header(headers: &HeaderMap) -> Result<Option<u64>, String> {
    headers
        .get(CONTENT_LENGTH)
        .map(|value| {
            value
                .to_str()
                .map_err(|error| format!("S3 returned an invalid Content-Length header: {error}"))?
                .parse::<u64>()
                .map_err(|error| format!("S3 returned an invalid Content-Length value: {error}"))
        })
        .transpose()
}

struct S3ListPage {
    entries: Vec<S3Entry>,
    is_truncated: bool,
    next_continuation_token: Option<String>,
}

fn parse_s3_list_page(xml: &str) -> S3ListPage {
    let mut entries = Vec::new();
    let mut search_start = 0;

    while let Some(pos) = xml[search_start..].find("<Contents>") {
        let abs_pos = search_start + pos;
        let block = &xml[abs_pos..];
        let end = block.find("</Contents>").unwrap_or(block.len());
        let block = &block[..end];

        let key = decode_xml_text(extract_tag(block, "Key").unwrap_or_default());
        let size = extract_tag(block, "Size").and_then(|s| s.parse::<u64>().ok());
        let modified = extract_tag(block, "LastModified").and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.timestamp_millis())
        });
        let etag = extract_tag(block, "ETag")
            .map(decode_xml_text)
            .filter(|value| !value.is_empty());

        if !key.is_empty() {
            entries.push(S3Entry {
                object_key: key.clone(),
                name: key.split('/').next_back().unwrap_or(&key).to_string(),
                is_directory: false,
                size_bytes: size,
                modified_ms: modified,
                etag,
            });
            search_start = abs_pos + end;
        } else {
            search_start = abs_pos + 1;
        }
    }

    S3ListPage {
        entries,
        is_truncated: extract_tag(xml, "IsTruncated")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("true")),
        next_continuation_token: extract_tag(xml, "NextContinuationToken")
            .map(decode_xml_text)
            .filter(|value| !value.is_empty()),
    }
}

fn decode_xml_text(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('&') {
        decoded.push_str(&remaining[..start]);
        let entity = &remaining[start..];
        let Some(end) = entity.find(';') else {
            decoded.push_str(entity);
            return decoded;
        };
        let name = &entity[1..end];
        let character = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "amp" => Some('&'),
            numeric if numeric.starts_with("#x") || numeric.starts_with("#X") => {
                u32::from_str_radix(&numeric[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            numeric if numeric.starts_with('#') => {
                numeric[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        if let Some(character) = character {
            decoded.push(character);
        } else {
            decoded.push_str(&entity[..=end]);
        }
        remaining = &entity[end + 1..];
    }
    decoded.push_str(remaining);
    decoded
}

fn extract_tag<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = block.find(&open)? + open.len();
    let e = block[s..].find(&close)? + s;
    Some(&block[s..e])
}

#[cfg(test)]
mod tests {
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

        assert!(authorization
            .contains("SignedHeaders=host;if-none-match;x-amz-content-sha256;x-amz-date"));
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
            let dir =
                std::env::temp_dir().join(format!("clipboard-cancel-{}", uuid::Uuid::new_v4()));
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
        let bucket = std::env::var("CLIPBOARD_S3_TEST_BUCKET")
            .expect("CLIPBOARD_S3_TEST_BUCKET must be set");
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
}
