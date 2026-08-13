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

use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ETAG};
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
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac accepts any key len");
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
fn signed_request(
    client: &reqwest::blocking::Client,
    req: &S3Request,
) -> Result<RequestBuilder, String> {
    let data = req.payload.unwrap_or(&[]);
    let payload_hash = sha256_hex(data);
    signed_request_with_payload_hash(client, req, &payload_hash)
}

fn signed_request_with_payload_hash(
    client: &reqwest::blocking::Client,
    req: &S3Request,
    payload_hash: &str,
) -> Result<RequestBuilder, String> {
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
    header_map.insert(
        HeaderName::from_static("x-amz-date"),
        HeaderValue::from_str(&signer.amz_date).unwrap(),
    );
    header_map.insert(
        HeaderName::from_static("x-amz-content-sha256"),
        HeaderValue::from_str(payload_hash).unwrap(),
    );
    for (name, value) in req.extra_headers {
        header_map.insert(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    header_map.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&authorization).unwrap(),
    );

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

fn err_from_response(resp: reqwest::blocking::Response, op: &str) -> String {
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    format!(
        "{op} failed: HTTP {status}: {}",
        body.chars().take(300).collect::<String>()
    )
}

pub fn test_s3_connection(
    endpoint: &str,
    region: &str,
    bucket: &str,
    access_key: &str,
    secret_key: &str,
) -> S3TestResult {
    let client = match shared_client() {
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
    match signed_request(&client, &req).and_then(|r| r.send().map_err(|e| e.to_string())) {
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
    let client = shared_client()?;
    let (scheme, host) = parse_endpoint(endpoint);
    let content_md5 = content_md5_base64(&data);
    let mut extra_headers = vec![
        ("content-type", "application/octet-stream"),
        ("content-md5", content_md5.as_str()),
    ];
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
        payload: Some(&data),
        access_key,
        secret_key,
        region,
        extra_headers: &extra_headers,
    };
    let resp = signed_request(&client, &req)?
        .body(data)
        .send()
        .map_err(|e| format!("upload failed: {e}"))?;

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
    let response = signed_request_with_payload_hash(&client, &req, payload_sha256)?
        .timeout(streaming_timeout(size_bytes))
        .body(reqwest::blocking::Body::sized(file, size_bytes))
        .send()
        .map_err(|error| format!("streaming upload failed: {error}"))?;

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
    let resp = signed_request(&client, &req)?
        .send()
        .map_err(|e| format!("download failed: {e}"))?;

    if resp.status().is_success() {
        let etag = response_etag(&resp)?;
        let bytes = resp.bytes().map_err(|e| e.to_string())?.to_vec();
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
    let response = signed_request(&client, &req)?
        .send()
        .map_err(|error| format!("metadata request failed: {error}"))?;

    if response.status().is_success() {
        Ok(Some(S3ObjectMetadata {
            size_bytes: response.content_length(),
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
    let mut response = signed_request(&client, &req)?
        .timeout(streaming_timeout(max_bytes))
        .send()
        .map_err(|error| format!("streaming download failed: {error}"))?;

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
        endpoint, region, bucket, prefix, start_after, access_key, secret_key, None,
    )
}

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
        let resp = signed_request(&client, &req)?
            .send()
            .map_err(|e| format!("list failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(err_from_response(resp, "list"));
        }

        let xml = resp.text().map_err(|e| e.to_string())?;
        if let Some(metrics) = metrics {
            metrics.record_list_page(xml.len() as u64);
        }
        let page = parse_s3_list_page(&xml);
        entries.extend(page.entries);
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
    let resp = signed_request(&client, &req)?
        .send()
        .map_err(|e| format!("delete failed: {e}"))?;

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

fn response_etag(response: &reqwest::blocking::Response) -> Result<Option<String>, String> {
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
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
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

        create_disposable_test_bucket(&endpoint, &region, &bucket, &access_key, &secret_key)
            .unwrap();

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
        assert!(matches!(updated, S3PutOutcome::Stored { .. }));
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

    fn create_disposable_test_bucket(
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
            match signed_request(&client, &req)?.body(empty).send() {
                Ok(response)
                    if response.status().is_success() || response.status().as_u16() == 409 =>
                {
                    return Ok(());
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
}
