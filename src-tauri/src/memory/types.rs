use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDiagnostics {
    pub sampled_at_ms: u64,
    pub current_process: MemoryProcess,
    pub process_group: MemoryProcessGroup,
    pub system: SystemMemory,
    pub ocr: OcrMemoryDiagnostics,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProcess {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub role: Option<String>,
    /// Resident pages currently mapped into the process, including shared
    /// pages.  This is the Windows `WorkingSetSize` / Linux `VmRSS` value.
    pub working_set_bytes: Option<u64>,
    /// Private committed bytes on Windows (`PrivateUsage`).  Platforms that
    /// do not expose an equivalent safely return `null`.
    pub private_bytes: Option<u64>,
    /// Windows 10+ exposes private resident pages separately from private
    /// commit.  Older Windows versions and other platforms may return null.
    pub private_working_set_bytes: Option<u64>,
    /// Windows reports private committed memory (commit charge) here rather
    /// than the full reserved address space.  Linux reports `VmSize`.  Other
    /// platforms may return `null` when an equivalent safe probe is absent.
    pub virtual_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProcessGroup {
    pub working_set_bytes: u64,
    pub private_bytes: u64,
    pub virtual_bytes: u64,
    pub processes: Vec<MemoryProcess>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMemory {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrMemoryDiagnostics {
    pub engine: String,
    pub model_variant: String,
    pub model_bytes: u64,
    pub model_file_count: u64,
    pub model_directory: String,
    /// Whether PP-OCR is selected and all files for the configured model
    /// variant are present. OCR loads the model lazily, so this is a readiness
    /// signal rather than a claim that the ONNX graph is resident right now.
    pub loaded: bool,
    pub installed_variants: Vec<String>,
}
