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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderApiKeyOption {
    pub name: String,
    pub key: String,
    pub masked_key: String,
    pub key_available: bool,
    pub token_id: String,
    pub user_id: String,
    pub status: String,
    pub used_quota: f64,
    pub remain_quota: f64,
    pub used_quota_raw: i64,
    pub remain_quota_raw: i64,
    pub unlimited_quota: bool,
    pub group: String,
    pub cross_group_retry: bool,
    pub model_limits_enabled: bool,
    pub model_limits: Vec<String>,
    pub allow_ips: Vec<String>,
    pub quota_display_type: String,
    pub currency_symbol: String,
    pub created_time: Option<i64>,
    pub accessed_time: Option<i64>,
    pub expired_time: Option<i64>,
}

impl ProviderApiKeyOption {
    pub fn current(key: &str) -> Self {
        let key = super::normalize_api_key(key);
        Self {
            name: "当前 API Key".to_string(),
            masked_key: mask_api_key(&key),
            key_available: !key.is_empty() && !key.contains('*'),
            key,
            status: "1".to_string(),
            quota_display_type: "currency".to_string(),
            currency_symbol: "$".to_string(),
            ..Self::default()
        }
    }

    pub fn normalize(mut self) -> Self {
        self.key = super::normalize_api_key(&self.key);
        self.masked_key = self.masked_key.trim().to_string();
        self.status = self.status.trim().to_string();
        if self.masked_key.is_empty() && !self.key.is_empty() {
            self.masked_key = mask_api_key(&self.key);
        }
        self.key_available = !self.key.is_empty() && !self.key.contains('*');
        self.name = self.name.trim().to_string();
        self.token_id = self.token_id.trim().to_string();
        self.user_id = self.user_id.trim().to_string();
        self.group = self.group.trim().to_string();
        self.model_limits = normalize_string_list(self.model_limits);
        self.allow_ips = normalize_string_list(self.allow_ips);
        if self.quota_display_type.trim().is_empty() {
            self.quota_display_type = "currency".to_string();
        }
        if self.currency_symbol.trim().is_empty() {
            self.currency_symbol = "$".to_string();
        }
        self
    }

    /// Merge a previously revealed key into a fresh remote metadata snapshot.
    /// NewAPI intentionally masks keys in the list response, and older or
    /// customized deployments may reject the reveal endpoint. Keeping the
    /// locally revealed value lets a metadata refresh update quotas/limits
    /// without making an already usable key disappear.
    pub fn merge_cached_key_material(
        options: &mut [ProviderApiKeyOption],
        cached: &[ProviderApiKeyOption],
    ) {
        for option in options.iter_mut() {
            if option.key_available {
                continue;
            }
            let Some(previous) = cached.iter().find(|candidate| {
                (!option.token_id.is_empty()
                    && !candidate.token_id.is_empty()
                    && option.token_id == candidate.token_id)
                    || (option.token_id.is_empty()
                        && candidate.token_id.is_empty()
                        && !option.masked_key.is_empty()
                        && option.masked_key == candidate.masked_key)
            }) else {
                continue;
            };
            let key = super::normalize_api_key(&previous.key);
            if key.is_empty() || key.contains('*') {
                continue;
            }
            option.key = key;
            option.key_available = true;
            if option.masked_key.is_empty() {
                option.masked_key = if previous.masked_key.is_empty() {
                    mask_api_key(&option.key)
                } else {
                    previous.masked_key.clone()
                };
            }
        }
    }
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if value.is_empty() || normalized.contains(&value) {
            continue;
        }
        normalized.push(value);
    }
    normalized
}

fn mask_api_key(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() || key.contains('*') {
        return key.to_string();
    }
    let chars = key.chars().collect::<Vec<_>>();
    if chars.len() <= 4 {
        return "*".repeat(chars.len());
    }
    if chars.len() <= 8 {
        return format!(
            "{}****{}",
            chars[..2].iter().collect::<String>(),
            chars[chars.len() - 2..].iter().collect::<String>()
        );
    }
    format!(
        "{}**********{}",
        chars[..4].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfigChange {
    pub file_path: String,
    pub field_path: String,
    pub before_value: Option<String>,
    pub after_value: Option<String>,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfigPreview {
    pub provider_id: String,
    pub provider_name: String,
    pub cli_kind: LivenessCliKind,
    pub revision: String,
    pub changes: Vec<CliConfigChange>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliLaunchResult {
    pub instance: TemporaryCliInstance,
    pub workspaces: Vec<super::Workspace>,
    pub workspace_error: Option<String>,
    pub preference: super::TemporaryCliPreference,
}
