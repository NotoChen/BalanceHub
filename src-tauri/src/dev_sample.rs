//! 开发版演示数据：在开发版现有配置之上追加各类卡片形态，供界面检查使用。
//!
//! 只在 `debug_assertions` 下编译；数据直接写成 `Provider` 字面量而不是 JSON，
//! 这样模型字段改名时这里会编译失败，不会悄悄留下对不上的演示数据。演示站点
//! 名称、账号与凭据全是编造值，统一使用 `demo-*.example.com` 域名；定时刷新、
//! 签到、测活全部关闭，开发进程不会真的打到这些域名上。
//!
//! 演示卡片按 ID 追加：只替换同 ID 的演示卡片，真实站点与其它设置保持不变。
//!
//! 覆盖范围按“卡片在真实使用里会出现的形态”排列：正常、警告、读取失败、停用、
//! 首次未同步、Key 维度额度、无限额度、订阅窗口（1 / 2 / 3 条）、不限量窗口、
//! OAuth 各厂商与续期异常、CPA 网关正常 / 有冷却 / 未连接。

use std::collections::HashSet;

use serde::Deserialize;

use crate::models::{
    AccountQuotaSample, AccountQuotaWindow, AccountSubscription, AppData, AuthMode, AuthSource,
    CpaAccountSummary, CpaGatewaySnapshot, OAuthCredential, OAuthVendor, Provider, ProviderAuth,
    ProviderIdentityInput, ProviderInput, ProviderLivenessInput, ProviderModelListState,
    ProviderModelScope, ProviderProtocol, ProviderQuotaScope, ProviderQuotaWindow, ProviderRuntime,
    ProviderStatus,
};

/// 开发版读取配置时顺手并入演示卡片的开关。与 `BALANCEHUB_DEV_ROOT` 一样由开发脚本
/// 注入，不需要设置页点击；发布构建不编译这里。
pub const ENV_MERGE_ON_LOAD: &str = "BALANCEHUB_DEV_SAMPLE";

#[derive(Deserialize)]
struct ProviderFile {
    #[serde(default)]
    providers: Vec<Provider>,
}

/// 读取开发版配置时调用。开关未开启或配置里已有演示卡片时返回 `None`，
/// 调用方照常使用原配置，不会多写一次盘。
pub fn merge_on_load(data: &mut AppData) -> Option<usize> {
    if !std::env::var(ENV_MERGE_ON_LOAD)
        .is_ok_and(|value| !matches!(value.trim(), "" | "0" | "false"))
    {
        return None;
    }
    if data.providers.iter().any(is_sample) {
        return None;
    }
    merge_into(data);
    Some(sample_providers().len())
}

/// 只从磁盘读一遍中转站列表。加载阶段还没建立事务状态，用这个结果判断演示卡片
/// 是否已经在盘上，避免每次启动都重写配置。
pub(super) fn samples_present(provider_file: &std::path::Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(provider_file) else {
        return false;
    };
    serde_json::from_str::<ProviderFile>(&raw)
        .is_ok_and(|file| file.providers.iter().any(is_sample))
}

fn is_sample(provider: &Provider) -> bool {
    provider.identity.name.starts_with("演示 · ")
}

/// 把演示卡片并进一份现有配置，返回其中演示卡片的数量。
///
/// 真实站点、设置、工作区和临时 CLI 偏好原样保留；演示卡片按 ID 覆盖，
/// 重复点击不会越加越多。定时刷新、签到与测活在这里统一关闭，然后走和
/// “导入配置”完全相同的归一化，保证交给存储层的快照本身就合法。
pub fn merge_into(data: &mut AppData) -> usize {
    let samples = sample_providers();
    let sample_count = samples.len();
    let sample_ids: HashSet<String> = samples
        .iter()
        .map(|provider| provider.identity.id.clone())
        .collect();
    data.providers
        .retain(|provider| !sample_ids.contains(&provider.identity.id));
    data.providers.extend(samples);
    quiet_automation(data);
    crate::limits::normalize_app_data(data);
    sample_count
}

/// 演示站点一律关闭定时任务：签到、刷新、测活都会真的打到编造域名。
fn quiet_automation(data: &mut AppData) {
    data.settings.auto_refresh_enabled = false;
    data.settings.auto_check_in_enabled = false;
    data.settings.notification_enabled = false;
    data.settings.liveness_enabled = false;
}

