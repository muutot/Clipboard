use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavEntry {
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: Option<u64>,
    pub modified_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavTestResult {
    pub success: bool,
    pub message: String,
    pub status_code: Option<u16>,
}

/// A shared HTTP client. Built once, then reused for every WebDAV call so each
/// request does not pay connection/header setup overhead. Auth is attached
/// per-request via `basic_auth`, since credentials vary between operations.
fn shared_client() -> Result<Client, String> {
    static CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
    Ok(CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("failed to build WebDAV client")
        })
        .clone())
}

fn auth<'a>(user: Option<&'a str>, pass: Option<&'a str>) -> Option<(&'a str, Option<&'a str>)> {
    match (user, pass) {
        (Some(u), Some(p)) => Some((u, Some(p))),
        _ => None,
    }
}

fn join_webdav_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{path}")
    }
}

/// Tests WebDAV connectivity by issuing a PROPFIND request.
pub fn test_webdav_connection(
    endpoint: &str,
    remote_path: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
) -> WebDavTestResult {
    let client = match shared_client() {
        Ok(c) => c,
        Err(e) => {
            return WebDavTestResult {
                success: false,
                message: e,
                status_code: None,
            }
        }
    };

    let url = join_webdav_url(endpoint, remote_path.unwrap_or(""));
    let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<D:propfind xmlns:D="DAV:">
  <D:prop><D:resourcetype/></D:prop>
</D:propfind>"#;

    let mut request = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
        .header("Depth", "1")
        .body(body.to_string());
    if let Some((u, p)) = auth(username, password) {
        request = request.basic_auth(u, p);
    }

    match request.send() {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() || status == StatusCode::from_u16(207).unwrap() {
                WebDavTestResult {
                    success: true,
                    message: "Connection successful".to_string(),
                    status_code: Some(status.as_u16()),
                }
            } else if status == StatusCode::UNAUTHORIZED {
                WebDavTestResult {
                    success: false,
                    message: "Authentication failed: check username and password".to_string(),
                    status_code: Some(401),
                }
            } else {
                WebDavTestResult {
                    success: false,
                    message: format!("Server returned HTTP {status}"),
                    status_code: Some(status.as_u16()),
                }
            }
        }
        Err(e) => WebDavTestResult {
            success: false,
            message: format!("Network error: {e}"),
            status_code: None,
        },
    }
}

/// Uploads a file to the WebDAV server via PUT.
pub fn upload_to_webdav(
    endpoint: &str,
    remote_path: &str,
    filename: &str,
    data: Vec<u8>,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<(), String> {
    let client = shared_client()?;
    let url = join_webdav_url(
        endpoint,
        &format!("{}/{}", remote_path.trim_start_matches('/'), filename),
    );

    let mut request = client.put(&url).body(data);
    if let Some((u, p)) = auth(username, password) {
        request = request.basic_auth(u, p);
    }

    let resp = request.send().map_err(|e| format!("upload failed: {e}"))?;

    if resp.status().is_success() || resp.status() == StatusCode::CREATED {
        Ok(())
    } else {
        Err(format!(
            "upload returned HTTP {}: {}",
            resp.status(),
            resp.text()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect::<String>()
        ))
    }
}

/// Downloads a file from the WebDAV server via GET.
pub fn download_from_webdav(
    endpoint: &str,
    remote_path: &str,
    filename: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<Vec<u8>, String> {
    let client = shared_client()?;
    let url = join_webdav_url(
        endpoint,
        &format!("{}/{}", remote_path.trim_start_matches('/'), filename),
    );

    let mut request = client.get(&url);
    if let Some((u, p)) = auth(username, password) {
        request = request.basic_auth(u, p);
    }

    let resp = request
        .send()
        .map_err(|e| format!("download failed: {e}"))?;

    if resp.status().is_success() {
        Ok(resp.bytes().map_err(|e| e.to_string())?.to_vec())
    } else {
        Err(format!("download returned HTTP {}", resp.status()))
    }
}

/// Deletes a file from the WebDAV server via DELETE.
pub fn delete_from_webdav(
    endpoint: &str,
    remote_path: &str,
    filename: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<(), String> {
    let client = shared_client()?;
    let url = join_webdav_url(
        endpoint,
        &format!("{}/{}", remote_path.trim_start_matches('/'), filename),
    );

    let mut request = client.delete(&url);
    if let Some((u, p)) = auth(username, password) {
        request = request.basic_auth(u, p);
    }

    let resp = request.send().map_err(|e| format!("delete failed: {e}"))?;

    if resp.status().is_success() || resp.status() == StatusCode::NOT_FOUND {
        Ok(())
    } else {
        Err(format!("delete returned HTTP {}", resp.status()))
    }
}

/// Lists files in a WebDAV directory via PROPFIND.
pub fn list_webdav_files(
    endpoint: &str,
    remote_path: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<Vec<WebDavEntry>, String> {
    let client = shared_client()?;
    let url = join_webdav_url(endpoint, remote_path.unwrap_or(""));
    let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<D:propfind xmlns:D="DAV:">
  <D:allprop/>
</D:propfind>"#;

    let mut request = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
        .header("Depth", "1")
        .body(body.to_string());
    if let Some((u, p)) = auth(username, password) {
        request = request.basic_auth(u, p);
    }

    let resp = request.send().map_err(|e| format!("list failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() && status != StatusCode::from_u16(207).unwrap() {
        return Err(format!("list returned HTTP {status}"));
    }

    let xml = resp.text().map_err(|e| e.to_string())?;
    Ok(parse_propfind_response(&xml))
}

fn parse_propfind_response(xml: &str) -> Vec<WebDavEntry> {
    let mut entries = Vec::new();

    for (mat, _) in xml
        .match_indices("<D:response>")
        .chain(xml.match_indices("<d:response>"))
    {
        let block = &xml[mat..];
        let end = block
            .find("</D:response>")
            .or_else(|| block.find("</d:response>"))
            .unwrap_or(block.len());
        let block = &block[..end];

        let name = extract_href(block).unwrap_or_default();
        let is_dir = block.contains("<D:collection/>") || block.contains("<d:collection/>");
        let size = extract_tag(block, "D:getcontentlength")
            .or_else(|| extract_tag(block, "d:getcontentlength"))
            .and_then(|s| s.parse::<u64>().ok());
        let modified = extract_tag(block, "D:getlastmodified")
            .or_else(|| extract_tag(block, "d:getlastmodified"))
            .and_then(parse_http_date_to_ms);

        if !name.is_empty() {
            entries.push(WebDavEntry {
                name,
                is_directory: is_dir,
                size_bytes: size,
                modified_ms: modified,
            });
        }
    }

    entries
}

fn extract_href(block: &str) -> Option<String> {
    let start_pat = "<D:href>";
    let end_pat = "</D:href>";
    let s = block.find(start_pat)? + start_pat.len();
    let e = block[s..].find(end_pat)? + s;
    let raw = &block[s..e];
    raw.rsplit('/').next().map(|n| {
        urlencoding::decode(n)
            .unwrap_or(std::borrow::Cow::Borrowed(n))
            .into_owned()
    })
}

fn extract_tag<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = block.find(&open)? + open.len();
    let e = block[s..].find(&close)? + s;
    Some(&block[s..e])
}

fn parse_http_date_to_ms(date_str: &str) -> Option<i64> {
    let cleaned = date_str.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_str(cleaned, "%a, %d %b %Y %H:%M:%S GMT") {
        return Some(dt.timestamp_millis());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_str(cleaned, "%Y-%m-%dT%H:%M:%SZ") {
        return Some(dt.timestamp_millis());
    }
    None
}
