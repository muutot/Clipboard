use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextTransform {
    pub input: String,
    pub operation: TransformOperation,
    pub result: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransformOperation {
    StripWhitespace,
    StripNewlines,
    ToUpperCase,
    ToLowerCase,
    JsonFormat,
    Base64Encode,
    Base64Decode,
    UrlEncode,
    UrlDecode,
    Md5,
    Sha256,
    Sha512,
    TrimWhitespace,
    CollapseWhitespace,
    StripUrlTrackingParams,
    CleanPaste,
}

impl TransformOperation {
    pub fn apply(&self, input: &str) -> String {
        match self {
            TransformOperation::StripWhitespace => strip_whitespace(input),
            TransformOperation::StripNewlines => strip_newlines(input),
            TransformOperation::ToUpperCase => input.to_uppercase(),
            TransformOperation::ToLowerCase => input.to_lowercase(),
            TransformOperation::JsonFormat => json_format(input),
            TransformOperation::Base64Encode => STANDARD.encode(input.as_bytes()),
            TransformOperation::Base64Decode => base64_decode(input),
            TransformOperation::UrlEncode => urlencoding::encode(input).into_owned(),
            TransformOperation::UrlDecode => urlencoding_decode(input),
            TransformOperation::Md5 => md5_hash(input),
            TransformOperation::Sha256 => sha256_hash(input),
            TransformOperation::Sha512 => sha512_hash(input),
            TransformOperation::TrimWhitespace => trim_whitespace(input),
            TransformOperation::CollapseWhitespace => collapse_whitespace(input),
            TransformOperation::StripUrlTrackingParams => strip_url_tracking_params(input),
            TransformOperation::CleanPaste => clean_paste(input),
        }
    }
}

fn strip_whitespace(input: &str) -> String {
    input.chars().filter(|c| !c.is_whitespace()).collect()
}

fn strip_newlines(input: &str) -> String {
    input.replace('\r', "").replace('\n', " ")
}

fn trim_whitespace(input: &str) -> String {
    input.trim().to_owned()
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

static RE_URL_IN_TEXT: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"https?://[^\s<>{}|\^`\[\]]+").unwrap());

/// URL query keys that are advertising/analytics trackers rather than content.
fn is_tracking_key(key: &str) -> bool {
    const TRACKING_KEYS: &[&str] = &[
        "fbclid", "gclid", "msclkid", "twclid", "igshid", "dclid", "yclid", "scid", "wbraid",
        "gbraid", "mc_cid", "mc_eid", "_hsenc", "_hsmi", "__hssc", "__hstc", "__hsfp", "spm",
        "ref", "ref_src", "ref_url",
    ];
    key.starts_with("utm_") || TRACKING_KEYS.contains(&key)
}

/// Removes known tracking parameters from a single URL, preserving parameter
/// order and any fragment.
fn strip_url_tracking_params_from_url(url: &str) -> String {
    let Some(question_index) = url.find('?') else {
        return url.to_owned();
    };

    let (scheme_and_path, query_and_fragment) = url.split_at(question_index);
    let query_and_fragment = &query_and_fragment[1..];

    let (query, fragment) = match query_and_fragment.find('#') {
        Some(index) => (&query_and_fragment[..index], &query_and_fragment[index..]),
        None => (query_and_fragment, ""),
    };

    let kept = query
        .split('&')
        .filter(|parameter| {
            let key = parameter.split('=').next().unwrap_or("");
            let decoded = urlencoding::decode(key)
                .map(|value| value.into_owned())
                .unwrap_or_else(|_| key.to_owned());
            !is_tracking_key(&decoded.to_ascii_lowercase())
        })
        .collect::<Vec<_>>();

    if kept.is_empty() {
        return format!("{scheme_and_path}{fragment}");
    }

    format!("{scheme_and_path}?{}{fragment}", kept.join("&"))
}

fn strip_url_tracking_params(input: &str) -> String {
    RE_URL_IN_TEXT
        .replace_all(input, |captures: &regex_lite::Captures<'_>| {
            let url = captures.get(0).map(|m| m.as_str()).unwrap_or("");
            let cleaned = strip_url_tracking_params_from_url(url);
            if cleaned == url {
                url.to_owned()
            } else {
                cleaned
            }
        })
        .into_owned()
}

