//! Object listing: pagination query, XML page parsing and helper decoders.

use super::*;

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
pub(super) const MAX_LISTED_OBJECTS: usize = 1_000_000;

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

pub(super) fn build_list_query(
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

pub(super) struct S3ListPage {
    pub(super) entries: Vec<S3Entry>,
    pub(super) is_truncated: bool,
    pub(super) next_continuation_token: Option<String>,
}

pub(super) fn parse_s3_list_page(xml: &str) -> S3ListPage {
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

pub(super) fn decode_xml_text(value: &str) -> String {
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
