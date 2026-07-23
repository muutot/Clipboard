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

#[cfg(test)]
mod tests {
    use super::OcrStatus;

    #[test]
    fn status_names_match_frontend_contract() {
        let json = serde_json::to_string(&OcrStatus::Processing).unwrap();
        assert_eq!(json, "\"processing\"");
    }
}