/// 演示卡片名称统一带前缀，和真实站点在列表里一眼分得开。
fn demo_name(name: &str) -> String {
    format!("演示 · {name}")
}

fn sample_providers() -> Vec<Provider> {
    let now = now_secs();
    vec![
        // —— NewAPI ——
        new_api_full(now),
        new_api_weekly_only(now),
        new_api_warning(now),
        new_api_error(),
        new_api_disabled(now),
        new_api_pending(),
        new_api_key(now),
        new_api_key_unlimited(now),
        // —— Sub2API ——
        sub2_api_subscription(now),
        sub2_api_quota_capped(now),
        sub2_api_key(now),
        sub2_api_error(now),
        // —— 通用 API Key ——
        generic_api_key(now),
        generic_api_key_unknown(now),
        // —— OAuth 账号 ——
        oauth_codex(now),
        oauth_claude(now),
        oauth_gemini(now),
        oauth_grok(now),
        oauth_renewal_error(now),
        // —— CPA 网关 ——
        cpa_gateway(now),
        cpa_gateway_cooling(now),
        cpa_gateway_offline(),
    ]
}

// —— NewAPI ——

/// 账号密码登录的完整形态：钱包、三条订阅窗口、签到与邀请、Key 管理、模型。
fn new_api_full(now: i64) -> Provider {
    let mut provider = new_api_account(
        "demo-newapi-full",
        "NewAPI 满能力账号",
        "https://demo-newapi.example.com",
        "demo-user",
        "10001",
    );
    provider.quota.available = 42.5;
    provider.quota.used = 17.5;
    provider.quota.windows = vec![
        window("今日", 1.2, 5.0, now + 3 * 3_600),
        window("本周", 21.0, 30.0, now + 86_400 * 2),
        window("本月", 64.0, 100.0, now + 86_400 * 12),
    ];
    provider.capabilities.check_in_known = true;
    provider.capabilities.check_in_supported = true;
    provider.capabilities.check_in_auth_modes = vec![AuthMode::Session];
    provider.capabilities.api_key_management_known = true;
    provider.capabilities.api_key_management_supported = true;
    provider.capabilities.invitation_known = true;
    provider.capabilities.invitation_supported = true;
    provider.capabilities.invite_link = "https://demo-newapi.example.com/register?aff=demo".into();
    provider.capabilities.available_models = models(&MANY_MODELS);
    settle(provider, ProviderStatus::Ok, now)
}

/// 只有一条周窗口：正文稀疏时页脚依然贴着卡片底部，不会在正文下方留空带。
fn new_api_weekly_only(now: i64) -> Provider {
    let mut provider = new_api_account(
        "demo-newapi-weekly",
        "NewAPI 单窗口账号",
        "https://demo-newapi.example.com",
        "demo-weekly",
        "10002",
    );
    provider.quota.available = 12.0;
    provider.quota.used = 8.0;
    provider.quota.windows = vec![window("每周", 18.0, 20.0, now + 86_400 * 4)];
    provider.capabilities.available_models = models(&["gpt-5.5"]);
    settle(provider, ProviderStatus::Ok, now)
}

/// 用量接近上限：卡片进入警告色，主数字和窗口条同时变色。
fn new_api_warning(now: i64) -> Provider {
    let mut provider = new_api_account(
        "demo-newapi-warning",
        "NewAPI 接近用尽",
        "https://demo-warning.example.com",
        "demo-tight",
        "10003",
    );
    provider.quota.available = 1.2;
    provider.quota.used = 28.8;
    provider.quota.windows = vec![
        window("今日", 9.2, 10.0, now + 1_200),
        window("本月", 96.0, 100.0, now + 86_400 * 9),
    ];
    provider.capabilities.check_in_known = true;
    provider.capabilities.check_in_auth_modes = vec![AuthMode::Session];
    provider.capabilities.available_models = models(&["claude-sonnet-5-5", "claude-opus-5-5"]);
    settle(provider, ProviderStatus::Warning, now)
}

