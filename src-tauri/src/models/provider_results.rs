use serde::{Deserialize, Serialize};

use serde_json::Value;

use super::{Provider, ProviderInput, ProviderQuotaDisplay};

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
pub struct ProviderUsageSummary {
    pub provider_id: String,
    pub provider_name: String,
    pub quota_display: ProviderQuotaDisplay,
    pub points: Vec<ProviderUsagePoint>,
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
    pub logs: Vec<ProviderRequestLog>,
    pub message: String,
}
