use serde::{Deserialize, Serialize};

use super::TemporaryCliTerminalKind;

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
    /// 实际发给上游的模型名（已应用 wireName 映射）。
    pub model: String,
    /// 用户配置的模型 ID；旧记录没有该字段，前端会退回 `model`。
    #[serde(default)]
    pub model_id: String,
    /// CLI 实际请求的地址（已按 Agent 端点规则归一化）。
    pub base_url: String,
    /// 归一化之前的站点地址；旧记录没有该字段，前端会退回 `base_url`。
    #[serde(default)]
    pub provider_base_url: String,
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
pub struct TemporaryTerminalProbeResult {
    pub available: bool,
    pub kind: TemporaryCliTerminalKind,
    pub name: String,
    pub version: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEnvironmentProbeResult {
    pub terminals: Vec<TemporaryTerminalProbeResult>,
}

fn default_liveness_record_source() -> String {
    "manual".to_string()
}
