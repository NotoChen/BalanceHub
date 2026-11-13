use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::defaults;
use crate::models::{
    default_liveness_interval, default_liveness_random_min_interval, default_liveness_timeout,
    default_true, AccountQuotaSample, AgentCliKind, AuthMode, AuthSource, LivenessIntervalMode,
    LivenessPromptMode, LivenessRecord, ProviderApiKeyOption, ProviderCheckInMethod,
    ProviderCheckInRecord, ProviderNotificationMode, ProviderProtocol, ProviderProxyMode,
    ProviderQuotaScope, ProviderStatus, ProviderTurnstileMode,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    /// 仅用于当前进程内 IPC 合并的单调版本号，不属于用户配置，也不写入导出文件。
    ///
    /// 每次成功持久化事务由 `AppState` 统一递增。前端据此拒绝晚到的旧响应，避免
    /// 单卡操作重新覆盖较新的卡片状态。
    #[serde(skip)]
    pub revision: u64,
    pub identity: ProviderIdentity,
    pub auth: ProviderAuth,
    pub quota: ProviderQuota,
    pub capabilities: ProviderCapabilities,
    #[serde(default)]
    pub cli: ProviderCli,
    pub automation: ProviderAutomation,
    pub liveness: ProviderLiveness,
    pub proxy: ProviderProxy,
    pub notification: ProviderNotification,
    pub runtime: ProviderRuntime,
    /// 该中转站专属的请求约定；默认值与「不带任何额外配置」一致。
    #[serde(default)]
    pub request: super::ProviderRequestConfig,
    /// 通用 API Key 的余额接口声明；未配置时保持协议默认行为。
    #[serde(default)]
    pub quota_spec: super::ProviderQuotaSpec,
    /// 模型目录的展示与元数据覆盖。
    #[serde(default)]
    pub model_config: super::ProviderModelConfig,
}