/// 读取失败：卡片保留上一次的数据，同时给出错误说明。
fn new_api_error() -> Provider {
    let mut provider = new_api_account(
        "demo-newapi-error",
        "NewAPI 读取失败",
        "https://demo-broken.example.com",
        "demo-broken",
        "10004",
    );
    provider.quota.known = false;
    provider.quota.total_known = false;
    provider.capabilities.available_models.clear();
    provider.runtime = ProviderRuntime {
        enabled: true,
        status: ProviderStatus::Error,
        error_message: Some("站点返回 502，额度接口暂无响应".into()),
    };
    provider
}

/// 停用：卡片整体变灰，刷新与签到入口关闭，其余入口仍可打开。
fn new_api_disabled(now: i64) -> Provider {
    let mut provider = new_api_full(now);
    provider.identity.id = "demo-newapi-disabled".into();
    provider.identity.name = demo_name("NewAPI 已停用");
    provider.identity.base_url = "https://demo-paused.example.com".into();
    provider.runtime = ProviderRuntime {
        enabled: false,
        status: ProviderStatus::Ok,
        error_message: None,
    };
    provider
}

/// 首次同步前：没有额度、没有模型，卡片显示“等待首次同步”。
fn new_api_pending() -> Provider {
    let mut provider = new_api_account(
        "demo-newapi-pending",
        "NewAPI 尚未同步",
        "https://demo-pending.example.com",
        "demo-pending",
        "10005",
    );
    provider.quota.available = 0.0;
    provider.quota.used = 0.0;
    provider.quota.known = false;
    provider.quota.total_known = false;
    provider.runtime = ProviderRuntime {
        enabled: true,
        status: ProviderStatus::Ok,
        error_message: None,
    };
    provider
}

/// NewAPI 的 Key 维度卡片：只有 Key 额度，没有签到与账号信息。
fn new_api_key(_now: i64) -> Provider {
    let mut provider = build(
        "demo-newapi-key",
        ProviderIdentityInput {
            name: "NewAPI 同事 Key".into(),
            base_url: "https://demo-newapi.example.com".into(),
            protocol: ProviderProtocol::NewApi,
            remark: "共享 Key".into(),
            ..Default::default()
        },
        key_auth("sk-demo-newapi-key"),
    );
    provider.quota.scope = ProviderQuotaScope::Token;
    provider.quota.available = 8.0;
    provider.quota.used = 2.0;
    provider.capabilities.api_key_management_known = true;
    provider.capabilities.api_key_management_supported = true;
    provider.capabilities.available_models = models(&["gpt-5.5"]);
    settle(provider, ProviderStatus::Ok, _now)
}

/// Key 额度无限：主数字写“无限”，不画比例条。
fn new_api_key_unlimited(now: i64) -> Provider {
    let mut provider = build(
        "demo-newapi-key-unlimited",
        ProviderIdentityInput {
            name: "NewAPI 无限 Key".into(),
            base_url: "https://demo-unlimited.example.com".into(),
            protocol: ProviderProtocol::NewApi,
            ..Default::default()
        },
        key_auth("sk-demo-unlimited"),
    );
    provider.quota.scope = ProviderQuotaScope::Token;
    provider.quota.known = true;
    provider.quota.total_known = true;
    provider.quota.unlimited = true;
    provider.capabilities.available_models = models(&["gpt-5.5", "gpt-5.5-mini"]);
    settle(provider, ProviderStatus::Ok, now)
}

// —— Sub2API ——

/// 订阅账号：钱包只剩溢出额度，真实余量在两条订阅窗口里。
fn sub2_api_subscription(now: i64) -> Provider {
    let mut provider = sub2_api_account(
        "demo-sub2api-subscription",
        "Sub2API 团队版订阅",
        "https://demo-sub2api.example.com",
        "demo-manager",
        Some("团队版"),
    );
    provider.quota.available = 0.0;
    provider.quota.used = 12.0;
    provider.quota.windows = vec![
        window("5 小时", 42.0, 60.0, now + 1_800),
        window("本周", 380.0, 500.0, now + 86_400 * 3),
    ];
    settle(provider, ProviderStatus::Warning, now)
}