/// Paste cleaning pipeline: trim surrounding whitespace, collapse internal
/// whitespace runs to a single space, then remove URL tracking parameters.
pub fn clean_paste(input: &str) -> String {
    let trimmed = trim_whitespace(input);
    let collapsed = collapse_whitespace(&trimmed);
    strip_url_tracking_params(&collapsed)
}

fn json_format(input: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(input) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| input.to_owned()),
        Err(_) => input.to_owned(),
    }
}

fn base64_decode(input: &str) -> String {
    STANDARD
        .decode(input.trim())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|_| input.to_owned())
}

fn urlencoding_decode(input: &str) -> String {
    urlencoding::decode(input)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| input.to_owned())
}

fn md5_hash(input: &str) -> String {
    use md5::Digest;
    let mut hasher = md5::Md5::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

fn sha256_hash(input: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

fn sha512_hash(input: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha512::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_whitespace_removes_spaces_and_tabs() {
        assert_eq!(TransformOperation::StripWhitespace.apply("a b\tc"), "abc");
    }

    #[test]
    fn strip_newlines_replaces_with_space() {
        assert_eq!(
            TransformOperation::StripNewlines.apply("line1\r\nline2"),
            "line1 line2"
        );
    }

    #[test]
    fn to_upper_and_lower() {
        assert_eq!(TransformOperation::ToUpperCase.apply("hello"), "HELLO");
        assert_eq!(TransformOperation::ToLowerCase.apply("HELLO"), "hello");
    }

    #[test]
    fn json_format_pretty_prints() {
        let input = r#"{"a":1,"b":2}"#;
        let result = TransformOperation::JsonFormat.apply(input);
        assert!(result.contains('\n'));
    }

    #[test]
    fn json_format_invalid_returns_original() {
        let input = "not json";
        assert_eq!(TransformOperation::JsonFormat.apply(input), input);
    }

    #[test]
    fn base64_encode_decode_roundtrip() {
        let original = "hello world";
        let encoded = TransformOperation::Base64Encode.apply(original);
        let decoded = TransformOperation::Base64Decode.apply(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn url_encode_decode_roundtrip() {
        let original = "hello world & more";
        let encoded = TransformOperation::UrlEncode.apply(original);
        let decoded = TransformOperation::UrlDecode.apply(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn hash_operations_produce_hex() {
        let md5 = TransformOperation::Md5.apply("hello");
        assert_eq!(md5.len(), 32);

        let sha256 = TransformOperation::Sha256.apply("hello");
        assert_eq!(sha256.len(), 64);

        let sha512 = TransformOperation::Sha512.apply("hello");
        assert_eq!(sha512.len(), 128);
    }

    #[test]
    fn trim_whitespace_removes_leading_and_trailing_whitespace() {
        assert_eq!(
            TransformOperation::TrimWhitespace.apply("  hello world\r\n"),
            "hello world"
        );
        assert_eq!(TransformOperation::TrimWhitespace.apply("  "), "");
    }

    #[test]
    fn collapse_whitespace_replaces_runs_with_single_space() {
        assert_eq!(
            TransformOperation::CollapseWhitespace.apply("a  b\t\tc\r\nd"),
            "a b c d"
        );
        assert_eq!(
            TransformOperation::CollapseWhitespace.apply("   spaced   "),
            "spaced"
        );
    }

    #[test]
    fn strip_url_tracking_params_removes_common_tracking_params() {
        let url = "https://example.com/page?a=1&utm_source=news&fbclid=abc&ref=nav&b=2";
        assert_eq!(
            TransformOperation::StripUrlTrackingParams.apply(url),
            "https://example.com/page?a=1&b=2"
        );
    }

    #[test]
    fn strip_url_tracking_params_keeps_urls_without_query() {
        let url = "https://example.com/page";
        assert_eq!(TransformOperation::StripUrlTrackingParams.apply(url), url);
    }

    #[test]
    fn strip_url_tracking_params_preserves_fragment_and_order() {
        let url = "https://example.com/?utm_medium=email&keep=1#section";
        assert_eq!(
            TransformOperation::StripUrlTrackingParams.apply(url),
            "https://example.com/?keep=1#section"
        );
    }

    #[test]
    fn clean_paste_trims_collapses_and_strips_tracking_params() {
        let input = "  https://example.com/page?utm_campaign=x&id=7   \r\n";
        assert_eq!(
            TransformOperation::CleanPaste.apply(input),
            "https://example.com/page?id=7"
        );
        assert_eq!(
            TransformOperation::CleanPaste.apply("  plain  text  "),
            "plain text"
        );
    }
}
