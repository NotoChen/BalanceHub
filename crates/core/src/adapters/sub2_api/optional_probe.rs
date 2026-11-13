//! Sub2API 可选接口探测。
//!
//! 账号接口按顺序读取，避免并发请求同时轮换令牌；每次成功都带着最新凭据
//! 继续下一次探测，最终把更新后的 Provider 交回调用方持久化。

use crate::{
    adapters::transport::ProviderTransport,
    models::{AuthMode, Provider, ProviderOptionalCapability, ProviderOptionalCapabilityState},
};
use reqwest::Method;

use super::{auth::request_account_json, usage::usage_dates};

pub async fn probe_optional(
    client: &ProviderTransport,
    provider: &Provider,
    pricing: ProviderOptionalCapabilityState,
) -> (Provider, Vec<ProviderOptionalCapabilityState>) {
    let mut states = vec![pricing];
    if matches!(provider.auth.mode, AuthMode::ApiKey) {
        states.extend([
            ProviderOptionalCapabilityState::unsupported(
                ProviderOptionalCapability::Usage,
                "API Key 认证不提供站点级用量趋势",
            ),
            ProviderOptionalCapabilityState::unsupported(
                ProviderOptionalCapability::RequestLogs,
                "API Key 认证不提供站点请求日志",
            ),
            ProviderOptionalCapabilityState::unsupported(
                ProviderOptionalCapability::Announcements,
                "API Key 认证不提供站点公告",
            ),
            ProviderOptionalCapabilityState::unsupported(
                ProviderOptionalCapability::Subscription,
                "API Key 认证不提供账号订阅接口",
            ),
            ProviderOptionalCapabilityState::unsupported(
                ProviderOptionalCapability::Groups,
                "API Key 认证不提供账号分组接口",
            ),
        ]);
        return (provider.clone(), states);
    }

    let mut authenticated = provider.clone();
    for (capability, path, label) in [
        (
            ProviderOptionalCapability::Usage,
            format!(
                "/usage/dashboard/trend?start_date={}&end_date={}",
                usage_dates("24h").0,
                usage_dates("24h").1
            ),
            "用量趋势",
        ),
        (
            ProviderOptionalCapability::RequestLogs,
            format!(
                "/usage?page=1&page_size=1&start_date={}&end_date={}",
                usage_dates("30d").0,
                usage_dates("30d").1
            ),
            "请求日志",
        ),
        (
            ProviderOptionalCapability::Announcements,
            "/announcements".to_string(),
            "站点公告",
        ),
        (
            ProviderOptionalCapability::Subscription,
            "/subscriptions/progress".to_string(),
            "订阅接口",
        ),
        (
            ProviderOptionalCapability::Groups,
            "/groups/available".to_string(),
            "分组接口",
        ),
    ] {
        match request_account_json(client, &authenticated, Method::GET, &path, None, label).await {
            Ok((next, _)) => {
                authenticated = next;
                states.push(ProviderOptionalCapabilityState::supported(
                    capability,
                    format!("{label}接口可用"),
                ));
            }
            Err(message) => states.push(classify_error(capability, label, message)),
        }
    }
    (authenticated, states)
}

fn classify_error(
    capability: ProviderOptionalCapability,
    label: &str,
    message: String,
) -> ProviderOptionalCapabilityState {
    if message.starts_with("HTTP 404") || message.starts_with("HTTP 501") {
        let status = message
            .split_whitespace()
            .nth(1)
            .unwrap_or("404")
            .trim_end_matches(':');
        return ProviderOptionalCapabilityState::unsupported(
            capability,
            format!("站点未提供{label}接口（HTTP {status}）"),
        );
    }
    if message.starts_with("HTTP 401") || message.starts_with("HTTP 403") {
        return ProviderOptionalCapabilityState::unknown(
            capability,
            format!("{label}接口需要有效的账号登录：{message}"),
        );
    }
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("not enabled")
        || normalized.contains("not supported")
        || normalized.contains("disabled")
        || message.contains("未开启")
        || message.contains("不支持")
    {
        return ProviderOptionalCapabilityState::unsupported(capability, message);
    }
    ProviderOptionalCapabilityState::unknown(capability, message)
}