/// 三条窗口且包含一条不限量：卡片列出三条，不限量那条写“不限”。
fn sub2_api_quota_capped(now: i64) -> Provider {
    let mut provider = sub2_api_account(
        "demo-sub2api-three-windows",
        "Sub2API 三窗口订阅",
        "https://demo-sub2api.example.com",
        "demo-three",
        Some("Pro 月度"),
    );
    provider.quota.available = 6.0;
    provider.quota.used = 4.0;
    provider.quota.windows = vec![
        window("5 小时", 55.0, 60.0, now + 900),
        window("每日", 120.0, 200.0, now + 86_400 / 2),
        window("每月", 0.0, 0.0, now + 86_400 * 20),
    ];
    provider.capabilities.available_models = models(&MANY_MODELS);
    settle(provider, ProviderStatus::Ok, now)
}

/// Sub2API 的 Key 卡片：Key 总额度与日 / 周限额都来自 Key 自身。
fn sub2_api_key(now: i64) -> Provider {
    let mut provider = build(
        "demo-sub2api-key",
        ProviderIdentityInput {
            name: "Sub2API Key 额度".into(),
            base_url: "https://demo-sub2api.example.com".into(),
            protocol: ProviderProtocol::Sub2Api,
            ..Default::default()
        },
        key_auth("sk-demo-sub2api"),
    );
    provider.quota.scope = ProviderQuotaScope::Token;
    provider.quota.available = 3.4;
    provider.quota.used = 6.6;
    provider.quota.windows = vec![
        window("5 小时限额", 2.0, 10.0, now + 2_400),
        window("每日限额", 24.0, 30.0, now + 86_400),
        window("每周限额", 60.0, 200.0, now + 86_400 * 5),
    ];
    provider.capabilities.api_key_management_known = true;
    provider.capabilities.api_key_management_supported = true;
    provider.capabilities.available_models = models(&["gpt-5.5", "gpt-5.5-mini"]);
    settle(provider, ProviderStatus::Ok, now)
}

/// 订阅接口读取失败：只有错误说明，钱包仍显示上次读到的数字。
fn sub2_api_error(now: i64) -> Provider {
    let mut provider = sub2_api_account(
        "demo-sub2api-error",
        "Sub2API 额度读取失败",
        "https://demo-sub2api-error.example.com",
        "demo-error",
        None,
    );
    provider.quota.available = 2.0;
    provider.quota.used = 0.0;
    provider.capabilities.subscription = Some(AccountSubscription {
        plan: String::new(),
        updated_at: Some(now),
        error: Some("订阅额度接口返回 401，请重新登录或更新访问令牌".into()),
        ..Default::default()
    });
    settle(provider, ProviderStatus::Warning, now)
}

// —— 通用 API Key ——

/// 通用 OpenAI 兼容 Key：没有站点额度，只有无限 Key 额度。
fn generic_api_key(now: i64) -> Provider {
    let mut provider = build(
        "demo-api-key",
        ProviderIdentityInput {
            name: "兼容 API Key".into(),
            base_url: "https://demo-api.example.com/v1".into(),
            protocol: ProviderProtocol::Api,
            ..Default::default()
        },
        key_auth("sk-demo-generic"),
    );
    provider.quota.known = false;
    provider.quota.total_known = false;
    provider.quota.unlimited = true;
    provider.capabilities.available_models = models(&["gpt-5.5", "deepseek-v3.2"]);
    settle(provider, ProviderStatus::Ok, now)
}

/// 额度未公开：卡片显示“未公开”，Model 列表读取失败给出提示。
fn generic_api_key_unknown(now: i64) -> Provider {
    let mut provider = build(
        "demo-api-key-unknown",
        ProviderIdentityInput {
            name: "兼容 API Key 未公开额度".into(),
            base_url: "https://demo-api-unknown.example.com/v1".into(),
            protocol: ProviderProtocol::Api,
            remark: "本地自建".into(),
            ..Default::default()
        },
        key_auth("sk-demo-unknown"),
    );
    provider.quota.known = false;
    provider.quota.total_known = false;
    provider.capabilities.available_models_state = ProviderModelListState {
        scope: Some(ProviderModelScope::ApiKey),
        updated_at: Some(now.to_string()),
        error: Some("模型列表接口不可用".into()),
        fallback_reason: None,
    };
    settle(provider, ProviderStatus::Ok, now)
}

// —— OAuth 账号 ——

