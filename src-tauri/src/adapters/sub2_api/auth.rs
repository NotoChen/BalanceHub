use crate::models::{AuthMode, Provider};
use reqwest::{Client, Method};
use serde_json::{json, Value};

use super::{
    json::{integer_field, string_field},
    response::{api_url, request_json, Credential},
};

pub(super) async fn authenticate_account(
    client: &Client,
    provider: &Provider,
) -> Result<(Provider, Value), String> {
    request_account_json(
        client,
        provider,
        Method::GET,
        "/user/profile",
        None,
        "读取 Sub2API 用户信息",
    )
    .await
}

pub(super) async fn request_account_json(
    client: &Client,
    provider: &Provider,
    method: Method,
    path: &str,
    body: Option<Value>,
    context: &str,
) -> Result<(Provider, Value), String> {
    let authenticated = authenticate_account_if_needed(client, provider).await?;
    let url = api_url(&provider.identity.base_url, path)?;
    let first = request_json(
        client,
        method.clone(),
        url,
        Some(Credential::Jwt(authenticated.auth.access_token.clone())),
        body.clone(),
        context,
    )
    .await;
    match first {
        Ok(value) => Ok((authenticated, value)),
        Err(first_error) if is_auth_failure(&first_error) && has_login_credentials(provider) => {
            let refreshed = login(client, provider)
                .await
                .map_err(|login_error| format!("{first_error}；重新登录失败: {login_error}"))?;
            let value = request_json(
                client,
                method,
                api_url(&provider.identity.base_url, path)?,
                Some(Credential::Jwt(refreshed.auth.access_token.clone())),
                body,
                context,
            )
            .await
            .map_err(|retry_error| format!("{first_error}；重新登录后重试失败: {retry_error}"))?;
            Ok((refreshed, value))
        }
        Err(error) => Err(error),
    }
}

async fn authenticate_account_if_needed(
    client: &Client,
    provider: &Provider,
) -> Result<Provider, String> {
    if matches!(provider.auth.mode, AuthMode::ApiKey) {
        return Err("API Key 认证不支持账号管理".to_string());
    }
    if matches!(provider.auth.mode, AuthMode::Session) {
        return Err("Sub2API 不使用 Cookie 认证".to_string());
    }
    if !provider.auth.access_token.trim().is_empty() {
        return Ok(provider.clone());
    }
    if matches!(
        provider.auth.mode,
        AuthMode::Password | AuthMode::AccessToken
    ) && has_login_credentials(provider)
    {
        return login(client, provider).await;
    }
    Err("缺少 Sub2API 访问令牌".to_string())
}

async fn login(client: &Client, provider: &Provider) -> Result<Provider, String> {
    let username = provider.auth.login_username.trim();
    let password = provider.auth.login_password.trim();
    if username.is_empty() || password.is_empty() {
        return Err("Sub2API 账号密码模式需要填写邮箱和密码".to_string());
    }
    let data = request_json(
        client,
        Method::POST,
        api_url(&provider.identity.base_url, "/auth/login")?,
        None,
        Some(json!({"email": username, "password": password, "turnstile_token": ""})),
        "Sub2API 登录",
    )
    .await?;
    if data
        .get("requires_2fa")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("该 Sub2API 账号启用了 2FA，请先在站点完成登录后粘贴访问令牌".to_string());
    }
    let mut authenticated = provider.clone();
    apply_token_response(&mut authenticated, &data, "Sub2API 登录")?;
    Ok(authenticated)
}

const REFRESH_SKEW_SECS: i64 = 120;

/// 用 refresh_token 滚动获取新的 access_token（服务端同时轮换出新的 refresh_token）。
/// 仅供持久化的刷新路径调用：读操作里旋转令牌却不落盘会导致下次提交旧令牌，触发
/// 服务端「重用攻击」并吊销整个会话家族。
pub(super) async fn refresh_tokens(
    client: &Client,
    provider: &Provider,
) -> Result<Provider, String> {
    let refresh_token = provider.auth.refresh_token.trim();
    if refresh_token.is_empty() {
        return Err("缺少 Sub2API 刷新令牌".to_string());
    }
    let data = request_json(
        client,
        Method::POST,
        api_url(&provider.identity.base_url, "/auth/refresh")?,
        None,
        Some(json!({ "refresh_token": refresh_token })),
        "刷新 Sub2API 令牌",
    )
    .await?;
    let mut refreshed = provider.clone();
    apply_token_response(&mut refreshed, &data, "刷新 Sub2API 令牌")?;
    Ok(refreshed)
}

/// 把登录/刷新返回的 access_token、refresh_token、有效期写入 provider。
fn apply_token_response(
    provider: &mut Provider,
    data: &Value,
    context: &str,
) -> Result<(), String> {
    let token = string_field(data, &["access_token", "accessToken"])
        .ok_or_else(|| format!("{context}成功但没有返回访问令牌"))?;
    provider.auth.access_token = token;
    provider.auth.refresh_token =
        string_field(data, &["refresh_token", "refreshToken"]).unwrap_or_default();
    let expires_in = integer_field(data, &["expires_in", "expiresIn"]);
    provider.auth.access_token_expires_at = if expires_in > 0 {
        Some(crate::util::unix_secs() as i64 + expires_in)
    } else {
        None
    };
    Ok(())
}

/// 是否需要在刷新路径里滚动令牌：持有 refresh_token，且令牌即将过期或已为空。
pub(super) fn needs_token_refresh(provider: &Provider) -> bool {
    if provider.auth.refresh_token.trim().is_empty() {
        return false;
    }
    match provider.auth.access_token_expires_at {
        Some(expires_at) => (crate::util::unix_secs() as i64) >= expires_at - REFRESH_SKEW_SECS,
        None => provider.auth.access_token.trim().is_empty(),
    }
}

/// refresh 失败是否属于「刷新链已断」（应清空令牌回退登录），以区别于瞬时网络错误
/// （瞬时错误须保留 refresh_token，避免把仍然有效的凭据误清）。
pub(super) fn is_refresh_chain_broken(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("refresh_token")
        || normalized.contains("refresh token")
        || normalized.contains("revoked")
        || normalized.contains("reused")
        || normalized.contains("token expired")
        || normalized.contains("expired token")
        || normalized.contains("http 401")
        || normalized.contains("http 403")
        || normalized.contains("unauthorized")
}

fn has_login_credentials(provider: &Provider) -> bool {
    !provider.auth.login_username.trim().is_empty()
        && !provider.auth.login_password.trim().is_empty()
}

pub(super) fn is_auth_failure(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized.starts_with("http 401")
        || normalized.starts_with("http 403")
        || normalized.contains("unauthorized")
        || normalized.contains("token expired")
        || normalized.contains("jwt expired")
        || normalized.contains("invalid token")
}

#[cfg(test)]
mod tests {
    use super::{is_auth_failure, is_refresh_chain_broken};

    #[test]
    fn recognizes_retryable_auth_failures() {
        assert!(is_auth_failure("HTTP 401: token expired"));
        assert!(is_auth_failure("HTTP 403: forbidden"));
        assert!(is_auth_failure("JWT expired"));
        assert!(!is_auth_failure("HTTP 500: unavailable"));
    }

    #[test]
    fn transient_certificate_errors_do_not_clear_the_refresh_chain() {
        assert!(!is_refresh_chain_broken(
            "request failed because the TLS certificate has expired"
        ));
        assert!(is_refresh_chain_broken("HTTP 401: refresh token expired"));
    }
}
