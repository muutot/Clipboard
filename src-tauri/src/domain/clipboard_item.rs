use serde::{Deserialize, Serialize};

/// Clipboard content categories shared by storage, platform adapters and the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardKind {
    Text,
    Link,
    Image,
    File,
}

/// The stable, storage-facing representation of one clipboard history entry.
/// Large binary payloads are stored outside SQLite and referenced by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardItem {
    pub id: String,
    pub kind: ClipboardKind,
    pub title: String,
    pub text_content: Option<String>,
    /// Optional HTML/rich-text fragment captured alongside plain text.
    /// `#[serde(default)]` keeps imports of older JSON exports compatible.
    #[serde(default)]
    pub html_content: Option<String>,
    /// Optional RTF fragment captured alongside rich text. Office suites
    /// (Word/Outlook) render RTF over HTML, so carrying it lets formatted
    /// paste survive those apps instead of degrading to plain text.
    /// `#[serde(default)]` keeps imports of older JSON exports compatible.
    #[serde(default)]
    pub rtf_content: Option<String>,
    pub resource_path: Option<String>,
    pub preview_path: Option<String>,
    pub content_hash: String,
    pub source_app: Option<String>,
    pub icon_path: Option<String>,
    pub size_bytes: u64,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
    pub is_favorite: bool,
    pub metadata_json: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::ClipboardKind;

    #[test]
    fn clipboard_kind_uses_frontend_friendly_names() {
        let json = serde_json::to_string(&ClipboardKind::Image).unwrap();
        assert_eq!(json, "\"image\"");
    }
}