/// Codex 账号：两条窗口带趋势采样，钱包不限。
fn oauth_codex(now: i64) -> Provider {
    let mut provider = oauth_account(
        "demo-oauth-codex",
        "Codex 账号",
        OAuthVendor::Codex,
        "demo.codex@example.com",
        "plus",
    );
    provider.capabilities.subscription = Some(subscription(
        now,
        "Plus",
        "",
        vec![
            subscription_window("primary", "5 小时", 35.0, now + 5_400, 18_000, 18),
            subscription_window("weekly", "每周", 72.5, now + 86_400 * 4, 604_800, 40),
        ],
        true,
    ));
    provider.capabilities.available_models = models(&["gpt-5.6-sol", "gpt-5.6", "o5-mini"]);
    settle(provider, ProviderStatus::Ok, now)
}

/// Claude 账号：带模型专属窗口，详情面板会分成两组。
fn oauth_claude(now: i64) -> Provider {
    let mut provider = oauth_account(
        "demo-oauth-claude",
        "Claude 账号",
        OAuthVendor::Claude,
        "demo.claude@example.com",
        "max",
    );
    let mut scoped = subscription_window("opus", "Opus 每周", 88.0, now + 86_400 * 2, 604_800, 30);
    scoped.model = Some("claude-opus-5-5".into());
    provider.identity.username = "Demo Team".into();
    provider.capabilities.subscription = Some(subscription(
        now,
        "Max 20x",
        "Demo Team",
        vec![
            subscription_window("5h", "5 小时", 61.0, now + 7_200, 18_000, 12),
            subscription_window("weekly", "每周", 44.0, now + 86_400 * 5, 604_800, 55),
            scoped,
        ],
        false,
    ));
    provider.capabilities.available_models =
        models(&["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"]);
    settle(provider, ProviderStatus::Warning, now)
}

/// Gemini 账号：单窗口，正文最稀疏，用来检查页脚是否仍然贴底。
fn oauth_gemini(now: i64) -> Provider {
    let mut provider = oauth_account(
        "demo-oauth-gemini",
        "Gemini 账号",
        OAuthVendor::Gemini,
        "demo.gemini@example.com",
        "pro",
    );
    provider.capabilities.subscription = Some(subscription(
        now,
        "Google AI Pro",
        "",
        vec![subscription_window(
            "daily",
            "每日",
            12.0,
            now + 3_600 * 9,
            86_400,
            8,
        )],
        true,
    ));
    settle(provider, ProviderStatus::Ok, now)
}

/// Grok 账号：窗口全部不限量，主数字写“不限量”。
fn oauth_grok(now: i64) -> Provider {
    let mut provider = oauth_account(
        "demo-oauth-grok",
        "Grok 账号",
        OAuthVendor::Grok,
        "demo.grok@example.com",
        "supergrok",
    );
    let mut window = subscription_window("monthly", "每月", 0.0, now + 86_400 * 18, 2_592_000, 0);
    window.used_percent = None;
    provider.capabilities.subscription =
        Some(subscription(now, "SuperGrok", "", vec![window], false));
    settle(provider, ProviderStatus::Ok, now)
}

/// 续期异常：卡片底栏与额度旁都给出重新授权的提示。
fn oauth_renewal_error(now: i64) -> Provider {
    let mut provider = oauth_codex(now);
    provider.identity.id = "demo-oauth-renewal-error".into();
    provider.identity.name = demo_name("Codex 续期异常");
    provider.auth.access_token.clear();
    if let Some(oauth) = provider.auth.oauth.as_mut() {
        oauth.email = "demo.renewal@example.com".into();
        oauth.refresh_error = Some("刷新令牌已被服务端轮换，请重新授权".into());
    }
    settle(provider, ProviderStatus::Warning, now)
}

// —— CPA 网关 ——

/// 网关正常：两份授权、成功率、请求分布与厂商分组。
fn cpa_gateway(now: i64) -> Provider {
    let mut provider = cpa_gateway_card(
        "demo-cpa-gateway",
        "CLI 网关",
        "https://demo-cpa.example.com",
    );
    provider.capabilities.gateway = Some(CpaGatewaySnapshot {
        api_version: "v0.4".into(),
        gateway_version: "demo".into(),
        accounts: vec![
            gateway_account("demo-codex", "codex", 1_284, 6, now),
            gateway_account("demo-claude", "claude", 402, 0, now),
        ],
        total_requests: Some(1_686),
        failed_requests: Some(6),
        updated_at: Some(now),
    });
    // 网关对外提供的模型就是卡片上那一行芯片，和账号卡同一套读法。
    provider.capabilities.available_models = models(&MANY_MODELS);
    settle(provider, ProviderStatus::Ok, now)
}

