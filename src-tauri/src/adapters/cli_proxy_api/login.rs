//! Signing in to a provider **on the gateway itself**.
//!
//! CPA generates the authorization URL and holds the pending session, so the
//! account never passes through this app: nothing is downloaded, stored or
//! refreshed locally. A gateway on the same machine completes the flow by
//! itself; a remote one hands the browser a loopback redirect that cannot
//! reach it, which is why the caller can submit the callback by hand.
use super::{base_url, require_key};
use crate::{adapters::oauth::http, models::*, network};
use reqwest::{Client, RequestBuilder, Response};
use serde_json::{json, Value};

/// Built-in providers the gateway signs in without a plugin. The ids are the
/// gateway's own keys, not this app's vendor names.
const BUILT_IN: [(&str, &str); 8] = [
    ("codex", "Codex"),
    ("claude", "Claude"),
    ("antigravity", "Antigravity"),
    ("kimi", "Kimi"),
    ("kimi-ai", "Kimi AI"),
    ("xai", "Grok (xAI)"),
    ("devin", "Devin"),
    ("meta", "Meta"),
];

/// Which management generation serves the login flow. v8 has one route family
/// for every provider; v0 keeps one auth-url route per provider. Both can
/// cancel a pending session, under different route spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginApi {
    V0,
    V8,
}

impl LoginApi {
    /// An unknown generation reads as v8: those routes exist on every gateway
    /// that can answer at all, and an absent one falls back to v0 anyway.
    fn from_version(raw: &str) -> Self {
        if raw == "v0" {
            Self::V0
        } else {
            Self::V8
        }
    }
    fn other(self) -> Self {
        if self == Self::V0 {
            Self::V8
        } else {
            Self::V0
        }
    }
    fn start_url(self, base: &str, key: &str) -> String {
        match self {
            Self::V8 => format!("{base}/v8/management/oauth/auth-url"),
            Self::V0 => format!("{base}/v0/management/{}-auth-url", v0_key(key)),
        }
    }
    fn status_url(self, base: &str) -> String {
        match self {
            Self::V8 => format!("{base}/v8/management/oauth/status"),
            Self::V0 => format!("{base}/v0/management/get-auth-status"),
        }
    }
    fn callback_url(self, base: &str) -> String {
        match self {
            Self::V8 => format!("{base}/v8/management/oauth/callback"),
            Self::V0 => format!("{base}/v0/management/oauth-callback"),
        }
    }
    /// v0 named the cancel route before the flow was grouped under `oauth/`,
    /// which is the only difference: both take `state` and answer the same way.
    fn cancel_url(self, base: &str) -> String {
        match self {
            Self::V8 => format!("{base}/v8/management/oauth/session"),
            Self::V0 => format!("{base}/v0/management/oauth-session"),
        }
    }
}

/// v0 names the Claude route after the vendor Claude exposes it as; the other
/// built-in keys are the same in both generations.
fn v0_key(key: &str) -> &str {
    if key == "claude" {
        "anthropic"
    } else {
        key
    }
}

/// The provider key is a query value and, for plugin providers, reaches plugin
/// lookup code, so it is held to the same shape as a plugin id.
fn provider_key(raw: &str) -> Result<String, String> {
    let key = raw.trim().to_ascii_lowercase();
    let valid = !key.is_empty()
        && key.len() <= 64
        && key
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !valid {
        return Err("CPA 授权服务标识无效".into());
    }
    Ok(key)
}

/// The state identifies the pending session in every follow-up call. CPA
/// applies the same rule to it, so anything else is refused before it reaches
/// a query string.
fn session_state(raw: &str) -> Result<String, String> {
    let state = raw.trim();
    let valid = !state.is_empty()
        && state.len() <= 128
        && state
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !valid {
        return Err("CPA 授权会话标识无效".into());
    }
    Ok(state.to_owned())
}

/// Sends a login request to the generation the session belongs to, and retries
/// the other one only when that route is absent. A gateway that was upgraded
/// or downgraded between two steps still answers, while a real refusal (such
/// as an expired state) is never mistaken for a missing route.
async fn login_request(
    preferred: LoginApi,
    mut request: impl FnMut(LoginApi) -> RequestBuilder,
) -> Result<(LoginApi, Response), String> {
    let response = request(preferred)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    if !matches!(response.status().as_u16(), 404 | 405) {
        return Ok((preferred, response));
    }
    let other = preferred.other();
    let response = request(other)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    Ok((other, response))
}

