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
        let body = read_body_bounded(resp, MAX_S3_LIST_BODY_BYTES, "list")?;
        if let Some(metrics) = metrics {
            metrics.record_list_page(body.len() as u64);
        }
        let xml = String::from_utf8(body)
            .map_err(|_| "S3 list response is not valid UTF-8".to_string())?;
        let page = parse_s3_list_page(&xml)?;
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListResult {
    is_truncated: bool,
    next_continuation_token: Option<String>,
    #[serde(rename = "Contents", default)]
    entries: Vec<ListEntry>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListEntry {
    key: String,
    size: Option<u64>,
    last_modified: Option<String>,
    #[serde(rename = "ETag")]
    etag: Option<String>,
}

fn invalid_list_xml() -> String {
    // Do not include response bodies or object keys in a parsing diagnostic.
    "S3 list response is not a complete valid ListBucketResult XML document".to_string()
}

/// Serde consumes one value; validate the entire document first so a closed
/// first root or missing trailing closing tag cannot return partial discovery.
fn validate_list_document(xml: &str) -> Result<(), String> {
    use quick_xml::{events::Event, Reader};

    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let mut depth = 0usize;
    let mut root_seen = false;
    loop {
        match reader.read_event().map_err(|_| invalid_list_xml())? {
            Event::Start(element) => {
                if depth == 0 {
                    if root_seen || element.local_name().as_ref() != "ListBucketResult" {
                        return Err(invalid_list_xml());
                    }
                    root_seen = true;
                }
                for attribute in element.attributes() {
                    attribute
                        .map_err(|_| invalid_list_xml())?
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| invalid_list_xml())?;
                }
                depth += 1;
                if depth > 64 {
                    return Err(invalid_list_xml());
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or_else(invalid_list_xml)?;
            }
            Event::Text(text) => {
                if depth == 0 && !text.trim().is_empty() {
                    return Err(invalid_list_xml());
                }
            }
            Event::GeneralRef(reference) => {
                if depth == 0 {
                    return Err(invalid_list_xml());
                }
                if reference
                    .resolve_char_ref()
                    .map_err(|_| invalid_list_xml())?
                    .is_none()
                    && quick_xml::escape::resolve_predefined_entity(&reference).is_none()
                {
                    return Err(invalid_list_xml());
                }
            }
            Event::CData(_) if depth == 0 => return Err(invalid_list_xml()),
            Event::DocType(_) => return Err(invalid_list_xml()),
            Event::Decl(_) if root_seen => return Err(invalid_list_xml()),
            Event::Eof => {
                return if root_seen && depth == 0 {
                    Ok(())
                } else {
                    Err(invalid_list_xml())
                };
            }
            _ => {}
        }
    }
}

pub(super) fn parse_s3_list_page(xml: &str) -> Result<S3ListPage, String> {
    validate_list_document(xml)?;
    let result: ListResult = quick_xml::de::from_str(xml).map_err(|_| invalid_list_xml())?;
    let entries = result
        .entries
        .into_iter()
        .map(|entry| {
            if entry.key.is_empty() {
                return Err(invalid_list_xml());
            }
            Ok(S3Entry {
                name: entry
                    .key
                    .split('/')
                    .next_back()
                    .unwrap_or(&entry.key)
                    .to_string(),
                object_key: entry.key,
                is_directory: false,
                size_bytes: entry.size,
                modified_ms: entry.last_modified.and_then(|value| {
                    chrono::DateTime::parse_from_rfc3339(&value)
                        .ok()
                        .map(|dt| dt.timestamp_millis())
                }),
                etag: entry.etag.filter(|value| !value.is_empty()),
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(S3ListPage {
        entries,
        is_truncated: result.is_truncated,
        next_continuation_token: result
            .next_continuation_token
            .filter(|value| !value.is_empty()),
    })
}