/// 网关有冷却：授权正在退避，底栏提示冷却数量与最早恢复时间。
fn cpa_gateway_cooling(now: i64) -> Provider {
    let mut provider = cpa_gateway_card(
        "demo-cpa-cooling",
        "CLI 网关 · 冷却中",
        "https://demo-cpa-cooling.example.com",
    );
    let mut cooling = gateway_account("demo-gemini", "gemini", 210, 18, now);
    cooling.available = false;
    cooling.status = "quota_exceeded".into();
    cooling.status_label = "额度耗尽".into();
    cooling.quota_exceeded = true;
    cooling.quota_reason = "上游返回 429".into();
    cooling.quota_next_recover_at = Some(now + 720);
    cooling.backoff_level = Some(2);
    let mut disabled = gateway_account("demo-grok", "grok", 0, 0, now);
    disabled.available = false;
    disabled.disabled = true;
    disabled.status = "disabled".into();
    disabled.status_label = "已停用".into();
    provider.capabilities.gateway = Some(CpaGatewaySnapshot {
        api_version: "v8".into(),
        gateway_version: "demo".into(),
        accounts: vec![
            gateway_account("demo-codex", "codex", 940, 3, now),
            cooling,
            disabled,
        ],
        total_requests: Some(1_171),
        failed_requests: Some(21),
        updated_at: Some(now),
    });
    provider.capabilities.available_models =
        models(&["gpt-5.6-sol", "claude-sonnet-5-5", "gemini-3-pro"]);
    settle(provider, ProviderStatus::Warning, now)
}

/// 网关未连接：账号、模型都还没有读回来。
fn cpa_gateway_offline() -> Provider {
    let mut provider = cpa_gateway_card(
        "demo-cpa-offline",
        "CLI 网关 · 未连接",
        "https://demo-cpa-offline.example.com",
    );
    provider.quota.known = false;
    provider.quota.total_known = false;
    provider.runtime = ProviderRuntime {
        enabled: true,
        status: ProviderStatus::Error,
        error_message: Some("无法连接网关管理接口".into()),
    };
    provider
}

// —— 构造辅助 ——

/// 用新建中转站的输入路径构造卡片，配额、能力等字段再按场景覆盖。
/// 这样演示数据始终满足 `from_input` 里的协议规则，不手抄一份默认值；
/// 测活与临时 CLI 入口按服务端真实的开关判断，演示里一律打开。
/// 所有演示卡片都从这里出来，名称统一加“演示 ·”前缀。
fn build(id: &str, mut identity: ProviderIdentityInput, auth: ProviderAuth) -> Provider {
    identity.name = demo_name(&identity.name);
    Provider::from_input(
        ProviderInput {
            identity,
            auth,
            liveness: liveness_input(),
            ..Default::default()
        },
        id.into(),
    )
}

fn new_api_account(
    id: &str,
    name: &str,
    base_url: &str,
    login_username: &str,
    user_id: &str,
) -> Provider {
    let mut provider = build(
        id,
        ProviderIdentityInput {
            name: name.into(),
            base_url: base_url.into(),
            protocol: ProviderProtocol::NewApi,
            ..Default::default()
        },
        account_auth(AuthMode::Session, login_username),
    );
    provider.identity.username = login_username.into();
    provider.identity.user_id = user_id.into();
    provider
}

fn sub2_api_account(
    id: &str,
    name: &str,
    base_url: &str,
    login_username: &str,
    remark: Option<&str>,
) -> Provider {
    let mut provider = build(
        id,
        ProviderIdentityInput {
            name: name.into(),
            base_url: base_url.into(),
            protocol: ProviderProtocol::Sub2Api,
            remark: remark.unwrap_or_default().into(),
            ..Default::default()
        },
        account_auth(AuthMode::AccessToken, login_username),
    );
    provider.auth.refresh_token = "demo-refresh-token".into();
    provider.identity.username = login_username.into();
    provider
}

