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
    /// Per-Agent model overrides keyed by [`AgentCliKind::key`]. One provider
    /// can serve several Agent CLIs with different model catalogues, so a
    /// single `preferred_model` cannot describe every binding.
    #[serde(default)]
    pub agent_models: BTreeMap<String, String>,
}

impl ProviderCli {
    /// Model a concrete Agent CLI should write, falling back to the provider
    /// default. This is the single read point for every adapter.
    pub fn model_for(&self, kind: AgentCliKind) -> &str {
        self.agent_models
            .get(kind.key())
            .map(String::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| self.preferred_model.trim())
    }

    /// Bind (or clear) the model this Agent CLI should use. An empty model
    /// removes the override so the provider default applies again.
    pub fn bind_agent_model(&mut self, kind: AgentCliKind, model: &str) {
        let model = model.trim();
        if model.is_empty() {
            self.agent_models.remove(kind.key());
        } else {
            self.agent_models
                .insert(kind.key().to_string(), model.to_string());
        }
    }
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
    /// 切换调用 Key 后，所有按 Key 维度采集的额度读数都失效。
    ///
    /// 保留展示单位和 scope（协议决定），但把余额、分组和窗口清空；卡片会
    /// 回到“未同步”，而不是继续展示旧 Key 的窗口。
    pub fn clear_key_observations(&mut self) {
        self.available = 0.0;
        self.used = 0.0;
        self.known = false;
        self.total_known = false;
        self.unlimited = false;
        self.group.clear();
        self.windows.clear();
    }

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
    /// 站点观测到的模型运行表现；成功后同样由普通刷新更新。
    #[serde(default)]
    pub performance: crate::models::ProviderPerformanceSnapshot,
}

/// 除签到、密钥管理和邀请之外，BalanceHub 会按需读取的站点接口。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderOptionalCapability {
    #[default]
    Pricing,
    Performance,
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
    /// 这条结论由谁写入：能力探测、普通刷新或手动操作。旧配置没有这个字段。
    pub source: Option<String>,
    /// 最近一次检查该接口的时间；检查失败也会更新。
    pub last_checked_at: Option<String>,
    /// 最近一次得到可用结论的时间；失败时保留上一次成功的时间。
    pub last_success_at: Option<String>,
}

impl ProviderOptionalCapabilityState {
    pub fn supported(capability: ProviderOptionalCapability, reason: impl Into<String>) -> Self {
        Self {
            capability,
            known: true,
            supported: true,
            reason: Some(reason.into()).filter(|reason| !reason.trim().is_empty()),
            error: None,
            source: None,
            last_checked_at: None,
            last_success_at: None,
        }
    }

    pub fn unsupported(capability: ProviderOptionalCapability, reason: impl Into<String>) -> Self {
        Self {
            capability,
            known: true,
            supported: false,
            reason: Some(reason.into()).filter(|reason| !reason.trim().is_empty()),
            error: None,
            source: None,
            last_checked_at: None,
            last_success_at: None,
        }
    }

