use serde::{Deserialize, Serialize};

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
    pub mime_types: Vec<String>,
    pub available_formats: Vec<ClipboardFormat>,
}

impl ClipboardFormatInfo {
    pub fn empty() -> Self {
        Self {
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
    let mime_types = mime_list.to_vec();

    for mime in mime_list {
        match mime.to_lowercase().as_str() {
            "text/plain" | "text/plain;charset=utf-8" | "utf8_string" | "text" => {
                if !formats.contains(&ClipboardFormat::PlainText) {
                    formats.push(ClipboardFormat::PlainText);
                }
            }
            "text/rtf" | "text/richtext" | "rich text format" => {
                if !formats.contains(&ClipboardFormat::RichText) {
                    formats.push(ClipboardFormat::RichText);
                }
            }
            "text/html" | "text/html;charset=utf-8" | "html format" => {
                if !formats.contains(&ClipboardFormat::Html) {
                    formats.push(ClipboardFormat::Html);
                }
            }
            "image/png" | "image/jpeg" | "image/bmp" | "image/gif" | "image/webp"
            | "image/tiff" => {
                if !formats.contains(&ClipboardFormat::Image) {
                    formats.push(ClipboardFormat::Image);
                }
            }
            "text/uri-list" | "cf_hdrop" | "filelist" | "files" => {
                if !formats.contains(&ClipboardFormat::FileList) {
                    formats.push(ClipboardFormat::FileList);
                }
            }
            other => {
                formats.push(ClipboardFormat::Unknown(other.to_owned()));
            }
        }
    }

    ClipboardFormatInfo {
        mime_types,
        available_formats: formats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plain_text() {
        let info = detect_formats_from_mime_list(&["text/plain".to_string()]);
        assert_eq!(info.available_formats, vec![ClipboardFormat::PlainText]);
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
}
