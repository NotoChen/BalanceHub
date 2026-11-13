//! NewAPI 可选接口探测。
//!
//! 这些接口多数是账号级能力，旧站点或裁剪版站点可能缺失。探测只做一次最小
//! 读取：成功即支持，明确的 404 / 未开启响应记为不支持，认证或网络错误记为
//! 未知，避免一次抖动把入口永久隐藏。

use crate::{
    adapters::transport::ProviderTransport,
    models::{AuthMode, Provider, ProviderOptionalCapability, ProviderOptionalCapabilityState},
};
use reqwest::{
    header::{ACCEPT, ORIGIN, REFERER, USER_AGENT},
    Method,
};
use serde_json::Value;
use std::time::Duration;

use super::http::{
    build_url, build_user_request, normalize_base_url, provider_user_management_context,
    USER_AGENT_VALUE,
};
use super::response::{send_text, trim_message};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

enum ProbeFailure {
    Unsupported(String),
    Failed(String),
}

/// 探测 NewAPI 的可选账号接口。API Key 卡片只有公开公告可探测，其余能力
/// 直接标记为不支持并给出认证边界说明。
pub async fn probe_optional(
    client: &ProviderTransport,
    provider: &Provider,
    pricing: ProviderOptionalCapabilityState,
) -> Vec<ProviderOptionalCapabilityState> {
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
                ProviderOptionalCapability::Subscription,
                "API Key 认证不提供账号订阅接口",
            ),
            ProviderOptionalCapabilityState::unsupported(
                ProviderOptionalCapability::Groups,
                "API Key 认证不提供账号分组接口",
            ),
        ]);
        states.push(probe_announcements(client, provider).await);
        return states;
    }

    let (usage, logs, announcements, subscription, groups) = tokio::join!(
        probe_usage(client, provider),
        probe_request_logs(client, provider),
        probe_announcements(client, provider),
        probe_subscription(client, provider),
        probe_groups(client, provider),
    );
    states.extend([usage, logs, announcements, subscription, groups]);
    states
}

async fn probe_usage(
    client: &ProviderTransport,
    provider: &Provider,
) -> ProviderOptionalCapabilityState {
    probe_user_endpoint(
        client,
        provider,
        "/api/data/self",
        &format!(
            "?start_timestamp={}&end_timestamp={}",
            now_seconds().saturating_sub(3_600),
            now_seconds()
        ),
        "用量趋势",
        ProviderOptionalCapability::Usage,
    )
    .await
}

async fn probe_request_logs(
    client: &ProviderTransport,
    provider: &Provider,
) -> ProviderOptionalCapabilityState {
    probe_user_endpoint(
        client,
        provider,
        "/api/log/self",
        &format!(
            "?p=1&page_size=1&start_timestamp={}&end_timestamp={}",
            now_seconds().saturating_sub(86_400),
            now_seconds()
        ),
        "请求日志",
        ProviderOptionalCapability::RequestLogs,
    )
    .await
}

async fn probe_subscription(
    client: &ProviderTransport,
    provider: &Provider,
) -> ProviderOptionalCapabilityState {
    probe_user_endpoint(
        client,
        provider,
        "/api/subscription/self",
        "",
        "订阅接口",
        ProviderOptionalCapability::Subscription,
    )
    .await
}

async fn probe_groups(
    client: &ProviderTransport,
    provider: &Provider,
) -> ProviderOptionalCapabilityState {
    probe_user_endpoint(
        client,
        provider,
        "/api/user/self/groups",
        "",
        "分组接口",
        ProviderOptionalCapability::Groups,
    )
    .await
}

async fn probe_user_endpoint(
    client: &ProviderTransport,
    provider: &Provider,
    path: &str,
    query: &str,
    context: &str,
    capability: ProviderOptionalCapability,
) -> ProviderOptionalCapabilityState {
    let Ok((base_url, api_user, credential)) = provider_user_management_context(provider) else {
        return ProviderOptionalCapabilityState::unknown(
            capability,
            "当前认证方式不能读取账号级接口",
        );
    };
    let Ok(url) = build_url(&base_url, &format!("{path}{query}")) else {
        return ProviderOptionalCapabilityState::unknown(capability, "接口地址无效");
    };
    let request = build_user_request(client, Method::GET, url, &base_url, &api_user, credential)
        .timeout(REQUEST_TIMEOUT);
    classify_probe(client, request, context, capability).await
}

