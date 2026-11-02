use super::AgentCliKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OAuthVendor {
    Codex,
    Claude,
    Gemini,
    Grok,
}

impl OAuthVendor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Grok => "Grok",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Grok => "grok",
        }
    }
    pub fn base_url(self) -> &'static str {
        match self {
            Self::Codex => "https://chatgpt.com",
            Self::Claude => "https://api.anthropic.com",
            Self::Gemini => "https://cloudcode-pa.googleapis.com",
            Self::Grok => "https://api.x.ai",
        }
    }
    /// Each vendor's OAuth authorization is only accepted by that vendor's own
    /// CLI, so temporary launches and native switches are never cross-vendor.
    pub fn agent_cli(self) -> AgentCliKind {
        match self {
            Self::Codex => AgentCliKind::Codex,
            Self::Claude => AgentCliKind::ClaudeCode,
            Self::Gemini => AgentCliKind::Gemini,
            Self::Grok => AgentCliKind::Grok,
        }
    }
}

/// Tokens live in ProviderAuth; this preserves the vendor-specific identity needed
/// to refresh and exchange them without confusing local IDs with upstream accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCredential {
    pub vendor: OAuthVendor,
    #[serde(default)]
    pub id_token: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub organization_id: String,
    #[serde(default)]
    pub organization_name: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub refresh_owner: Option<String>,
    /// Set durably before a rotating request. An interrupted refresh must never
    /// replay a potentially consumed refresh token after a restart.
    #[serde(default)]
    pub refresh_pending: bool,
    #[serde(default)]
    pub refresh_error: Option<String>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

impl OAuthCredential {
    pub fn new(vendor: OAuthVendor) -> Self {
        Self {
            vendor,
            id_token: String::new(),
            account_id: String::new(),
            subject: String::new(),
            email: String::new(),
            organization_id: String::new(),
            organization_name: String::new(),
            project_id: String::new(),
            plan: String::new(),
            scopes: Vec::new(),
            client_id: String::new(),
            client_secret: String::new(),
            source: String::new(),
            refresh_owner: None,
            refresh_pending: false,
            refresh_error: None,
            updated_at: None,
        }
    }
}

/// One reading of a quota window's used percentage. The providers serve only
/// the current value, so BalanceHub keeps its own short series to show how the
/// window moved between refreshes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountQuotaSample {
    /// Unix seconds when BalanceHub took the reading.
    pub at: i64,
    pub used_percent: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountQuotaWindow {
    pub id: String,
    pub label: String,
    pub model: Option<String>,
    pub used_percent: Option<f64>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub unit: Option<String>,
    pub reset_at: Option<i64>,
    pub window_seconds: Option<i64>,
    /// Readings carried across refreshes. Old documents have no series, and a
    /// window that a provider only reports once never grows one.
    #[serde(default)]
    pub history: Vec<AccountQuotaSample>,
}

