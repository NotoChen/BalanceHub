use serde::{Deserialize, Serialize};

use super::Provider;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivenessRecord {
    pub checked_at: String,
    #[serde(default = "default_liveness_record_source")]
    pub source: String,
    #[serde(default)]
    pub cli_kind: String,
    pub ok: bool,
    pub latency_ms: u128,
    pub model: String,
    pub base_url: String,
    pub prompt: String,
    pub response_preview: String,
    #[serde(default)]
    pub response_raw: String,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_output_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    pub message: String,
    pub command_preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivenessRunResult {
    pub providers: Vec<Provider>,
    pub provider: Provider,
    pub record: LivenessRecord,
    pub codex_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliProbeResult {
    pub path: String,
    pub version: String,
}

/// 一个测活 CLI 候选可执行文件：枚举所有存在的候选时用于在 UI 标注来源/版本/有效性。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCandidate {
    pub path: String,
    pub version: Option<String>,
    pub valid: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelSyncResult {
    pub providers: Vec<Provider>,
    pub provider: Provider,
    pub models: Vec<String>,
    pub message: String,
}

fn default_liveness_record_source() -> String {
    "manual".to_string()
}
