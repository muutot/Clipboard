use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};

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
        }
    }
}

fn strip_whitespace(input: &str) -> String {
    input.chars().filter(|c| !c.is_whitespace()).collect()
}

fn strip_newlines(input: &str) -> String {
    input.replace('\r', "").replace('\n', " ")
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
    format!("{:x}", hasher.finalize())
}

fn sha256_hash(input: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn sha512_hash(input: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha512::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_whitespace_removes_spaces_and_tabs() {
        assert_eq!(
            TransformOperation::StripWhitespace.apply("a b\tc"),
            "abc"
        );
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
}
