use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OcrStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrTextBlock {
    pub text: String,
    pub confidence: f32,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrResult {
    pub item_id: String,
    pub status: OcrStatus,
    pub engine: String,
    pub model_version: String,
    pub language: Option<String>,
    pub full_text: String,
    pub blocks: Vec<OcrTextBlock>,
    pub image_hash: String,
    pub created_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub error_message: Option<String>,
}

impl OcrResult {
    pub fn completed(
        item_id: &str,
        engine: &str,
        model_version: &str,
        language: Option<&str>,
        full_text: &str,
        blocks: &[OcrTextBlock],
        image_hash: &str,
    ) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        Self {
            item_id: item_id.to_owned(),
            status: OcrStatus::Completed,
            engine: engine.to_owned(),
            model_version: model_version.to_owned(),
            language: language.map(|s| s.to_owned()),
            full_text: full_text.to_owned(),
            blocks: blocks.to_vec(),
            image_hash: image_hash.to_owned(),
            created_at_ms: now_ms,
            completed_at_ms: Some(now_ms),
            error_message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OcrStatus;

    #[test]
    fn status_names_match_frontend_contract() {
        let json = serde_json::to_string(&OcrStatus::Processing).unwrap();
        assert_eq!(json, "\"processing\"");
    }
}