/// Reads a login answer. CPA explains a refused step in the body while
/// answering with a 4xx, and those reasons are the only useful thing it says,
/// so they are read before the status is turned into a generic message.
async fn login_response(response: Response, context: &str) -> Result<Value, String> {
    let status = response.status();
    let text = network::read_http_text(response, context)
        .await
        .map_err(|_| format!("{context}响应读取失败"))?;
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(value);
    }
    Err(match status.as_u16() {
        401 | 403 => http::http_error(context, status.as_u16()),
        _ => known_reason(&value).unwrap_or_else(|| http::http_error(context, status.as_u16())),
    })
}

/// The reason a login step failed, in the panel's words. Only reasons this app
/// knows are translated: an internal gateway message would otherwise reach the
/// panel as raw Go text.
fn known_reason(value: &Value) -> Option<String> {
    Some(
        match gateway_message(value)? {
            "provider is required" => "请选择授权服务",
            "provider_not_found" => "CPA 不支持该授权服务",
            "invalid state" | "state is required" => "授权会话标识无效",
            "unknown or expired state" => "授权会话已过期，请重新开始",
            "oauth flow is already completed" => "该授权已完成，可直接刷新卡片",
            "oauth flow is not pending" => "授权会话已结束，请重新开始",
            "code or error is required" => "回调地址里没有授权码",
            "invalid redirect_url" => "回调地址格式不正确",
            "provider does not match state" => "回调地址不属于本次授权的服务",
            "unsupported provider" => "CPA 不支持该授权服务",
            "failed to persist oauth callback" => "CPA 未能保存回调结果",
            "Authentication failed" => "授权被服务商拒绝",
            "callback server unavailable"
            | "failed to start callback server"
            | "failed to generate authorization url" => {
                "CPA 无法准备回调地址，请检查网关端口是否可用"
            }
            // The Claude flow reports these on the pending session itself.
            "Bad request" => "授权被拒绝，请重新开始",
            "State code error" => "授权回调与本次会话不匹配，请重新开始",
            "Failed to exchange authorization code for tokens" => "CPA 未能用授权码换取令牌",
            "Failed to save authentication tokens" => "CPA 未能保存授权令牌",
            _ => return None,
        }
        .into(),
    )
}

/// A 200 answer can still refuse the step. A plugin fills the reason itself,
/// so an unknown one is passed through rather than replaced by a guess.
fn failure_message(value: &Value, fallback: &str) -> String {
    known_reason(value)
        .or_else(|| gateway_message(value).map(str::to_owned))
        .unwrap_or_else(|| fallback.into())
}

