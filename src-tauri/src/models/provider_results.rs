use serde::{Deserialize, Serialize};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    AgentCliKind, Provider, ProviderInput, ProviderProtocol, ProviderQuotaDisplay,
    TemporaryCliTerminalKind,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilityProbeResult {
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderSaveOptions {
    /// 覆盖前一次重复校验返回的同账号/同 API Key 中转站。
    pub overwrite_provider_id: Option<String>,
    /// 将 API Key 追加到重复校验返回的已有中转站卡片。
    pub merge_api_key_into_provider_id: Option<String>,
    /// 在前一次同 URL、不同 API Key 冲突的卡片旁创建独立卡片。
    pub create_separate_from_provider_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSaveConflict {
    pub kind: super::ProviderDuplicateKind,
    pub existing_provider_id: String,
    pub existing_provider_name: String,
}

#[derive(Debug, Clone)]
pub struct ProviderSaveResult {
    pub saved: bool,
    pub provider: Option<Provider>,
    pub conflict: Option<ProviderSaveConflict>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRemovalResult {
    pub id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderApiKeyOption {
    /// 本机密钥库中的稳定标识。只用于选择和本地操作，不作为远程 token ID。
    #[serde(default)]
    pub local_id: String,
    /// 用户在 BalanceHub 中为这条 Key 设置的本地备注。远端同步不得覆盖。
    #[serde(default)]
    pub local_name: String,
    /// 站点返回的远程 Key 名称，与本地备注分开维护。
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
        Self::current_for_protocol(key, ProviderProtocol::NewApi)
    }

    pub fn current_for_protocol(key: &str, protocol: ProviderProtocol) -> Self {
        let key = super::normalize_api_key_for_protocol(key, protocol);
        Self {
            name: "当前 API Key".to_string(),
            masked_key: mask_api_key(&key),
            key_available: is_full_api_key_value(&key),
            key,
            status: "1".to_string(),
            quota_display_type: "currency".to_string(),
            currency_symbol: "$".to_string(),
            ..Self::default()
        }
        .normalize_for_protocol(protocol)
    }

    pub fn normalize(self) -> Self {
        self.normalize_for_protocol(ProviderProtocol::NewApi)
    }

    pub fn normalize_for_protocol(mut self, protocol: ProviderProtocol) -> Self {
        self.key = super::normalize_api_key_for_protocol(&self.key, protocol);
        self.local_id = self.local_id.trim().to_string();
        self.local_name = crate::limits::normalize_api_key_remark(&self.local_name);
        self.masked_key = self.masked_key.trim().to_string();
        self.status = self.status.trim().to_string();
        if self.masked_key.is_empty() && !self.key.is_empty() {
            self.masked_key = mask_api_key(&self.key);
        }
        self.key_available = is_full_api_key_value(&self.key);
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
        if self.local_id.is_empty() {
            self.local_id =
                provider_api_key_local_id(protocol, &self.token_id, &self.key, &self.masked_key);
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
        protocol: ProviderProtocol,
    ) {
        for option in options.iter_mut() {
            option.key_available = is_full_api_key_value(&option.key);
            let Some(previous) = cached.iter().find(|candidate| {
                (!option.local_id.is_empty()
                    && !candidate.local_id.is_empty()
                    && option.local_id == candidate.local_id)
                    || (!option.token_id.is_empty()
                        && !candidate.token_id.is_empty()
                        && option.token_id == candidate.token_id)
                    || (!option.key.is_empty()
                        && !candidate.key.is_empty()
                        && option.key == candidate.key)
                    || same_masked_key_identity(option, candidate)
            }) else {
                continue;
            };
            // A remote refresh may discover a token id for a key that was
            // originally added locally. Keep the persisted local identity so
            // UI selections and temporary CLI preferences do not drift.
            if !previous.local_id.is_empty() {
                option.local_id = previous.local_id.clone();
            }
            option.local_name = crate::limits::normalize_api_key_remark(&previous.local_name);
            if option.key_available {
                // The remote response may include the full key. We still
                // need to carry the local identity and remark across the
                // refresh before leaving this branch.
                continue;
            }
            let key = super::normalize_api_key_for_protocol(&previous.key, protocol);
            if !is_full_api_key_value(&key) {
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

fn same_masked_key_identity(
    option: &ProviderApiKeyOption,
    candidate: &ProviderApiKeyOption,
) -> bool {
    if option.masked_key.is_empty() || option.masked_key != candidate.masked_key {
        return false;
    }

    // A masked key is useful as a refresh bridge when a deployment rotates its
    // token identifier, but it must not collapse unrelated entries that happen
    // to share the same short mask. Prefer another stable piece of remote
    // metadata when both sides provide one.
    let same_user = !option.user_id.is_empty()
        && !candidate.user_id.is_empty()
        && option.user_id == candidate.user_id;
    let same_name =
        !option.name.is_empty() && !candidate.name.is_empty() && option.name == candidate.name;
    same_user || same_name || option.token_id.is_empty() || candidate.token_id.is_empty()
}

/// Return whether a value contains a complete API key rather than an empty or
/// server-redacted placeholder. This is deliberately kept in the model layer
/// so credential selection, network probes, and temporary CLI launches share
/// one safety rule instead of trusting a stale `key_available` flag.
pub(crate) fn is_full_api_key_value(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.contains('*')
}

pub(crate) fn provider_api_key_local_id(
    protocol: ProviderProtocol,
    token_id: &str,
    key: &str,
    masked_key: &str,
) -> String {
    let token_id = token_id.trim();
    let key = key.trim();
    let masked_key = masked_key.trim();
    let identity = if !key.is_empty() {
        format!("key:{key}")
    } else if !token_id.is_empty() {
        format!("token:{token_id}")
    } else if !masked_key.is_empty() {
        format!("masked:{masked_key}")
    } else {
        return String::new();
    };
    let digest = Sha256::digest(format!("{}|{identity}", protocol.key()).as_bytes());
    let suffix = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("key-{suffix}")
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
    pub updated_providers: Vec<Provider>,
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
    #[serde(rename = "quotaDelta", skip_serializing_if = "Option::is_none")]
    pub quota_delta: Option<f64>,
}

/// 批量刷新/签到通过 Tauri Channel 推送的安全展示数据。
///
/// 这里刻意不携带完整 `Provider`，避免 API Key、Cookie、访问令牌和密码进入
/// 进度事件或前端临时状态。最终完整卡片仍由批量 command 的返回值统一写回。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderBatchOperation {
    Refresh,
    CheckIn,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderBatchStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBatchDetails {
    pub username: String,
    pub user_id: String,
    pub available: f64,
    pub used: f64,
    pub known: bool,
    pub total_known: bool,
    pub quota_display_type: String,
    pub currency_symbol: String,
    pub unlimited: bool,
    pub model_count: usize,
    pub last_synced_at: Option<String>,
    pub last_checked_in_at: Option<String>,
    pub last_check_in_user: String,
    pub quota_delta: Option<f64>,
}

impl ProviderBatchDetails {
    pub fn from_provider(provider: &Provider, quota_delta: Option<f64>) -> Self {
        Self {
            username: provider.identity.username.clone(),
            user_id: provider.identity.user_id.clone(),
            available: provider.quota.available,
            used: provider.quota.used,
            known: provider.quota.known,
            total_known: provider.quota.total_known,
            quota_display_type: provider.quota.display_type.clone(),
            currency_symbol: provider.quota.currency_symbol.clone(),
            unlimited: provider.quota.unlimited,
            model_count: provider.capabilities.available_models.len(),
            last_synced_at: provider.automation.last_synced_at.clone(),
            last_checked_in_at: provider.automation.last_checked_in_at.clone(),
            last_check_in_user: provider.automation.last_check_in_user.clone(),
            quota_delta,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBatchProgressItem {
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
    pub status: ProviderBatchStatus,
    pub message: String,
    pub details: Option<ProviderBatchDetails>,
}

impl ProviderBatchProgressItem {
    pub fn pending(provider: &Provider) -> Self {
        Self::new(provider, ProviderBatchStatus::Pending, "", None)
    }

    pub fn skipped(provider: &Provider, message: impl Into<String>) -> Self {
        Self::new(
            provider,
            ProviderBatchStatus::Skipped,
            message,
            Some(ProviderBatchDetails::from_provider(provider, None)),
        )
    }

    pub fn new(
        provider: &Provider,
        status: ProviderBatchStatus,
        message: impl Into<String>,
        details: Option<ProviderBatchDetails>,
    ) -> Self {
        Self {
            provider_id: provider.identity.id.clone(),
            name: provider.display_label(),
            base_url: provider.identity.base_url.clone(),
            status,
            message: message.into(),
            details,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBatchSummary {
    pub total: usize,
    pub completed: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl ProviderBatchSummary {
    pub fn from_items(items: &[ProviderBatchProgressItem]) -> Self {
        let success = items
            .iter()
            .filter(|item| matches!(item.status, ProviderBatchStatus::Success))
            .count();
        let failed = items
            .iter()
            .filter(|item| matches!(item.status, ProviderBatchStatus::Failed))
            .count();
        let skipped = items
            .iter()
            .filter(|item| matches!(item.status, ProviderBatchStatus::Skipped))
            .count();
        Self {
            total: items.len(),
            completed: success + failed + skipped,
            success,
            failed,
            skipped,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum ProviderBatchProgressEvent {
    #[serde(rename = "started")]
    Started {
        operation: ProviderBatchOperation,
        total: usize,
        items: Vec<ProviderBatchProgressItem>,
    },
    #[serde(rename = "providerStarted")]
    ProviderStarted {
        operation: ProviderBatchOperation,
        item: ProviderBatchProgressItem,
    },
    #[serde(rename = "providerFinished")]
    ProviderFinished {
        operation: ProviderBatchOperation,
        item: ProviderBatchProgressItem,
    },
    #[serde(rename = "completed")]
    Completed {
        operation: ProviderBatchOperation,
        summary: ProviderBatchSummary,
    },
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
pub struct ProviderProtocolDetectionResult {
    pub detected_protocol: Option<ProviderProtocol>,
    pub message: String,
    pub site: Option<ProviderSiteProbeResult>,
    pub ambiguous: bool,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelSyncResult {
    pub provider: Provider,
    pub models: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfigSnapshot {
    pub cli_kind: AgentCliKind,
    pub configured: bool,
    pub provider_id: Option<String>,
    pub modified_at: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfigFile {
    pub file_path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfigPreview {
    pub provider_id: String,
    pub provider_name: String,
    pub cli_kind: AgentCliKind,
    pub revision: String,
    pub original_files: Vec<CliConfigFile>,
    pub files: Vec<CliConfigFile>,
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
    /// 启动时记录的会话标题快照，不依赖之后重新读取 CLI 历史索引。
    #[serde(default)]
    pub session_title: String,
    /// 启动时记录的非敏感账号展示快照（用户名、用户 ID 或 API Key 标签）。
    #[serde(default)]
    pub account_label: String,
    pub cli_kind: AgentCliKind,
    pub workdir: String,
    pub terminal_kind: TemporaryCliTerminalKind,
    pub terminal_name: String,
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
    /// Rust Agent 目录是汇总入口的唯一顺序和命名来源。
    #[serde(default)]
    pub agents: Vec<crate::models::AgentCliDescriptor>,
    pub configs: Vec<CliConfigSnapshot>,
    pub instances: Vec<TemporaryCliInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteAnnouncement {
    /// 站点接口返回的公告 ID；无稳定 ID 的协议使用标题和正文摘要生成本地 ID。
    pub id: String,
    /// 协议、站点及公告内容共同生成的稳定指纹，用于站点级去重和本地已读状态。
    pub fingerprint: String,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_protocol: ProviderProtocol,
    pub title: String,
    pub content: String,
    pub published_at: Option<String>,
    pub updated_at: Option<String>,
    pub read_at: Option<String>,
    pub can_mark_read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteAnnouncementSourceError {
    pub provider_id: String,
    pub provider_name: String,
    pub provider_protocol: ProviderProtocol,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteAnnouncementsSnapshot {
    pub fetched_at: String,
    pub announcements: Vec<SiteAnnouncement>,
    pub errors: Vec<SiteAnnouncementSourceError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliLaunchResult {
    pub instance: TemporaryCliInstance,
    pub workspaces: Vec<super::Workspace>,
    pub workspace_error: Option<String>,
    pub preference: super::TemporaryCliPreference,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_key_local_id_does_not_depend_on_remote_token_id() {
        let first = ProviderApiKeyOption {
            key: "sk-stable-secret".to_string(),
            token_id: "old-token-id".to_string(),
            ..ProviderApiKeyOption::default()
        }
        .normalize_for_protocol(ProviderProtocol::NewApi);
        let refreshed = ProviderApiKeyOption {
            key: "sk-stable-secret".to_string(),
            token_id: "new-token-id".to_string(),
            ..ProviderApiKeyOption::default()
        }
        .normalize_for_protocol(ProviderProtocol::NewApi);

        assert_eq!(first.local_id, refreshed.local_id);
    }

    #[test]
    fn cached_key_merge_keeps_local_identity_and_remark_after_remote_refresh() {
        let mut cached = ProviderApiKeyOption::current("sk-stable-secret");
        cached.token_id = "old-token-id".to_string();
        cached.name = "Remote name".to_string();
        cached.local_name = "我的备用 Key".to_string();
        let stable_local_id = cached.local_id.clone();
        let mut refreshed = vec![ProviderApiKeyOption {
            name: cached.name.clone(),
            local_name: "远端不应写入这里".to_string(),
            key: "sk-stable-secret".to_string(),
            masked_key: cached.masked_key.clone(),
            token_id: "new-token-id".to_string(),
            ..ProviderApiKeyOption::default()
        }
        .normalize_for_protocol(ProviderProtocol::NewApi)];

        ProviderApiKeyOption::merge_cached_key_material(
            &mut refreshed,
            &[cached],
            ProviderProtocol::NewApi,
        );

        assert_eq!(refreshed[0].local_id, stable_local_id);
        assert_eq!(refreshed[0].local_name, "我的备用 Key");
        assert_eq!(refreshed[0].key, "sk-stable-secret");
        assert!(refreshed[0].key_available);
    }

    #[test]
    fn batch_progress_event_uses_frontend_event_names() {
        let provider = Provider::from_input(ProviderInput::default(), "provider-1".to_string());
        let event = ProviderBatchProgressEvent::Started {
            operation: ProviderBatchOperation::CheckIn,
            total: 1,
            items: vec![ProviderBatchProgressItem::pending(&provider)],
        };
        let value = serde_json::to_value(event).expect("batch event should serialize");
        assert_eq!(value["event"], "started");
        assert_eq!(value["data"]["operation"], "checkIn");
        assert_eq!(value["data"]["items"][0]["status"], "pending");
        assert!(value["data"]["items"][0]["providerId"] == "provider-1");
    }
}