impl Provider {
    /// 用户界面中用于区分卡片的统一名称。账号卡片保留站点名并追加备注；
    /// API Key 卡片优先使用备注，没有备注时回退到站点名。
    pub fn display_label(&self) -> String {
        let name = self.identity.name.trim();
        let remark = self.identity.remark.trim();
        if matches!(self.auth.mode, AuthMode::ApiKey) {
            return if remark.is_empty() { name } else { remark }.to_string();
        }
        match (name.is_empty(), remark.is_empty()) {
            (true, true) => String::new(),
            (true, false) => remark.to_string(),
            (false, true) => name.to_string(),
            (false, false) => format!("{name} · {remark}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentity {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub protocol: ProviderProtocol,
    /// 用户为卡片设置的本地备注，不受站点同步结果覆盖。
    #[serde(default)]
    pub remark: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub site_logo: String,
    #[serde(default)]
    pub backup_urls: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentityInput {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub protocol: ProviderProtocol,
    /// 用户为卡片设置的本地备注。
    #[serde(default)]
    pub remark: String,
    /// 已认证账号的稳定用户 ID。新增草稿通常为空，保存已存在账号时由前端回传。
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub backup_urls: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCli {
    #[serde(default)]
    pub preferred_model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCliInput {
    #[serde(default)]
    pub preferred_model: String,
}

/// NewAPI dashboard login session. Independent of the user's long-lived PAT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewApiSession {
    pub refresh_cookie: String,
    pub session_id: String,
    pub access_token: String,
    pub access_expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderAuth {
    #[serde(default)]
    pub oauth: Option<crate::models::OAuthCredential>,
    pub mode: AuthMode,
    /// 主凭据的来源：手动 / 账号密码 / OAuth。与 mode（凭据种类）正交。
    #[serde(default)]
    pub source: AuthSource,
    pub api_key: String,
    #[serde(default)]
    pub api_key_token_id: String,
    #[serde(default)]
    pub api_key_options: Vec<ProviderApiKeyOption>,
    pub access_token: String,
    pub session_cookie: String,
    pub api_user: String,
    #[serde(default)]
    pub login_username: String,
    #[serde(default)]
    pub login_password: String,
    /// Sub2API 刷新令牌（滚动轮换）；仅在持久化的刷新路径中使用，避免重用攻击。
    #[serde(default)]
    pub refresh_token: String,
    /// access_token 过期时刻（unix 秒）；None 表示未知（NewAPI 等无此概念）。
    #[serde(default)]
    pub access_token_expires_at: Option<i64>,
    #[serde(default)]
    pub new_api_session: Option<NewApiSession>,
    #[serde(default)]
    pub browser_binding: Option<crate::models::BrowserLoginBinding>,
    /// Advances on explicit credential replacement/removal, not token rotation.
    #[serde(default)]
    pub credential_revision: u64,
    #[serde(default)]
    pub session_updated_at: Option<i64>,
}

impl Default for ProviderAuth {
    fn default() -> Self {
        Self {
            oauth: None,
            mode: AuthMode::default(),
            source: AuthSource::Password,
            api_key: String::new(),
            api_key_token_id: String::new(),
            api_key_options: Vec::new(),
            access_token: String::new(),
            session_cookie: String::new(),
            api_user: String::new(),
            login_username: String::new(),
            login_password: String::new(),
            refresh_token: String::new(),
            access_token_expires_at: None,
            new_api_session: None,
            browser_binding: None,
            credential_revision: 0,
            session_updated_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuota {
    pub available: f64,
    pub used: f64,
    #[serde(default = "default_true")]
    pub known: bool,
    #[serde(default = "default_true")]
    pub total_known: bool,
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
    /// 账号当前使用的站点分组。NewAPI 的模型价格要乘分组倍率才是实际收费，
    /// 刷新时由站点返回；API Key 卡片取当前 Key 所属分组。
    #[serde(default)]
    pub group: String,
    /// Subscription-billed relays (NewAPI 订阅、Sub2API 订阅分组) keep their real
    /// quota in period windows while the wallet only holds overflow, so the card
    /// needs those windows to answer "还有多少" for such an account.
    #[serde(default)]
    pub windows: Vec<ProviderQuotaWindow>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuotaWindow {
    pub label: String,
    pub used: f64,
    /// 0 表示不限量：NewAPI 订阅把 0 作为“不限额度”，Sub2API 不给出窗口上限时同样如此。
    #[serde(default)]
    pub limit: f64,
    /// 窗口下一次重置的 unix 秒；窗口不重置时为空。
    #[serde(default)]
    pub resets_at: Option<i64>,
    /// 本应用自己观察到的窗口读数。站点只返回当前值，趋势和耗尽预测只能
    /// 来自刷新历史；旧文档没有这一项时按空序列读取。
    #[serde(default)]
    pub history: Vec<AccountQuotaSample>,
}

impl ProviderQuotaWindow {
    /// 已用比例；不限量窗口没有可比的比例，因此返回 None 而不是 0。
    pub fn percent(&self) -> Option<f64> {
        (self.limit > 0.0).then(|| self.used / self.limit * 100.0)
    }
}

impl ProviderQuota {
    /// 把上一次刷新留下的窗口读数接到本次结果上，并追加本次读数。
    ///
    /// 套餐窗口没有稳定 id，按标签对应；站点不再返回某个窗口时，它重新出现
    /// 会从新序列开始，避免把另一档额度的历史算进来。刷新失败但保留了旧窗口
    /// 时，百分比没有变化，去重规则不会追加重复点。
    pub fn record_window_history(&mut self, previous: &ProviderQuota, at: i64) {
        for window in &mut self.windows {
            let prior = previous
                .windows
                .iter()
                .find(|prior| prior.label == window.label);
            if let Some(prior) = prior {
                window.history.clone_from(&prior.history);
            }
            let percent = window.percent();
            crate::quota_history::push_sample(&mut window.history, percent, at);
        }
    }

    /// 压力最大的窗口决定账号是否还有余量，卡片据此给窗口着色。
    pub fn tightest_window(&self) -> Option<&ProviderQuotaWindow> {
        self.windows
            .iter()
            .filter_map(|window| window.percent().map(|percent| (percent, window)))
            .max_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, window)| window)
    }

    /// 订阅窗口有上限时才代表账户真实余量：任一窗口还有空间就不是用尽。
    /// 没有窗口或窗口全部不限量时返回 None，调用方继续按钱包余额判断。
    pub fn windows_exhausted(&self) -> Option<bool> {
        let mut limited = false;
        for window in &self.windows {
            let Some(percent) = window.percent() else {
                continue;
            };
            limited = true;
            if percent < 100.0 {
                return Some(false);
            }
        }
        limited.then_some(true)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    #[serde(default)]
    pub subscription: Option<crate::models::AccountSubscription>,
    #[serde(default)]
    pub gateway: Option<crate::models::CpaGatewaySnapshot>,
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
    #[serde(default)]
    pub available_models_state: ProviderModelListState,
    /// 可选站点接口的探测结果。签到、密钥管理和邀请仍有专用字段，其余能力
    /// 统一放在这里，UI 据此显隐入口并说明不支持原因。
    #[serde(default)]
    pub optional: Vec<ProviderOptionalCapabilityState>,
    /// 站点价目快照；和能力探测结果分开维护，普通刷新也会更新。
    #[serde(default)]
    pub pricing: crate::models::ProviderPricingSnapshot,
}

/// 除签到、密钥管理和邀请之外，BalanceHub 会按需读取的站点接口。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderOptionalCapability {
    #[default]
    Pricing,
    Usage,
    RequestLogs,
    Announcements,
    Subscription,
    Groups,
    Models,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderOptionalCapabilityState {
    pub capability: ProviderOptionalCapability,
    /// 是否得到了明确结论。网络错误或认证失败时保持 false，调用方继续按
    /// 协议默认能力处理，而不是把一次失败当成「站点不支持」。
    pub known: bool,
    pub supported: bool,
    /// 不支持或确认支持时的说明，供能力详情展示。
    pub reason: Option<String>,
    /// 探测请求本身失败的原因。
    pub error: Option<String>,
}

impl ProviderOptionalCapabilityState {
    pub fn supported(capability: ProviderOptionalCapability, reason: impl Into<String>) -> Self {
        Self {
            capability,
            known: true,
            supported: true,
            reason: Some(reason.into()).filter(|reason| !reason.trim().is_empty()),
            error: None,
        }
    }

    pub fn unsupported(capability: ProviderOptionalCapability, reason: impl Into<String>) -> Self {
        Self {
            capability,
            known: true,
            supported: false,
            reason: Some(reason.into()).filter(|reason| !reason.trim().is_empty()),
            error: None,
        }
    }

    pub fn unknown(capability: ProviderOptionalCapability, error: impl Into<String>) -> Self {
        Self {
            capability,
            known: false,
            supported: false,
            reason: None,
            error: Some(error.into()).filter(|error| !error.trim().is_empty()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderModelScope {
    Account,
    ApiKey,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderModelListState {
    pub scope: Option<ProviderModelScope>,
    pub updated_at: Option<String>,
    pub error: Option<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderModelList {
    pub models: Vec<String>,
    pub scope: ProviderModelScope,
    pub fallback_reason: Option<String>,
}

impl ProviderModelList {
    pub fn new(models: Vec<String>, scope: ProviderModelScope) -> Self {
        Self {
            models,
            scope,
            fallback_reason: None,
        }
    }
}

impl ProviderCapabilities {
    pub fn set_available_models(&mut self, result: ProviderModelList) {
        self.available_models = result.models;
        self.available_models_state = ProviderModelListState {
            scope: Some(result.scope),
            updated_at: Some(crate::util::unix_millis().to_string()),
            error: None,
            fallback_reason: result.fallback_reason,
        };
    }

    pub fn clear_available_models(&mut self) {
        self.available_models.clear();
        self.available_models_state = ProviderModelListState::default();
    }

    pub fn optional_state(
        &self,
        capability: ProviderOptionalCapability,
    ) -> Option<&ProviderOptionalCapabilityState> {
        self.optional
            .iter()
            .find(|state| state.capability == capability)
    }

    /// 同一能力只保留一条结果；后一次探测覆盖前一次。
    pub fn set_optional_state(&mut self, state: ProviderOptionalCapabilityState) {
        match self
            .optional
            .iter_mut()
            .find(|existing| existing.capability == state.capability)
        {
            Some(existing) => *existing = state,
            None => self.optional.push(state),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAutomation {
    #[serde(default)]
    pub refresh_interval: u64,
    #[serde(default)]
    pub check_in_time: String,
    #[serde(default)]
    pub check_in_method: ProviderCheckInMethod,
    #[serde(default = "default_true")]
    pub auto_shield: bool,
    #[serde(default)]
    pub turnstile_mode: ProviderTurnstileMode,
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
    #[serde(default)]
    pub check_in_method: ProviderCheckInMethod,
    #[serde(default = "default_true")]
    pub auto_shield: bool,
    #[serde(default)]
    pub turnstile_mode: ProviderTurnstileMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLiveness {
    #[serde(default = "default_true")]
    pub use_global: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub agent_base_urls: BTreeMap<AgentCliKind, String>,
    #[serde(default)]
    pub cli_kind: Option<AgentCliKind>,
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
    pub agent_base_urls: BTreeMap<AgentCliKind, String>,
    #[serde(default)]
    pub cli_kind: Option<AgentCliKind>,
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
    /// 连续刷新失败次数：瞬时失败指数退避的依据，成功一次即清零。
    #[serde(default)]
    pub consecutive_failures: u32,
    /// 自动刷新被上游明确拒绝后的冷却截止时间（Unix 秒）。时间到了才允许
    /// 再次自动请求；手动刷新不受限制。
    #[serde(default)]
    pub cooldown_until: Option<i64>,
    /// 冷却原因，直接展示在卡片上，例如「凭据被拒绝」。
    #[serde(default)]
    pub cooldown_reason: Option<String>,
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
