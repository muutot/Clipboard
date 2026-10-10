//! Object-level S3 operations: upload, download, head, delete and connectivity.

use super::*;

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

pub(super) fn content_md5_base64(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(data);
    STANDARD.encode(hasher.finalize())
}

pub(super) fn validate_payload_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("S3 payload SHA-256 must be 64 lowercase hexadecimal characters".to_string());
    }
    Ok(())
}
