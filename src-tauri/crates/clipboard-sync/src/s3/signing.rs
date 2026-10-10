//! AWS SigV4 signing, endpoint parsing and request construction.

use super::*;

/// Parses the user-provided endpoint into (scheme, host).
/// Accepts `s3.amazonaws.com`, `https://s3.amazonaws.com`, `localhost:9000`,
/// `http://127.0.0.1:9000`. Defaults to https when no scheme is present.
pub(super) fn parse_endpoint(endpoint: &str) -> (String, String) {
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
pub(super) struct SigV4<'a> {
    pub(super) access_key: &'a str,
    pub(super) secret_key: &'a str,
    pub(super) region: &'a str,
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

pub(super) fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

impl<'a> SigV4<'a> {
    pub(super) fn new(
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

    pub(super) fn sign(
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
pub(super) struct S3Request<'a> {
    pub(super) method: &'a str,
    pub(super) scheme: &'a str,
    pub(super) endpoint_host: &'a str,
    pub(super) bucket: &'a str,
    pub(super) key: &'a str,
    pub(super) query: Option<String>,
    pub(super) payload: Option<&'a [u8]>,
    pub(super) access_key: &'a str,
    pub(super) secret_key: &'a str,
    pub(super) region: &'a str,
    pub(super) extra_headers: &'a [(&'a str, &'a str)],
}

/// Signs a request and returns a `RequestBuilder`.
pub(super) fn signed_request(
    client: &reqwest::Client,
    req: &S3Request,
) -> Result<RequestBuilder, String> {
    let data = req.payload.unwrap_or(&[]);
    let payload_hash = sha256_hex(data);
    signed_request_with_payload_hash(client, req, &payload_hash)
}

pub(super) fn signed_request_with_payload_hash(
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

pub(super) fn percent_encode(input: &str) -> String {
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