/// The reason the gateway put in its answer, which a plugin fills in its own
/// words. Empty when it sent none.
fn gateway_message(value: &Value) -> Option<&str> {
    value
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// Lists the providers this gateway can sign in to: the built-in ones plus one
/// entry per enabled plugin that serves its own login.
pub(crate) async fn login_providers(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<Vec<CpaLoginProvider>, String> {
    let plugins = super::plugins(settings, provider).await?;
    let mut providers: Vec<CpaLoginProvider> = BUILT_IN
        .iter()
        .map(|(id, label)| CpaLoginProvider {
            id: (*id).into(),
            label: (*label).into(),
            plugin: None,
        })
        .collect();
    for plugin in plugins.plugins {
        // The login of a plugin that serves one is only reachable while the
        // plugin runs, and a disabled plugin would answer 404.
        if !plugin.effective_enabled
            || plugin.oauth_provider.is_empty()
            || providers.iter().any(|p| p.id == plugin.oauth_provider)
        {
            continue;
        }
        providers.push(CpaLoginProvider {
            id: plugin.oauth_provider,
            label: plugin.name,
            plugin: Some(plugin.id),
        });
    }
    Ok(providers)
}

/// Starts a login on the gateway and returns the URL to open. `is_webui` asks
/// the gateway to listen for the loopback callback itself, which completes the
/// flow with no manual step when the gateway runs on this machine.
pub(crate) async fn start_login(
    client: &Client,
    provider: &Provider,
    gateway_provider: &str,
) -> Result<CpaLoginStart, String> {
    require_key(provider)?;
    let key = provider_key(gateway_provider)?;
    let base = base_url(provider)?;
    let (api, response) = login_request(LoginApi::V8, |api| {
        let request = client.get(api.start_url(&base, &key));
        let request = if api == LoginApi::V8 {
            request.query(&[("provider", key.as_str())])
        } else {
            request
        };
        request
            .query(&[("is_webui", "true")])
            .bearer_auth(&provider.auth.access_token)
    })
    .await?;
    let value = login_response(response, "在 CPA 上开始授权").await?;
    let text = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    if value.get("status").and_then(Value::as_str) == Some("error") {
        return Err(failure_message(&value, "CPA 无法开始授权"));
    }
    let url = text("url").ok_or("CPA 未返回授权地址")?;
    let state = session_state(&text("state").unwrap_or_default())?;
    Ok(CpaLoginStart {
        state,
        url,
        user_code: text("user_code"),
        expires_in: value.get("expires_in").and_then(Value::as_u64),
        flow: text("flow").unwrap_or_else(|| "browser".into()),
        api_version: match api {
            LoginApi::V8 => "v8".into(),
            LoginApi::V0 => "v0".into(),
        },
    })
}

/// Reads the pending session. A gateway that no longer knows the state answers
/// `error` with "unknown or expired state", which the caller shows as a
/// restart prompt rather than as a failure to retry.
pub(crate) async fn login_status(
    client: &Client,
    provider: &Provider,
    state: &str,
    api_version: &str,
) -> Result<CpaLoginStatus, String> {
    require_key(provider)?;
    let state = session_state(state)?;
    let base = base_url(provider)?;
    let (_, response) = login_request(LoginApi::from_version(api_version), |api| {
        client
            .get(api.status_url(&base))
            .query(&[("state", state.as_str())])
            .bearer_auth(&provider.auth.access_token)
    })
    .await?;
    let value = login_response(response, "读取 CPA 授权进度").await?;
    Ok(match value.get("status").and_then(Value::as_str) {
        Some("ok") => CpaLoginStatus::Completed,
        Some("wait") => CpaLoginStatus::Waiting,
        _ => CpaLoginStatus::Failed {
            message: failure_message(&value, "CPA 授权失败"),
        },
    })
}

/// Hands the gateway the code the browser ended on. The panel accepts either
/// the full redirect URL or the bare code: a URL carries its own state, so it
/// is forwarded as `redirect_url`; a bare code is paired with the state this
/// login started with.
pub(crate) async fn submit_login_callback(
    client: &Client,
    provider: &Provider,
    gateway_provider: &str,
    state: &str,
    api_version: &str,
    input: &str,
) -> Result<(), String> {
    require_key(provider)?;
    let key = provider_key(gateway_provider)?;
    let state = session_state(state)?;
    let input = input.trim();
    if input.is_empty() {
        return Err("请粘贴回调地址或授权码".into());
    }
    let is_url = input.starts_with("http://") || input.starts_with("https://");
    let body = if is_url {
        json!({ "provider": key, "state": state, "redirect_url": input })
    } else {
        json!({ "provider": key, "state": state, "code": input })
    };
    let base = base_url(provider)?;
    let (_, response) = login_request(LoginApi::from_version(api_version), |api| {
        client
            .post(api.callback_url(&base))
            .bearer_auth(&provider.auth.access_token)
            .json(&body)
    })
    .await?;
    let value = login_response(response, "提交 CPA 授权回调").await?;
    if value.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(failure_message(&value, "CPA 未确认授权回调"));
    }
    Ok(())
}

/// Drops the pending session so the gateway stops waiting. Both generations can
/// do it; a gateway too old for either route answers 404, which is the state
/// the caller wanted anyway, and lets the session expire on its own.
pub(crate) async fn cancel_login(
    client: &Client,
    provider: &Provider,
    state: &str,
) -> Result<(), String> {
    require_key(provider)?;
    let state = session_state(state)?;
    let base = base_url(provider)?;
    let (_, response) = login_request(LoginApi::V8, |api| {
        client
            .delete(api.cancel_url(&base))
            .query(&[("state", state.as_str())])
            .bearer_auth(&provider.auth.access_token)
    })
    .await?;
    if matches!(response.status().as_u16(), 200 | 204 | 404 | 405 | 409) {
        return Ok(());
    }
    let value = login_response(response, "取消 CPA 授权").await?;
    if value.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(failure_message(&value, "CPA 未确认取消授权"));
    }
    Ok(())
}
