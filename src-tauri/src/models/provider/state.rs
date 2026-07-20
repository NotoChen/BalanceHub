use serde::{Deserialize, Serialize};

use super::defaults;
use crate::models::{
    default_liveness_interval, default_liveness_random_min_interval, default_liveness_timeout,
    default_true, AuthMode, LivenessCliKind, LivenessIntervalMode, LivenessPromptMode,
    LivenessRecord, ProviderCheckInRecord, ProviderNotificationMode, ProviderProxyMode,
    ProviderQuotaScope, ProviderStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub identity: ProviderIdentity,
    pub auth: ProviderAuth,
    pub quota: ProviderQuota,
    pub capabilities: ProviderCapabilities,
    pub automation: ProviderAutomation,
    pub liveness: ProviderLiveness,
    pub proxy: ProviderProxy,
    pub notification: ProviderNotification,
    pub runtime: ProviderRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentity {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub site_logo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentityInput {
    pub name: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuth {
    pub mode: AuthMode,
    pub api_key: String,
    pub access_token: String,
    pub session_cookie: String,
    pub api_user: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuota {
    pub available: f64,
    pub used: f64,
    #[serde(default)]
    pub scope: ProviderQuotaScope,
    #[serde(default)]
    pub unlimited: bool,
    #[serde(default = "defaults::quota_per_unit")]
    pub per_unit: f64,
    #[serde(default = "defaults::quota_display_type")]
    pub display_type: String,
    #[serde(default = "defaults::currency_symbol")]
    pub currency_symbol: String,
    #[serde(default = "defaults::currency_exchange_rate")]
    pub currency_exchange_rate: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    #[serde(default)]
    pub check_in_known: bool,
    #[serde(default)]
    pub check_in_supported: bool,
    #[serde(default)]
    pub check_in_auth_modes: Vec<AuthMode>,
    #[serde(default)]
    pub api_key_management_known: bool,
    #[serde(default)]
    pub api_key_management_supported: bool,
    #[serde(default)]
    pub invitation_known: bool,
    #[serde(default)]
    pub invitation_supported: bool,
    #[serde(default)]
    pub invite_link: String,
    #[serde(default)]
    pub probed_at: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub available_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAutomation {
    #[serde(default)]
    pub refresh_interval: u64,
    #[serde(default)]
    pub check_in_time: String,
    pub last_synced_at: Option<String>,
    #[serde(default)]
    pub last_checked_in_at: Option<String>,
    #[serde(default)]
    pub last_check_in_user: String,
    #[serde(default)]
    pub check_in_records: Vec<ProviderCheckInRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAutomationInput {
    #[serde(default)]
    pub refresh_interval: u64,
    #[serde(default)]
    pub check_in_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLiveness {
    #[serde(default = "default_true")]
    pub use_global: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub openai_base_url: String,
    #[serde(default)]
    pub anthropic_base_url: String,
    #[serde(default)]
    pub cli_kind: Option<LivenessCliKind>,
    #[serde(default)]
    pub interval_mode: LivenessIntervalMode,
    #[serde(default = "default_liveness_interval")]
    pub interval: u64,
    #[serde(default = "default_liveness_random_min_interval")]
    pub random_min_interval: u64,
    #[serde(default = "default_liveness_interval")]
    pub random_max_interval: u64,
    #[serde(default = "default_liveness_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompt_mode: LivenessPromptMode,
    #[serde(default)]
    pub fixed_prompt: String,
    #[serde(default)]
    pub prompt_cursor: u64,
    #[serde(default)]
    pub next_at: Option<String>,
    #[serde(default)]
    pub records: Vec<LivenessRecord>,
    /// 该中转站测活累计统计（独立持久化，不受 records 的 40 条上限影响）。
    #[serde(default)]
    pub run_count: u64,
    #[serde(default)]
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLivenessInput {
    #[serde(default = "default_true")]
    pub use_global: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub openai_base_url: String,
    #[serde(default)]
    pub anthropic_base_url: String,
    #[serde(default)]
    pub cli_kind: Option<LivenessCliKind>,
    #[serde(default)]
    pub interval_mode: LivenessIntervalMode,
    #[serde(default = "default_liveness_interval")]
    pub interval: u64,
    #[serde(default = "default_liveness_random_min_interval")]
    pub random_min_interval: u64,
    #[serde(default = "default_liveness_interval")]
    pub random_max_interval: u64,
    #[serde(default = "default_liveness_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompt_mode: LivenessPromptMode,
    #[serde(default)]
    pub fixed_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProxy {
    #[serde(default)]
    pub mode: ProviderProxyMode,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderNotification {
    #[serde(default)]
    pub mode: ProviderNotificationMode,
    #[serde(default)]
    pub channel_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRuntime {
    pub enabled: bool,
    pub status: ProviderStatus,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRuntimeInput {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuotaDisplay {
    pub quota_display_type: String,
    pub currency_symbol: String,
}

impl Default for ProviderQuotaDisplay {
    fn default() -> Self {
        Self {
            quota_display_type: defaults::quota_display_type(),
            currency_symbol: defaults::currency_symbol(),
        }
    }
}
