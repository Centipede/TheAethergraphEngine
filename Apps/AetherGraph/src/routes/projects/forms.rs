use crate::app_settings::OcrCommandFormat;
use crate::ocr_poll::ServerStatus;
use serde::{Deserialize, Serialize};
use crate::hocr_parser::{HocrBbox, AddBlockType, DetectionThresholds};

#[derive(Deserialize, Serialize, Clone)]
pub struct OcrServerData {
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize, Serialize)]
pub struct OcrSettingsUpdate {
    #[serde(default)]
    pub server_1: Option<OcrServerData>,
    #[serde(default)]
    pub server_2: Option<OcrServerData>,
    pub command_format: OcrCommandFormat,
}

#[derive(Deserialize, Serialize)]
pub struct SettingsUpdate {
    #[serde(default)]
    pub openai_api_key: Option<String>,
    #[serde(default)]
    pub perplexity_api_key: Option<String>,
    #[serde(default)]
    pub ocr: Option<OcrSettingsUpdate>,
}

#[derive(Deserialize, Serialize)]
pub struct SettingsResponse {
    pub openai_api_key_set: bool,
    pub perplexity_api_key_set: bool,
    pub ocr_server_1: Option<OcrServerData>,
    pub ocr_server_2: Option<OcrServerData>,
    pub ocr_command_format: OcrCommandFormat,
    pub ocr_server_1_status: ServerStatus,
    pub ocr_server_2_status: ServerStatus,
}


fn default_language() -> String {
    "eng".to_string()
}

#[derive(Deserialize, Serialize)]
pub struct ScanRequest {
    pub indices: Vec<usize>,
    #[serde(default)]
    pub force: bool,
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Deserialize, Serialize)]
pub struct RescanRequest {
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ScanPageResult {
    pub scan: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, PartialEq)]
pub enum JobState {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ScanJobStatus {
    pub total_pages: usize,
    pub processed_pages: usize,
    pub status: JobState,
    pub results: Vec<ScanPageResult>,
    pub error: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct ScanResponse {
    pub results: Vec<ScanPageResult>,
}

#[derive(Deserialize, Serialize)]
pub struct ScanConflict {
    pub pages: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct CreateProject {
    pub name: String,
    pub machine_name: String,
}

#[derive(Deserialize, Serialize)]
pub struct IngestQuery {
    pub after: Option<usize>,
    pub before: Option<usize>,
}

#[derive(Deserialize, Serialize)]
pub struct AddRequest {
    pub to_carea: Option<String>,
    pub to_block: Option<String>,
    pub to_line: Option<String>,
    pub bbox: HocrBbox,
    pub block_type: Option<AddBlockType>,
    pub text: Option<String>,
    pub shrink_wrap_carea: Option<bool>,
    pub erase_underneath: Option<bool>,
    pub erase_overlap: Option<u8>
}

#[derive(Deserialize, Serialize)]
pub struct RemoveRequest {
    pub indices: Vec<usize>,
}

#[derive(Deserialize, Serialize)]
pub struct MergeRequest {
    pub other_id: String,
}

#[derive(Deserialize, Serialize)]
pub struct MergeItemsRequest {
    pub item_ids: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct SplitRequest {
    pub before_id: String,
    pub after_id: String,
}

#[derive(Deserialize, Serialize)]
pub struct MorphRequest {
    #[serde(default)]
    pub item_ids: Vec<String>,
    pub turn_into: String,
    #[serde(default)]
    pub merge: bool,
}

#[derive(Deserialize, Serialize)]
pub struct AutoAssistRequest {
    pub stems: Vec<String>,
    #[serde(default)]
    pub carea_ids: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize)]
pub struct AutoBridgeRequest {
    pub thresholds: DetectionThresholds,
    pub flow_set: Option<Vec<String>>,
}
/// Where an agent says it is working, as polled by the UI so the view can follow along.
///
/// `age_seconds` exists so the UI can ignore stale focus: without it, an agent that stops
/// mid-task would leave the "agent is driving" indicator lit and the view hijacked.
#[derive(Serialize)]
pub struct AgentFocusResponse {
    pub machine_name: String,
    /// Scan stem, which is also the `:page` route param.
    pub page: String,
    pub mode: String,
    pub seq: u64,
    pub age_seconds: u64,
}