/// OAuth 账号（各厂商）：订阅窗口来自账号侧，没有钱包与签到。
fn oauth_account(id: &str, name: &str, vendor: OAuthVendor, email: &str, plan: &str) -> Provider {
    let mut auth = account_auth(AuthMode::AccessToken, "");
    auth.source = AuthSource::Oauth;
    let mut oauth = OAuthCredential::new(vendor);
    oauth.email = email.into();
    oauth.plan = plan.into();
    auth.oauth = Some(oauth);
    let mut provider = build(
        id,
        ProviderIdentityInput {
            name: name.into(),
            base_url: vendor.base_url().into(),
            protocol: ProviderProtocol::OAuth,
            ..Default::default()
        },
        auth,
    );
    provider.identity.username = email.into();
    provider.quota.known = false;
    provider.quota.total_known = false;
    provider
}

/// CLIProxyAPI 网关：卡片按网关里的凭证展示。
fn cpa_gateway_card(id: &str, name: &str, base_url: &str) -> Provider {
    let mut provider = build(
        id,
        ProviderIdentityInput {
            name: name.into(),
            base_url: base_url.into(),
            protocol: ProviderProtocol::CliProxyApi,
            ..Default::default()
        },
        account_auth(AuthMode::AccessToken, ""),
    );
    provider.quota.known = false;
    provider.quota.total_known = false;
    provider
}

fn account_auth(mode: AuthMode, login_username: &str) -> ProviderAuth {
    ProviderAuth {
        mode,
        source: AuthSource::Password,
        access_token: "demo-access-token".into(),
        session_cookie: "demo-session".into(),
        login_username: login_username.into(),
        login_password: "demo-password".into(),
        ..ProviderInput::default().auth
    }
}

/// 测活在演示里打开：卡片上才会出现「测活明细」入口。请求本身不会发生，
/// 因为全局测活开关保持关闭，卡片也没有可用的真实凭据。
fn liveness_input() -> ProviderLivenessInput {
    ProviderLivenessInput {
        use_global: true,
        enabled: true,
        ..serde_json::from_str("{}").expect("ProviderLivenessInput 的默认字段全部可缺省")
    }
}

fn key_auth(api_key: &str) -> ProviderAuth {
    ProviderAuth {
        mode: AuthMode::ApiKey,
        source: AuthSource::Manual,
        api_key: api_key.into(),
        ..ProviderInput::default().auth
    }
}

fn window(label: &str, used: f64, limit: f64, resets_at: i64) -> ProviderQuotaWindow {
    ProviderQuotaWindow {
        label: label.into(),
        used,
        limit,
        resets_at: Some(resets_at),
    }
}

fn subscription(
    now: i64,
    plan: &str,
    organization: &str,
    windows: Vec<AccountQuotaWindow>,
    credits_unlimited: bool,
) -> AccountSubscription {
    AccountSubscription {
        plan: plan.into(),
        organization: organization.into(),
        project_id: String::new(),
        windows,
        credits: None,
        credits_unit: None,
        credits_unlimited,
        updated_at: Some(now),
        error: None,
    }
}

/// 一条订阅窗口，带 `samples` 个趋势采样：卡片和详情面板的迷你折线靠它。
fn subscription_window(
    id: &str,
    label: &str,
    used_percent: f64,
    resets_at: i64,
    window_seconds: i64,
    samples: i32,
) -> AccountQuotaWindow {
    let step = 1_800;
    let history = (0..samples.max(0))
        .map(|index| {
            // 逐渐往上爬，最后一个采样就是当前比例，折线看得出趋势。
            let progress = f64::from(samples - index) / f64::from(samples.max(1));
            AccountQuotaSample {
                at: resets_at - window_seconds + i64::from(index + 1) * step,
                used_percent: (used_percent * (1.0 - progress * 0.6)).clamp(0.0, 100.0),
            }
        })
        .collect();
    AccountQuotaWindow {
        id: id.into(),
        label: label.into(),
        model: None,
        used_percent: Some(used_percent),
        used: None,
        limit: None,
        unit: None,
        reset_at: Some(resets_at),
        window_seconds: Some(window_seconds),
        history,
    }
}

fn gateway_account(
    name: &str,
    provider: &str,
    success: u64,
    failed: u64,
    now: i64,
) -> CpaAccountSummary {
    CpaAccountSummary {
        name: name.into(),
        provider: provider.into(),
        email: format!("{name}@example.com"),
        status: "ok".into(),
        status_label: "正常".into(),
        available: true,
        can_download: true,
        success: Some(success),
        failed: Some(failed),
        last_refresh: Some(now - 600),
        ..Default::default()
    }
}

