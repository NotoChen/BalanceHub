use serde::{Deserialize, Serialize};

use serde_json::Value;

use super::{
    LivenessCliKind, Provider, ProviderInput, ProviderQuotaDisplay, TemporaryCliTerminalKind,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilityProbeResult {
    pub providers: Vec<Provider>,
    pub provider: Provider,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialCompletionResult {
    pub input: ProviderInput,
    pub changed_fields: Vec<String>,
    pub steps: Vec<ProviderCredentialCompletionStep>,
    pub api_key_options: Vec<ProviderApiKeyOption>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialCompletionStep {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeyOption {
    pub name: String,
    pub key: String,
    #[serde(default)]
    pub token_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub used_quota: f64,
    #[serde(default)]
    pub remain_quota: f64,
    #[serde(default)]
    pub unlimited_quota: bool,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub model_limits_enabled: bool,
    #[serde(default)]
    pub model_limits: Vec<String>,
    #[serde(default)]
    pub allow_ips: Vec<String>,
    #[serde(default)]
    pub created_time: Option<i64>,
    #[serde(default)]
    pub accessed_time: Option<i64>,
    #[serde(default)]
    pub expired_time: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectionTestResult {
    pub ok: bool,
    pub message: String,
    pub available: Option<f64>,
    pub used: Option<f64>,
    #[serde(default)]
    pub quota_display: ProviderQuotaDisplay,
    pub steps: Vec<ProviderConnectionTestStep>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectionTestStep {
    pub name: String,
    pub ok: bool,
    pub message: String,
    pub available: Option<f64>,
    pub used: Option<f64>,
    #[serde(default)]
    pub quota_display: ProviderQuotaDisplay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub providers: Vec<Provider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCheckInResult {
    pub ok: bool,
    pub message: String,
    #[serde(rename = "lastCheckedInAt", skip_serializing_if = "Option::is_none")]
    pub last_checked_in_at: Option<String>,
    #[serde(rename = "lastCheckInUser", skip_serializing_if = "Option::is_none")]
    pub last_check_in_user: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCheckInRecord {
    pub date: String,
    pub checked_at: Option<String>,
    pub quota_delta: Option<f64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCheckInRecordsResult {
    pub provider_id: String,
    pub month: String,
    pub records: Vec<ProviderCheckInRecord>,
    pub quota_display: ProviderQuotaDisplay,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSiteProbeResult {
    pub ok: bool,
    pub message: String,
    pub system_name: Option<String>,
    pub logo: Option<String>,
    pub quota_display: ProviderQuotaDisplay,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsagePoint {
    pub date: String,
    pub used: f64,
    pub request_count: i64,
    pub token_used: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageModelStat {
    pub model_name: String,
    pub used: f64,
    pub request_count: i64,
    pub token_used: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageModelPoint {
    pub date: String,
    pub model_name: String,
    pub used: f64,
    pub request_count: i64,
    pub token_used: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageSummary {
    pub provider_id: String,
    pub provider_name: String,
    pub quota_display: ProviderQuotaDisplay,
    pub points: Vec<ProviderUsagePoint>,
    pub model_stats: Vec<ProviderUsageModelStat>,
    pub model_points: Vec<ProviderUsageModelPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequestLogsQuery {
    #[serde(default)]
    pub keyword: String,
    #[serde(default)]
    pub page: u64,
    #[serde(default = "default_request_logs_page_size")]
    pub page_size: u64,
}

impl Default for ProviderRequestLogsQuery {
    fn default() -> Self {
        Self {
            keyword: String::new(),
            page: 0,
            page_size: default_request_logs_page_size(),
        }
    }
}

fn default_request_logs_page_size() -> u64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequestLog {
    pub id: String,
    pub created_at: String,
    pub token_name: String,
    pub model_name: String,
    pub request_id: String,
    pub status: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub token_used: i64,
    pub quota: f64,
    pub channel: String,
    pub duration_ms: Option<i64>,
    pub content: String,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequestLogsResult {
    pub provider_id: String,
    pub provider_name: String,
    pub page: u64,
    pub page_size: u64,
    pub total: Option<i64>,
    pub quota_display: ProviderQuotaDisplay,
    #[serde(default)]
    pub stats: ProviderRequestLogStats,
    pub logs: Vec<ProviderRequestLog>,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequestLogStats {
    pub quota: f64,
    pub rpm: f64,
    pub tpm: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfigSnapshot {
    pub configured: bool,
    pub provider_id: Option<String>,
    pub modified_at: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemporaryCliInstanceStatus {
    Starting,
    Running,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliInstance {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub cli_kind: LivenessCliKind,
    pub workdir: String,
    pub terminal_kind: TemporaryCliTerminalKind,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub pid: Option<u32>,
    pub status: TemporaryCliInstanceStatus,
    pub exit_code: Option<i32>,
    pub can_activate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliRuntimeSnapshot {
    pub codex: CliConfigSnapshot,
    pub claude_code: CliConfigSnapshot,
    pub instances: Vec<TemporaryCliInstance>,
}