impl AccountQuotaWindow {
    /// The window's used percentage, whichever way the provider reported it.
    /// A used/limit pair is converted here so every consumer reads one number.
    pub fn pressure(&self) -> Option<f64> {
        if let Some(percent) = self.used_percent.filter(|value| value.is_finite()) {
            return Some(percent);
        }
        self.used
            .zip(self.limit)
            .filter(|(_, limit)| *limit > 0.0)
            .map(|(used, limit)| used / limit * 100.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSubscription {
    pub plan: String,
    pub organization: String,
    #[serde(default)]
    pub project_id: String,
    pub windows: Vec<AccountQuotaWindow>,
    pub credits: Option<f64>,
    #[serde(default)]
    pub credits_unit: Option<String>,
    pub credits_unlimited: bool,
    pub updated_at: Option<i64>,
    pub error: Option<String>,
}

impl AccountSubscription {
    /// Providers that report a used/limit pair instead of a percentage still
    /// need one comparable number: the card ranks windows by pressure, the
    /// tray prints percentages and the panel draws meters. The ratio is filled
    /// in once on refresh so every consumer reads `used_percent`; a window
    /// whose limit is zero or missing stays unknown instead of becoming 0.
    pub fn fill_missing_percentages(&mut self) {
        for window in &mut self.windows {
            if window.used_percent.is_none() {
                window.used_percent = window.pressure();
            }
        }
    }

    /// Order the windows by how used up they are, tightest first, leaving
    /// windows without a percentage at the end.
    ///
    /// Ordering is settled here rather than in the UI because the card, the
    /// tray tooltip and the detail panel all show only the first windows: the
    /// provider's own order can lead with a barely-used window and hide the
    /// limit that is about to block work. `sort_by` is stable, so windows with
    /// equal pressure keep the provider's order and the list does not churn
    /// between refreshes.
    pub fn sort_windows_by_pressure(&mut self) {
        self.windows.sort_by(|left, right| {
            let rank = |window: &AccountQuotaWindow| {
                window
                    .pressure()
                    .filter(|value| value.is_finite())
                    .unwrap_or(f64::NEG_INFINITY)
            };
            rank(right)
                .partial_cmp(&rank(left))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaRequestBucket {
    /// The bucket's time label exactly as CPA reports it.
    #[serde(default)]
    pub time: String,
    pub success: u64,
    pub failed: u64,
}

/// Request counts for one API-key credential the gateway holds in its own
/// configuration. CPA groups these by provider and keys each one by
/// `base_url|api_key`; they are not auth files, so the credential list cannot
/// show them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaApiKeyUsage {
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    /// The credential's key, split out of the composite so the panel can mask it
    /// the same way it masks the downstream client keys.
    #[serde(default)]
    pub key: String,
    pub success: u64,
    pub failed: u64,
    #[serde(default)]
    pub recent_requests: Vec<CpaRequestBucket>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaQuotaMetric {
    pub key: String,
    pub label: String,
    pub value: f64,
    /// Empty when the plugin reported no unit.
    #[serde(default)]
    pub unit: String,
    /// `currency` means `value` is money denominated in `currency`; `number`
    /// is a plain count, and anything else is shown as a count.
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub currency: String,
}

/// One limit window. `remaining_fraction` is what the provider has left, so the
/// panel draws usage from its complement; a probe that only knows a reset time
/// leaves it empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaQuotaBucket {
    #[serde(default)]
    pub window: String,
    pub remaining_fraction: Option<f64>,
    pub reset_at: Option<i64>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaQuotaGroup {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub buckets: Vec<CpaQuotaBucket>,
}

/// What a gateway plugin reports for one credential's quota. `plan` and `tier`
/// are the provider's own names, not this app's plan labels.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaAccountQuota {
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub metrics: Vec<CpaQuotaMetric>,
    #[serde(default)]
    pub groups: Vec<CpaQuotaGroup>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaAccountSummary {
    pub name: String,
    #[serde(default)]
    pub auth_index: String,
    pub provider: String,
    pub email: String,
    pub status: String,
    pub status_label: String,
    pub disabled: bool,
    pub unavailable: bool,
    pub available: bool,
    pub can_download: bool,
    pub download_unavailable_reason: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    pub success: Option<u64>,
    pub failed: Option<u64>,
    pub priority: Option<i64>,
    pub weight: Option<i64>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub excluded_models: Vec<String>,
    pub request_retry: Option<i64>,
    pub last_refresh: Option<i64>,
    pub next_retry_after: Option<i64>,
    /// A credential in cooldown can be reset from the card instead of the CPA
    /// panel; the reason and recovery time explain why it is unavailable.
    #[serde(default)]
    pub quota_exceeded: bool,
    #[serde(default)]
    pub quota_reason: String,
    pub quota_next_recover_at: Option<i64>,
    pub backoff_level: Option<i64>,
    /// The gateway reports a plugin that can read this credential's quota; on a
    /// v8 gateway it also names that plugin, while a v0 gateway resolves the
    /// declarative probe through its generic quota route.
    #[serde(default)]
    pub supports_quota: bool,
    #[serde(default)]
    pub quota_plugin_id: String,
    #[serde(default)]
    pub recent_requests: Vec<CpaRequestBucket>,
}

/// Gateway-wide scheduling settings. They belong to the gateway, not to one
/// credential, so they are read on demand instead of on every refresh.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaGatewaySettings {
    /// One of `round-robin`, `weighted-round-robin` or `fill-first`.
    pub routing_strategy: String,
    pub proxy_url: String,
    pub request_retry: Option<i64>,
    pub max_retry_credentials: Option<i64>,
    pub max_retry_interval: Option<i64>,
    /// What the gateway does when an account reports its quota exhausted.
    /// Switching project re-asks the same provider with another project, and
    /// switching the preview model falls back to a preview model instead of
    /// failing the request.
    #[serde(default)]
    pub switch_project: Option<bool>,
    #[serde(default)]
    pub switch_preview_model: Option<bool>,
    /// Whether the gateway keeps a separate log file for every request. The
    /// shared log is written either way; this is what makes one request
    /// readable on its own.
    #[serde(default)]
    pub request_log: Option<bool>,
    pub logs_max_total_size_mb: Option<i64>,
    pub error_logs_max_files: Option<i64>,
    /// Whether the gateway records usage that its usage queue reports.
    #[serde(default)]
    pub usage_statistics_enabled: Option<bool>,
    /// Whether the gateway writes debug lines into the same log.
    #[serde(default)]
    pub debug: Option<bool>,
    /// Whether a model request must carry the credential's prefix. Off, an
    /// unprefixed request may also be served by a prefixed credential.
    #[serde(default)]
    pub force_model_prefix: Option<bool>,
    /// Whether the AI Studio provider authenticates WebSocket calls.
    #[serde(default)]
    pub ws_auth: Option<bool>,
}

/// A partial edit of the gateway-wide scheduling settings. Only the values the
/// panel actually changed are present, so an untouched field keeps whatever the
/// gateway has, including values this app does not surface yet.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaGatewaySettingsUpdate {
    pub routing_strategy: Option<String>,
    /// An empty string clears the gateway's outbound proxy.
    pub proxy_url: Option<String>,
    pub request_retry: Option<i64>,
    pub max_retry_credentials: Option<i64>,
    pub max_retry_interval: Option<i64>,
    pub switch_project: Option<bool>,
    pub switch_preview_model: Option<bool>,
    pub request_log: Option<bool>,
    pub logs_max_total_size_mb: Option<i64>,
    pub error_logs_max_files: Option<i64>,
    pub usage_statistics_enabled: Option<bool>,
    pub debug: Option<bool>,
    pub force_model_prefix: Option<bool>,
    pub ws_auth: Option<bool>,
}

/// The keys downstream clients present to the gateway. CPA stores them as one
/// plain string list, so every change saves the whole list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaClientKeys {
    pub keys: Vec<String>,
}

/// A client key the panel asked the gateway to add. The gateway accepts any
/// string, so an empty value asks this app to mint one.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaClientKeyAdd {
    pub value: String,
}

/// The saved list plus the key that was added, so a minted key can be shown in
/// full once before the panel masks it again.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaClientKeyAdded {
    pub list: CpaClientKeys,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaGatewaySnapshot {
    pub api_version: String,
    /// The gateway's own build, as CPA reports it on every management
    /// response. Empty when the gateway does not send one.
    #[serde(default)]
    pub gateway_version: String,
    pub accounts: Vec<CpaAccountSummary>,
    /// Sum of the per-credential counters CPA reports in its inventory. The
    /// management API has no lifetime token counter, and its usage queue is a
    /// consuming endpoint, so no token total is kept here.
    pub total_requests: Option<u64>,
    pub failed_requests: Option<u64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OAuthExportFormat {
    BalanceHub,
    Cpa,
    CcSwitch,
    NewApi,
    Sub2Api,
    Magpie,
    Native,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthExportOption {
    pub format: OAuthExportFormat,
    pub label: &'static str,
    pub available: bool,
    pub reason: Option<&'static str>,
    pub instructions: &'static str,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthImportEntry {
    pub id: String,
    pub vendor: OAuthVendor,
    pub label: String,
    pub account_id: String,
    pub plan: String,
    pub source: String,
    pub expires_at: Option<i64>,
    pub refreshable: bool,
    pub duplicate_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthImportPreview {
    pub token: String,
    pub entries: Vec<OAuthImportEntry>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthImportFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthImportRequest {
    pub token: String,
    pub entry_ids: Vec<String>,
    #[serde(default)]
    pub replace_existing: bool,
    #[serde(default)]
    pub manage_refresh: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthExportFile {
    pub name: String,
    pub content: String,
    pub instructions: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OAuthLoginPhase {
    Starting,
    Waiting,
    Exchanging,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthLoginTask {
    pub revision: u64,
    pub updated_at: i64,
    pub id: String,
    pub vendor: OAuthVendor,
    pub phase: OAuthLoginPhase,
    pub authorization_url: Option<String>,
    pub user_code: Option<String>,
    pub accepts_callback: bool,
    pub expires_at: i64,
    pub created_at: i64,
    pub message: String,
    pub provider_id: Option<String>,
    pub can_cancel: bool,
    pub can_retry_save: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gateway snapshot is written into `data.json`, so every field added
    /// after a snapshot was saved must stay optional on the wire; otherwise the
    /// whole app data file fails to load and the user loses the workspace.
    #[test]
    fn persisted_snapshots_without_scheduling_fields_still_load() {
        let snapshot: CpaGatewaySnapshot = serde_json::from_str(
            r#"{
                "apiVersion": "v8",
                "accounts": [{
                    "name": "codex.json",
                    "provider": "codex",
                    "email": "codex@example.test",
                    "status": "active",
                    "statusLabel": "可用",
                    "disabled": false,
                    "unavailable": false,
                    "available": true,
                    "canDownload": true,
                    "downloadUnavailableReason": null
                }],
                "totalRequests": 1,
                "failedRequests": 0,
                "totalTokens": 2,
                "updatedAt": 3,
                "usageError": null
            }"#,
        )
        .expect("legacy snapshot parses");
        let account = &snapshot.accounts[0];
        assert_eq!(account.name, "codex.json");
        assert!(account.models.is_empty());
        assert!(account.excluded_models.is_empty());
        assert_eq!(account.note, "");
        assert_eq!(account.priority, None);
        assert_eq!(account.request_retry, None);
        assert_eq!(account.last_refresh, None);
        assert_eq!(account.next_retry_after, None);
        assert!(!account.quota_exceeded);
        assert_eq!(account.quota_reason, "");
        assert!(account.recent_requests.is_empty());
        assert_eq!(snapshot.total_requests, Some(1));
    }

    fn window(
        id: &str,
        used_percent: Option<f64>,
        used: Option<f64>,
        limit: Option<f64>,
    ) -> AccountQuotaWindow {
        AccountQuotaWindow {
            id: id.into(),
            label: id.into(),
            used_percent,
            used,
            limit,
            ..Default::default()
        }
    }

    #[test]
    fn pressure_reads_a_reported_percentage_or_a_used_limit_ratio() {
        assert_eq!(window("a", Some(42.5), None, None).pressure(), Some(42.5));
        assert_eq!(
            window("a", None, Some(30.0), Some(120.0)).pressure(),
            Some(25.0)
        );
        assert_eq!(window("a", None, Some(30.0), None).pressure(), None);
        assert_eq!(window("a", None, Some(30.0), Some(0.0)).pressure(), None);
        // 上报比例优先，即使同时带了一组换算后不同的 used/limit。
        assert_eq!(
            window("a", Some(70.0), Some(1.0), Some(100.0)).pressure(),
            Some(70.0)
        );
    }

    #[test]
    fn windows_sort_tightest_first_and_unknown_last() {
        let mut subscription = AccountSubscription {
            windows: vec![
                window("loose", Some(12.0), None, None),
                window("unknown", None, None, None),
                window("ratio", None, Some(9.0), Some(10.0)),
                window("tight", Some(88.0), None, None),
            ],
            ..Default::default()
        };
        subscription.sort_windows_by_pressure();
        let ids: Vec<_> = subscription.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["ratio", "tight", "loose", "unknown"]);
    }

    #[test]
    fn equal_pressure_keeps_the_provider_order() {
        let mut subscription = AccountSubscription {
            windows: vec![
                window("seven_day_sonnet", Some(20.0), None, None),
                window("seven_day_opus", Some(20.0), None, None),
            ],
            ..Default::default()
        };
        subscription.sort_windows_by_pressure();
        assert_eq!(subscription.windows[0].id, "seven_day_sonnet");
    }

    #[test]
    fn refresh_fills_the_percentage_a_used_limit_pair_implies() {
        let mut subscription = AccountSubscription {
            windows: vec![
                window("money", None, Some(30.0), Some(120.0)),
                window("unknown", None, Some(30.0), Some(0.0)),
                window("reported", Some(70.0), Some(1.0), Some(100.0)),
            ],
            ..Default::default()
        };
        subscription.fill_missing_percentages();
        assert_eq!(subscription.windows[0].used_percent, Some(25.0));
        assert_eq!(subscription.windows[1].used_percent, None);
        // 上报的比例不被换算值覆盖。
        assert_eq!(subscription.windows[2].used_percent, Some(70.0));
    }
}