fn models(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

const MANY_MODELS: [&str; 12] = [
    "gpt-5.6-sol",
    "gpt-5.6",
    "gpt-5.6-mini",
    "deepseek-v3.2",
    "deepseek-r2",
    "claude-sonnet-5-5",
    "claude-opus-5-5",
    "claude-haiku-5-5",
    "gemini-3-pro",
    "kimi-k3",
    "glm-5",
    "qwen3-max",
];

/// 卡片以“已同步”结束：给出状态和同步时间，但不留下任何会自动重试的标记。
fn settle(mut provider: Provider, status: ProviderStatus, now: i64) -> Provider {
    provider.runtime = ProviderRuntime {
        enabled: true,
        status,
        error_message: None,
    };
    provider.automation.last_synced_at = Some(now.to_string());
    provider
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(1_759_000_000)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// 演示数据要覆盖每一类卡片与关键形态，否则开发版看不出问题。
    #[test]
    fn sample_data_covers_every_card_kind() {
        let providers = sample_providers();
        let mut protocols: BTreeMap<String, usize> = BTreeMap::new();
        for provider in &providers {
            let key = format!("{:?}", provider.identity.protocol);
            *protocols.entry(key).or_default() += 1;
        }
        for protocol in ["NewApi", "Sub2Api", "Api", "OAuth", "CliProxyApi"] {
            assert!(
                protocols.get(protocol).copied().unwrap_or(0) >= 1,
                "演示数据缺少 {protocol} 卡片"
            );
        }
        assert_eq!(
            providers.len(),
            providers
                .iter()
                .map(|provider| provider.identity.id.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "演示数据里有重复的卡片 ID"
        );
    }

    /// OAuth 卡片的地址来自厂商常量，其余全部落在编造的 example.com 域名里，
    /// 名称统一带“演示 ·”前缀，和真实站点一眼分得开。
    #[test]
    fn sample_cards_stay_offline_and_labeled() {
        let providers = sample_providers();
        assert!(providers.iter().all(|provider| {
            provider.identity.base_url.contains(".example.com")
                || provider.identity.protocol == ProviderProtocol::OAuth
        }));
        assert!(providers
            .iter()
            .all(|provider| provider.identity.name.starts_with("演示 · ")));
    }

    /// 演示卡片追加到现有配置上：真实站点与设置保留，重复执行不会越加越多。
    #[test]
    fn merge_keeps_real_providers_and_is_idempotent() {
        let mut real = sample_providers().remove(0);
        real.identity.id = "real-provider".into();
        real.identity.name = "真实站点".into();
        let settings = crate::models::AppSettings {
            auto_refresh_enabled: true,
            ..crate::models::AppSettings::default()
        };
        let mut data = AppData::new_current(vec![real], settings);

        let merged = merge_into(&mut data);
        let expected = sample_providers().len() + 1;
        assert_eq!(merged, expected - 1);
        assert_eq!(data.providers.len(), expected);
        assert!(data
            .providers
            .iter()
            .any(|provider| provider.identity.id == "real-provider"
                && provider.identity.name == "真实站点"));
        assert!(
            !data.settings.auto_refresh_enabled,
            "演示会话必须关闭自动刷新"
        );

        merge_into(&mut data);
        assert_eq!(data.providers.len(), expected, "重复写入不应重复追加");
    }

    /// 加载阶段只在开关开启且盘上还没有演示卡片时才并入一次。
    #[test]
    fn merge_on_load_requires_flag_and_absent_samples() {
        let mut real = sample_providers().remove(0);
        real.identity.id = "real-provider".into();
        real.identity.name = "真实站点".into();
        let mut data = AppData::new_current(vec![real], Default::default());
        std::env::remove_var(ENV_MERGE_ON_LOAD);
        assert_eq!(merge_on_load(&mut data), None, "未开启开关时不并入");

        std::env::set_var(ENV_MERGE_ON_LOAD, "1");
        assert_eq!(merge_on_load(&mut data), Some(sample_providers().len()));
        assert_eq!(merge_on_load(&mut data), None, "已有演示卡片时不再重复并入");
        std::env::remove_var(ENV_MERGE_ON_LOAD);
    }
}
