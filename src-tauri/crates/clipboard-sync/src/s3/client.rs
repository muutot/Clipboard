//! Shared reqwest client, retry policy, bounded body reads and response helpers.

use super::*;

/// A shared request-signing client. Rebuilt only once, then reused so every
/// S3 call does not pay connection/header overhead.
pub(super) fn shared_client() -> Result<Client, String> {
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

pub(super) fn test_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("failed to build S3 test client: {error}"))
}

/// A `Read` view over a shared buffer.
///
/// `reqwest`'s `Body` only accepts a `'static` payload, and the in-memory upload
/// path can carry the full 256 MiB protocol ceiling, so a retry must re-send the
/// same bytes without copying them. Each attempt gets a fresh view over the same
/// `Arc`.
pub(super) struct SharedBuffer {
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) offset: usize,
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
pub(super) const MAX_SEND_ATTEMPTS: u32 = 4;

/// First backoff step; each further attempt doubles it up to the cap.
const SEND_RETRY_BASE_DELAY: Duration = Duration::from_millis(250);
pub(super) const SEND_RETRY_MAX_DELAY: Duration = Duration::from_secs(4);

/// Statuses worth another attempt: the request was understood but the server
/// could not serve it at that moment.
///
/// Deliberately absent:
/// - 412, which the conditional-put path models as `PreconditionFailed` and
///   which must reach the caller so it can re-read the ETag and re-decide;
/// - 403, an authorization decision that will not change on a retry;
/// - 404, a normal "absent" answer that callers branch on;
/// - 501 and 505, which are permanent server configuration problems.
pub(super) fn is_retryable_status(status: u16) -> bool {
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
pub(super) fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
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
pub(super) fn send_with_retry(
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
pub(super) const MAX_S3_LIST_BODY_BYTES: u64 = 8 * 1024 * 1024;

/// Error bodies are only echoed (first 300 chars) into an error string.
pub(super) const MAX_S3_ERROR_BODY_BYTES: u64 = 64 * 1024;

/// Reads a response body into memory, refusing to buffer more than `limit`
/// bytes (early-rejecting via `Content-Length` when present, and hard-capping
/// the stream otherwise).
pub(super) fn read_body_bounded(
    resp: transport::Response,
    limit: u64,
    op: &str,
) -> Result<Vec<u8>, String> {
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

pub(super) fn err_from_response(resp: transport::Response, op: &str) -> String {
    let status = resp.status();
    let body = read_body_bounded(resp, MAX_S3_ERROR_BODY_BYTES, op)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    format!(
        "{op} failed: HTTP {status}: {}",
        body.chars().take(300).collect::<String>()
    )
}

/// Size budget used to size the in-memory (non-file-streamed) object transfer
/// timeout. Segments and checkpoint/device pointers must not inherit the fixed
/// 60 s shared-client timeout, or a slow link aborts a legitimate transfer that
/// the size-scaled streaming paths would have completed. The budget is derived
/// from the same constant that bounds the buffered body, so the two cannot
/// disagree.
pub(super) fn streaming_timeout(size_limit_bytes: u64) -> Duration {
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

pub(super) fn response_etag(response: &transport::Response) -> Result<Option<String>, String> {
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

pub(super) fn response_content_length_header(headers: &HeaderMap) -> Result<Option<u64>, String> {
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