async fn probe_announcements(
    client: &ProviderTransport,
    provider: &Provider,
) -> ProviderOptionalCapabilityState {
    let base_url = normalize_base_url(&provider.identity.base_url);
    if base_url.is_empty() {
        return ProviderOptionalCapabilityState::unknown(
            ProviderOptionalCapability::Announcements,
            "缺少中转站地址",
        );
    }
    let capability = ProviderOptionalCapability::Announcements;
    let notice = probe_public_json(client, &base_url, "/api/notice", "站点公告", capability).await;
    if notice.known && notice.supported {
        return notice;
    }
    // 新版 NewAPI 的公告面板挂在 `/api/status` 上；只缺 `/api/notice` 的站点
    // 公告仍然可用，不能因为一个旧接口 404 就把公告源整个关掉。
    let status = probe_public_json(client, &base_url, "/api/status", "站点状态", capability).await;
    if status.known && status.supported {
        return status;
    }
    if notice.known {
        notice
    } else if status.known {
        status
    } else {
        notice
    }
}

async fn probe_public_json(
    client: &ProviderTransport,
    base_url: &str,
    path: &str,
    context: &str,
    capability: ProviderOptionalCapability,
) -> ProviderOptionalCapabilityState {
    let Ok(url) = build_url(base_url, path) else {
        return ProviderOptionalCapabilityState::unknown(capability, "接口地址无效");
    };
    let request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, base_url)
        .header(REFERER, format!("{base_url}/"))
        .timeout(REQUEST_TIMEOUT);
    classify_probe(client, request, context, capability).await
}

async fn classify_probe(
    client: &ProviderTransport,
    request: reqwest::RequestBuilder,
    context: &str,
    capability: ProviderOptionalCapability,
) -> ProviderOptionalCapabilityState {
    let (status, body) = match send_text(client, request, context).await {
        Ok(response) => response,
        Err(error) => return ProviderOptionalCapabilityState::unknown(capability, error),
    };
    if matches!(status.as_u16(), 404 | 501) {
        return ProviderOptionalCapabilityState::unsupported(
            capability,
            format!("站点未提供{context}接口（HTTP {}）", status.as_u16()),
        );
    }
    if !status.is_success() {
        return ProviderOptionalCapabilityState::unknown(
            capability,
            format!("HTTP {}: {}", status.as_u16(), trim_message(&body)),
        );
    }
    match parse_probe_body(&body) {
        Ok(()) => {
            ProviderOptionalCapabilityState::supported(capability, format!("{context}接口可用"))
        }
        Err(ProbeFailure::Unsupported(message)) => {
            ProviderOptionalCapabilityState::unsupported(capability, message)
        }
        Err(ProbeFailure::Failed(message)) => {
            ProviderOptionalCapabilityState::unknown(capability, message)
        }
    }
}

fn parse_probe_body(body: &str) -> Result<(), ProbeFailure> {
    let decoded = serde_json::from_str::<Value>(body)
        .map_err(|error| ProbeFailure::Failed(format!("解析响应失败: {error}")))?;
    // 与业务读取共用同一套成功判定，但这里只关心接口是否存在。
    if decoded.get("success").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let message = decoded
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("接口返回失败")
        .trim()
        .to_string();
    if looks_unsupported(&message) {
        Err(ProbeFailure::Unsupported(message))
    } else {
        Err(ProbeFailure::Failed(message))
    }
}

fn looks_unsupported(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("not found")
        || normalized.contains("not enabled")
        || normalized.contains("not supported")
        || normalized.contains("disabled")
        || message.contains("未开启")
        || message.contains("不支持")
        || message.contains("不存在")
        || message.contains("无权")
        || message.contains("permission")
}

fn now_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}
