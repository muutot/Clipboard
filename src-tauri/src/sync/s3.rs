use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Entry {
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: Option<u64>,
    pub modified_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct S3TestResult {
    pub success: bool,
    pub message: String,
    pub status_code: Option<u16>,
}

fn build_s3_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("failed to build S3 client: {e}"))
}

fn now_rfc7231() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let datetime = chrono::DateTime::from_timestamp(now, 0).unwrap_or_default();
    datetime.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn simple_md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn test_s3_connection(
    endpoint: &str,
    _region: &str,
    bucket: &str,
    access_key: &str,
    _secret_key: &str,
) -> S3TestResult {
    let client = match build_s3_client() {
        Ok(c) => c,
        Err(e) => {
            return S3TestResult {
                success: false,
                message: e,
                status_code: None,
            }
        }
    };

    let url = format!("https://{bucket}.{endpoint}/");
    let date = now_rfc7231();

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("date"),
        HeaderValue::from_str(&date).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("AWS {access_key}:placeholder")).unwrap(),
    );

    match client.head(&url).headers(headers).send() {
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
                    message: "Authentication failed or bucket not found".to_string(),
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
    _region: &str,
    bucket: &str,
    key: &str,
    data: Vec<u8>,
    access_key: &str,
    _secret_key: &str,
) -> Result<(), String> {
    let client = build_s3_client()?;
    let url = format!("https://{bucket}.{endpoint}/{key}");
    let date = now_rfc7231();
    let content_md5 = simple_md5_hex(&data);

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("date"),
        HeaderValue::from_str(&date).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("content-md5"),
        HeaderValue::from_str(&content_md5).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("AWS {access_key}:placeholder")).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("content-length"),
        HeaderValue::from_str(&data.len().to_string()).unwrap(),
    );

    let resp = client
        .put(&url)
        .headers(headers)
        .body(data)
        .send()
        .map_err(|e| format!("upload failed: {e}"))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("upload returned HTTP {}", resp.status()))
    }
}

pub fn download_from_s3(
    endpoint: &str,
    _region: &str,
    bucket: &str,
    key: &str,
    access_key: &str,
    _secret_key: &str,
) -> Result<Vec<u8>, String> {
    let client = build_s3_client()?;
    let url = format!("https://{bucket}.{endpoint}/{key}");
    let date = now_rfc7231();

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("date"),
        HeaderValue::from_str(&date).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("AWS {access_key}:placeholder")).unwrap(),
    );

    let resp = client
        .get(&url)
        .headers(headers)
        .send()
        .map_err(|e| format!("download failed: {e}"))?;

    if resp.status().is_success() {
        let bytes = resp.bytes().map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    } else {
        Err(format!("download returned HTTP {}", resp.status()))
    }
}

pub fn list_s3_objects(
    endpoint: &str,
    _region: &str,
    bucket: &str,
    prefix: Option<&str>,
    access_key: &str,
    _secret_key: &str,
) -> Result<Vec<S3Entry>, String> {
    let client = build_s3_client()?;
    let prefix_str = prefix.unwrap_or("");
    let url = format!("https://{bucket}.{endpoint}/?prefix={prefix_str}&delimiter=/");
    let date = now_rfc7231();

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("date"),
        HeaderValue::from_str(&date).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("AWS {access_key}:placeholder")).unwrap(),
    );

    let resp = client
        .get(&url)
        .headers(headers)
        .send()
        .map_err(|e| format!("list failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("list returned HTTP {}", resp.status()));
    }

    let xml = resp.text().map_err(|e| e.to_string())?;
    Ok(parse_s3_list_response(&xml))
}

pub fn delete_from_s3(
    endpoint: &str,
    _region: &str,
    bucket: &str,
    key: &str,
    access_key: &str,
    _secret_key: &str,
) -> Result<(), String> {
    let client = build_s3_client()?;
    let url = format!("https://{bucket}.{endpoint}/{key}");
    let date = now_rfc7231();

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("date"),
        HeaderValue::from_str(&date).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("AWS {access_key}:placeholder")).unwrap(),
    );

    let resp = client
        .delete(&url)
        .headers(headers)
        .send()
        .map_err(|e| format!("delete failed: {e}"))?;

    if resp.status().is_success() || resp.status().as_u16() == 404 {
        Ok(())
    } else {
        Err(format!("delete returned HTTP {}", resp.status()))
    }
}

fn parse_s3_list_response(xml: &str) -> Vec<S3Entry> {
    let mut entries = Vec::new();
    let mut search_start = 0;

    while let Some(pos) = xml[search_start..].find("<Contents>") {
        let abs_pos = search_start + pos;
        let block = &xml[abs_pos..];
        let end = block.find("</Contents>").unwrap_or(block.len());
        let block = &block[..end];

        let key = extract_tag(block, "Key").unwrap_or_default();
        let size = extract_tag(block, "Size").and_then(|s| s.parse::<u64>().ok());
        let modified = extract_tag(block, "LastModified").and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.timestamp_millis())
        });

        if !key.is_empty() {
            entries.push(S3Entry {
                name: key.split('/').next_back().unwrap_or(key).to_string(),
                is_directory: false,
                size_bytes: size,
                modified_ms: modified,
            });
            search_start = abs_pos + end;
        } else {
            search_start = abs_pos + 1;
        }
    }

    entries
}

fn extract_tag<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = block.find(&open)? + open.len();
    let e = block[s..].find(&close)? + s;
    Some(&block[s..e])
}