    pub fn unknown(capability: ProviderOptionalCapability, error: impl Into<String>) -> Self {
        Self {
            capability,
            known: false,
            supported: false,
            reason: None,
            error: Some(error.into()).filter(|error| !error.trim().is_empty()),
            source: None,
            last_checked_at: None,
            last_success_at: None,
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
        // 模型列表是当前配置和凭据的观测：列表清空后，能力台账不能继续
        // 显示上一次读取的模型数量和成功时间。协议层「支持读取」的结论
        // 仍然保留，只把证据标记为待重新读取。
        if let Some(state) = self
            .optional
            .iter_mut()
            .find(|state| state.capability == ProviderOptionalCapability::Models)
        {
            state.last_checked_at = None;
            state.last_success_at = None;
            state.error = None;
            if state.known && state.supported {
                state.reason = Some("配置或凭据变更后待重新读取".to_string());
            }
        }
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

    /// 记录一次接口检查，同时保留上次的明确结论。
    ///
    /// 网络错误或认证失败只能说明「这次没读到」，不能推翻上次确认的支持或
    /// 不支持结论；但检查时间和错误仍要更新，用户才能区分「刚检查过但失败」
    /// 和「很久没检查」。所有能力探测、模型同步和普通刷新都走这里，避免各
    /// 自维护一套“失败要不要覆盖”的判断。
    pub fn record_optional_check(
        &mut self,
        mut state: ProviderOptionalCapabilityState,
        checked_at: &str,
        source: &str,
    ) {
        let previous = self.optional_state(state.capability).cloned();
        let previous_known = previous.as_ref().is_some_and(|existing| existing.known);
        state.source = Some(source.to_string());
        state.last_checked_at = Some(checked_at.to_string());
        state.last_success_at = if state.known && state.supported {
            Some(checked_at.to_string())
        } else {
            previous
                .as_ref()
                .and_then(|existing| existing.last_success_at.clone())
        };
        match previous {
            // 本轮没有得到结论：保留已知结论、说明和上次成功时间，只记录
            // 本轮确实检查过以及本轮错误。
            Some(mut existing) if previous_known && !state.known => {
                existing.source = state.source;
                existing.last_checked_at = state.last_checked_at;
                if state.error.is_some() {
                    existing.error = state.error;
                }
                self.set_optional_state(existing);
            }
            // 成功的 known 结论（包括明确不支持）仍然覆盖旧值；从未确认过
            // 的接口也保留 unknown，让用户看到「检查过但没确认」。
            _ => self.set_optional_state(state),
        }
    }

    /// 把一次能力探测结果合并到已有快照。
    ///
    /// 探测失败时适配器只能返回默认结构；如果直接整包替换，就会把上次成功
    /// 读到的价格、模型表现和已知能力一起清掉。这里只接受探测确实产生的
    /// 观测：失败或未触达的字段沿用旧值，明确的“支持/不支持”结论仍然生效。
    pub fn merge_probe(
        &self,
        probed: ProviderCapabilities,
        invite_link: &str,
        probe_error: Option<&str>,
        checked_at: &str,
    ) -> ProviderCapabilities {
        let mut next = self.clone();
        if probed.subscription.is_some() {
            next.subscription = probed.subscription;
        }
        if probed.gateway.is_some() {
            next.gateway = probed.gateway;
        }
        if probed.check_in_known {
            next.check_in_known = true;
            next.check_in_supported = probed.check_in_supported;
            next.check_in_auth_modes = probed.check_in_auth_modes;
        }
        if probed.api_key_management_known {
            next.api_key_management_known = true;
            next.api_key_management_supported = probed.api_key_management_supported;
        }
        if probed.invitation_known {
            next.invitation_known = true;
            next.invitation_supported = probed.invitation_supported;
        }
        if !invite_link.trim().is_empty() {
            next.invite_link = invite_link.to_string();
        } else if probe_error.is_none() && probed.invitation_known && !probed.invitation_supported {
            // 一次成功的探测明确确认邀请能力不可用时，旧邀请链接不应继续展示。
            next.invite_link.clear();
        }
        if probe_touched_pricing(&probed.pricing) {
            next.pricing = probed.pricing;
        }
        if probe_touched_performance(&probed.performance) {
            next.performance = probed.performance;
        }
        for state in probed.optional {
            next.record_optional_check(state, checked_at, "probe");
        }
        next
    }
}

fn probe_touched_pricing(snapshot: &crate::models::ProviderPricingSnapshot) -> bool {
    snapshot.updated_at.is_some()
        || snapshot.error.is_some()
        || snapshot.source != crate::models::ProviderPricingSource::None
        || !snapshot.models.is_empty()
        || !snapshot.groups.is_empty()
}

fn probe_touched_performance(snapshot: &crate::models::ProviderPerformanceSnapshot) -> bool {
    snapshot.updated_at.is_some() || snapshot.error.is_some() || !snapshot.models.is_empty()
}

#[cfg(test)]
mod capability_merge_tests {
    use super::*;
    use crate::models::{
        ProviderModelPerformance, ProviderModelPrice, ProviderOptionalCapability,
        ProviderOptionalCapabilityState, ProviderPricingSnapshot, ProviderPricingSource,
    };

    fn observed() -> ProviderCapabilities {
        let mut capabilities = ProviderCapabilities {
            check_in_known: true,
            check_in_supported: true,
            api_key_management_known: true,
            api_key_management_supported: true,
            invitation_known: true,
            invitation_supported: true,
            invite_link: "https://example.com/invite".into(),
            probed_at: Some("1000".into()),
            ..ProviderCapabilities::default()
        };
        capabilities.available_models = vec!["model-a".into()];
        capabilities.pricing.mark_loaded(
            ProviderPricingSource::NewApi,
            std::collections::BTreeMap::from([(
                "model-a".into(),
                ProviderModelPrice {
                    input: Some(1.0),
                    ..ProviderModelPrice::default()
                },
            )]),
            std::collections::BTreeMap::new(),
            String::new(),
            Default::default(),
        );
        capabilities.performance.mark_loaded(
            std::collections::BTreeMap::from([(
                "model-a".into(),
                ProviderModelPerformance {
                    success_rate: Some(99.0),
                    ..ProviderModelPerformance::default()
                },
            )]),
            Some(1),
            Some(2),
        );
        capabilities.set_optional_state(ProviderOptionalCapabilityState::supported(
            ProviderOptionalCapability::Usage,
            "已读取用量",
        ));
        capabilities
    }

    #[test]
    fn failed_probe_keeps_previous_observations() {
        let previous = observed();
        let failed = ProviderCapabilities {
            error_message: Some("认证失败".into()),
            ..ProviderCapabilities::default()
        };
        let merged = previous.merge_probe(failed, "", Some("认证失败"), "1000");
        assert_eq!(merged.available_models, vec!["model-a"]);
        assert_eq!(merged.pricing.model("model-a").unwrap().input, Some(1.0));
        assert_eq!(
            merged.performance.model("model-a").unwrap().success_rate,
            Some(99.0)
        );
        assert!(merged.check_in_supported);
        assert!(merged.api_key_management_supported);
        assert!(merged.invitation_supported);
        assert_eq!(merged.invite_link, "https://example.com/invite");
        assert!(
            merged
                .optional_state(ProviderOptionalCapability::Usage)
                .unwrap()
                .known
        );
    }

    #[test]
    fn explicit_probe_conclusion_replaces_the_previous_one() {
        let previous = observed();
        let probed = ProviderCapabilities {
            check_in_known: true,
            check_in_supported: false,
            invitation_known: true,
            invitation_supported: false,
            ..ProviderCapabilities::default()
        };
        let merged = previous.merge_probe(probed, "", None, "2000");
        assert!(!merged.check_in_supported);
        assert!(!merged.invitation_supported);
        assert!(merged.invite_link.is_empty());
        // A probe that does not touch pricing/performance must not erase them.
        assert!(merged.pricing.model("model-a").is_some());
        assert!(merged.performance.model("model-a").is_some());
    }

    #[test]
    fn unknown_optional_state_does_not_erase_a_known_conclusion() {
        let mut previous = observed();
        let usage = previous
            .optional
            .iter_mut()
            .find(|state| state.capability == ProviderOptionalCapability::Usage)
            .expect("usage observation");
        usage.last_checked_at = Some("2500".into());
        usage.last_success_at = Some("2500".into());

        let probed = ProviderCapabilities {
            optional: vec![ProviderOptionalCapabilityState::unknown(
                ProviderOptionalCapability::Usage,
                "网络超时",
            )],
            ..ProviderCapabilities::default()
        };
        let merged = previous.merge_probe(probed, "", Some("网络超时"), "3000");
        let usage = merged
            .optional_state(ProviderOptionalCapability::Usage)
            .unwrap();
        assert!(usage.known);
        assert!(usage.supported);
        assert_eq!(usage.reason.as_deref(), Some("已读取用量"));
        // 失败也要留下“刚检查过”的证据，但不能改写上次成功时间。
        assert_eq!(usage.last_checked_at.as_deref(), Some("3000"));
        assert_eq!(usage.last_success_at.as_deref(), Some("2500"));
        assert_eq!(usage.error.as_deref(), Some("网络超时"));
        assert_eq!(usage.source.as_deref(), Some("probe"));
    }

    #[test]
    fn a_confirmed_optional_state_records_its_success_time() {
        let previous = observed();
        let probed = ProviderCapabilities {
            optional: vec![ProviderOptionalCapabilityState::supported(
                ProviderOptionalCapability::Models,
                "已读取 3 个模型",
            )],
            ..ProviderCapabilities::default()
        };
        let merged = previous.merge_probe(probed, "", None, "4000");
        let models = merged
            .optional_state(ProviderOptionalCapability::Models)
            .unwrap();
        assert!(models.known);
        assert!(models.supported);
        assert_eq!(models.last_checked_at.as_deref(), Some("4000"));
        assert_eq!(models.last_success_at.as_deref(), Some("4000"));
    }

    #[test]
    fn clearing_models_drops_its_evidence_but_keeps_protocol_support() {
        let mut capabilities = observed();
        capabilities.set_available_models(crate::models::ProviderModelList::new(
            vec!["model-a".into()],
            crate::models::ProviderModelScope::Account,
        ));
        capabilities.record_optional_check(
            ProviderOptionalCapabilityState::supported(
                ProviderOptionalCapability::Models,
                "已读取 1 个模型",
            ),
            "5000",
            "probe",
        );

        capabilities.clear_available_models();

        let models = capabilities
            .optional_state(ProviderOptionalCapability::Models)
            .unwrap();
        assert!(models.known);
        assert!(models.supported);
        assert_eq!(models.reason.as_deref(), Some("配置或凭据变更后待重新读取"));
        assert!(models.last_checked_at.is_none());
        assert!(models.last_success_at.is_none());
    }

    #[test]
    fn explicit_unsupported_pricing_clears_stale_prices() {
        let mut previous = observed();
        previous.pricing.mark_unsupported("站点未提供价目".into());
        let probed = ProviderCapabilities {
            pricing: {
                let mut snapshot = ProviderPricingSnapshot::default();
                snapshot.mark_unsupported("站点未提供价目".into());
                snapshot
            },
            ..ProviderCapabilities::default()
        };
        let merged = previous.merge_probe(probed, "", None, "2000");
        assert!(merged.pricing.model("model-a").is_none());
        assert_eq!(merged.pricing.error.as_deref(), Some("站点未提供价目"));
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
    /// 最近一次刷新被站点明确拒绝凭据。卡片据此把「重新登录」当作修复入口，
    /// 而不是把网络故障、限流也误判成认证问题。
    #[serde(default)]
    pub credential_rejected: bool,
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
