use serde::{Deserialize, Serialize};

const CLIPBOARD_FORMATS_METADATA_KEY: &str = "clipboardFormats";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardFormat {
    PlainText,
    RichText,
    Html,
    Image,
    FileList,
    Unknown(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardFormatInfo {
    #[serde(default)]
    pub raw_formats: Vec<String>,
    #[serde(default)]
    pub mime_types: Vec<String>,
    #[serde(default)]
    pub available_formats: Vec<ClipboardFormat>,
}

impl ClipboardFormatInfo {
    pub fn empty() -> Self {
        Self {
            raw_formats: Vec::new(),
            mime_types: Vec::new(),
            available_formats: Vec::new(),
        }
    }
}

pub fn parse_mime_types(raw_formats: &[String]) -> ClipboardFormatInfo {
    detect_formats_from_mime_list(raw_formats)
}

pub trait ClipboardFormatReader {
    fn read_format(&self, format: ClipboardFormat) -> Result<Vec<u8>, String>;
}

pub fn detect_formats_from_mime_list(mime_list: &[String]) -> ClipboardFormatInfo {
    let mut formats = Vec::new();
    let mut raw_formats = Vec::new();
    let mut mime_types = Vec::new();

    for raw_format in mime_list {
        let trimmed = raw_format.trim();
        if trimmed.is_empty()
            || raw_formats
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        raw_formats.push(trimmed.to_owned());

        let normalized = trimmed.to_lowercase();
        let normalized_base = normalized
            .split(';')
            .next()
            .map(str::trim)
            .unwrap_or_default();
        let canonical_mime = match normalized_base {
            "text/plain" | "utf8_string" | "text" | "cf_text" | "cf_oemtext" | "cf_unicodetext" => {
                Some("text/plain")
            }
            "text/rtf" | "text/richtext" | "rich text format" => Some("text/rtf"),
            "text/html" | "html format" => Some("text/html"),
            "image/png" | "png" => Some("image/png"),
            "image/jpeg" => Some("image/jpeg"),
            "image/gif" => Some("image/gif"),
            "image/webp" => Some("image/webp"),
            "image/tiff" | "cf_tiff" => Some("image/tiff"),
            "image/bmp" | "cf_bitmap" | "cf_dib" | "cf_dibv5" => Some("image/bmp"),
            "text/uri-list" | "cf_hdrop" | "filelist" | "files" => Some("text/uri-list"),
            other if other.contains('/') && !other.chars().any(char::is_whitespace) => {
                Some(trimmed)
            }
            _ => None,
        };
        if let Some(mime) = canonical_mime {
            if !mime_types
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(mime))
            {
                mime_types.push(mime.to_owned());
            }
        }

        match normalized_base {
            "text/plain" | "utf8_string" | "text" | "cf_text" | "cf_oemtext" | "cf_unicodetext" => {
                if !formats.contains(&ClipboardFormat::PlainText) {
                    formats.push(ClipboardFormat::PlainText);
                }
            }
            "text/rtf" | "text/richtext" | "rich text format" => {
                if !formats.contains(&ClipboardFormat::RichText) {
                    formats.push(ClipboardFormat::RichText);
                }
            }
            "text/html" | "html format" => {
                if !formats.contains(&ClipboardFormat::Html) {
                    formats.push(ClipboardFormat::Html);
                }
            }
            "image/png" | "png" | "image/jpeg" | "image/bmp" | "image/gif" | "image/webp"
            | "image/tiff" | "cf_tiff" | "cf_bitmap" | "cf_dib" | "cf_dibv5" => {
                if !formats.contains(&ClipboardFormat::Image) {
                    formats.push(ClipboardFormat::Image);
                }
            }
            "text/uri-list" | "cf_hdrop" | "filelist" | "files" => {
                if !formats.contains(&ClipboardFormat::FileList) {
                    formats.push(ClipboardFormat::FileList);
                }
            }
            _ => {
                formats.push(ClipboardFormat::Unknown(trimmed.to_owned()));
            }
        }
    }

    ClipboardFormatInfo {
        raw_formats,
        mime_types,
        available_formats: formats,
    }
}

pub fn merge_clipboard_format_metadata(
    metadata_json: Option<&str>,
    raw_formats: &[String],
) -> Result<Option<String>, String> {
    if raw_formats.is_empty() {
        return Ok(metadata_json.map(str::to_owned));
    }

    let mut metadata = metadata_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    metadata[CLIPBOARD_FORMATS_METADATA_KEY] =
        serde_json::to_value(detect_formats_from_mime_list(raw_formats))
            .map_err(|error| error.to_string())?;
    Ok(Some(metadata.to_string()))
}

pub fn clipboard_format_info_from_metadata(metadata_json: Option<&str>) -> ClipboardFormatInfo {
    metadata_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|metadata| metadata.get(CLIPBOARD_FORMATS_METADATA_KEY).cloned())
        .and_then(|formats| serde_json::from_value(formats).ok())
        .unwrap_or_else(ClipboardFormatInfo::empty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plain_text() {
        let info = detect_formats_from_mime_list(&["text/plain".to_string()]);
        assert_eq!(info.available_formats, vec![ClipboardFormat::PlainText]);
        assert_eq!(info.raw_formats, vec!["text/plain"]);
        assert_eq!(info.mime_types, vec!["text/plain"]);
    }

    #[test]
    fn detects_multiple_formats() {
        let info = detect_formats_from_mime_list(&[
            "text/plain".to_string(),
            "text/html".to_string(),
            "image/png".to_string(),
        ]);
        assert!(info.available_formats.contains(&ClipboardFormat::PlainText));
        assert!(info.available_formats.contains(&ClipboardFormat::Html));
        assert!(info.available_formats.contains(&ClipboardFormat::Image));
    }

    #[test]
    fn deduplicates_same_format_type() {
        let info = detect_formats_from_mime_list(&[
            "text/plain".to_string(),
            "text/plain;charset=utf-8".to_string(),
        ]);
        assert_eq!(info.available_formats.len(), 1);
    }

    #[test]
    fn recognizes_mime_parameters_with_optional_spacing() {
        let info = detect_formats_from_mime_list(&[
            "text/plain; charset=utf-8".to_owned(),
            "text/html; charset=utf-8".to_owned(),
        ]);

        assert_eq!(
            info.available_formats,
            vec![ClipboardFormat::PlainText, ClipboardFormat::Html]
        );
        assert_eq!(info.mime_types, vec!["text/plain", "text/html"]);
    }

    #[test]
    fn unknown_format_is_captured() {
        let info = detect_formats_from_mime_list(&["application/x-custom".to_string()]);
        assert_eq!(
            info.available_formats,
            vec![ClipboardFormat::Unknown("application/x-custom".to_string())]
        );
    }

    #[test]
    fn empty_list_returns_no_formats() {
        let info = detect_formats_from_mime_list(&[]);
        assert!(info.available_formats.is_empty());
    }

    #[test]
    fn parse_mime_types_delegates() {
        let info = parse_mime_types(&["text/plain".to_string()]);
        assert_eq!(info.available_formats, vec![ClipboardFormat::PlainText]);
    }

    #[test]
    fn recognizes_windows_predefined_clipboard_formats() {
        let info = parse_mime_types(&[
            "CF_UNICODETEXT".to_owned(),
            "CF_DIBV5".to_owned(),
            "CF_HDROP".to_owned(),
        ]);

        assert!(info.available_formats.contains(&ClipboardFormat::PlainText));
        assert!(info.available_formats.contains(&ClipboardFormat::Image));
        assert!(info.available_formats.contains(&ClipboardFormat::FileList));
        assert_eq!(
            info.mime_types,
            vec!["text/plain", "image/bmp", "text/uri-list"]
        );
    }

    #[test]
    fn recognizes_registered_windows_rich_formats() {
        let info = parse_mime_types(&[
            "HTML Format".to_owned(),
            "Rich Text Format".to_owned(),
            "PNG".to_owned(),
        ]);

        assert_eq!(
            info.available_formats,
            vec![
                ClipboardFormat::Html,
                ClipboardFormat::RichText,
                ClipboardFormat::Image
            ]
        );
        assert_eq!(info.mime_types, vec!["text/html", "text/rtf", "image/png"]);
    }

    #[test]
    fn deduplicates_raw_format_names_case_insensitively() {
        let info = parse_mime_types(&[
            "CF_UNICODETEXT".to_owned(),
            "cf_unicodetext".to_owned(),
            "  CF_DIBV5  ".to_owned(),
        ]);

        assert_eq!(info.raw_formats, vec!["CF_UNICODETEXT", "CF_DIBV5"]);
        assert_eq!(
            info.available_formats,
            vec![ClipboardFormat::PlainText, ClipboardFormat::Image]
        );
    }

    #[test]
    fn clipboard_format_metadata_preserves_resource_fields() {
        let metadata = merge_clipboard_format_metadata(
            Some(r#"{"width":120,"resourcePath":"image/test.png"}"#),
            &["CF_DIBV5".to_owned(), "PNG".to_owned()],
        )
        .unwrap()
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&metadata).unwrap();

        assert_eq!(value["width"], 120);
        assert_eq!(value["resourcePath"], "image/test.png");
        assert_eq!(value["clipboardFormats"]["rawFormats"][0], "CF_DIBV5");
    }

    #[test]
    fn reads_clipboard_format_info_from_metadata() {
        let metadata = merge_clipboard_format_metadata(
            None,
            &["CF_UNICODETEXT".to_owned(), "HTML Format".to_owned()],
        )
        .unwrap();
        let info = clipboard_format_info_from_metadata(metadata.as_deref());

        assert_eq!(
            info.available_formats,
            vec![ClipboardFormat::PlainText, ClipboardFormat::Html]
        );
        assert_eq!(info.mime_types, vec!["text/plain", "text/html"]);
    }
}
