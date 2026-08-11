use crate::{
    adapters::transport::ProviderTransport,
    models::{AuthMode, Provider},
};
use reqwest::Method;
use serde_json::{json, Value};

use super::{
    json::{integer_field, string_field},
    response::{api_url, request_json, Credential},
};

pub(super) async fn authenticate_account(
    client: &ProviderTransport,
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
    client: &ProviderTransport,
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
        Err(first_error) if is_auth_failure(&first_error) => {
            // Sub2API refresh tokens are rotating credentials. Retry with a
            // newly issued access token before falling back to a password login.
            if !authenticated.auth.refresh_token.trim().is_empty() {
                match refresh_tokens(client, &authenticated).await {
                    Ok(refreshed) => {
                        let retry = request_json(
                            client,
                            method.clone(),
                            api_url(&refreshed.identity.base_url, path)?,
                            Some(Credential::Jwt(refreshed.auth.access_token.clone())),
                            body.clone(),
                            context,
                        )
                        .await;
                        match retry {
                            Ok(value) => return Ok((refreshed, value)),
                            Err(retry_error)
                                if is_auth_failure(&retry_error)
                                    && has_login_credentials(provider) =>
                            {
                                let relogged = login(client, provider).await.map_err(|login_error| {
                                    format!(
                                        "{first_error}；刷新令牌后重试失败: {retry_error}；重新登录失败: {login_error}"
                                    )
                                })?;
                                let value = request_json(
                                    client,
                                    method,
                                    api_url(&relogged.identity.base_url, path)?,
                                    Some(Credential::Jwt(relogged.auth.access_token.clone())),
                                    body,
                                    context,
                                )
                                .await
                                .map_err(|final_error| {
                                    format!(
                                        "{first_error}；刷新令牌后重试失败: {retry_error}；重新登录后重试失败: {final_error}"
                                    )
                                })?;
                                return Ok((relogged, value));
                            }
                            Err(retry_error) => {
                                return Err(format!(
                                    "{first_error}；刷新令牌后重试失败: {retry_error}"
                                ));
                            }
                        }
                    }
                    Err(refresh_error) if is_refresh_chain_broken(&refresh_error) => {
                        if !has_login_credentials(provider) {
                            return Err(format!(
                                "{first_error}；刷新 Sub2API 令牌失败: {refresh_error}"
                            ));
                        }
                    }
                    Err(refresh_error) => {
                        return Err(format!(
                            "{first_error}；刷新 Sub2API 令牌失败: {refresh_error}"
                        ));
                    }
                }
            }

            if has_login_credentials(provider) {
                let refreshed = login(client, provider)
                    .await
                    .map_err(|login_error| format!("{first_error}；重新登录失败: {login_error}"))?;
                let value = request_json(
                    client,
                    method,
                    api_url(&refreshed.identity.base_url, path)?,
                    Some(Credential::Jwt(refreshed.auth.access_token.clone())),
                    body,
                    context,
                )
                .await
                .map_err(|retry_error| {
                    format!("{first_error}；重新登录后重试失败: {retry_error}")
                })?;
                return Ok((refreshed, value));
            }

            Err(first_error)
        }
        Err(error) => Err(error),
    }
}

async fn authenticate_account_if_needed(
    client: &ProviderTransport,
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
    if !provider.auth.refresh_token.trim().is_empty() {
        match refresh_tokens(client, provider).await {
            Ok(refreshed) => return Ok(refreshed),
            Err(error) if is_refresh_chain_broken(&error) && has_login_credentials(provider) => {
                return login(client, provider).await;
            }
            Err(error) => return Err(error),
        }
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

async fn login(client: &ProviderTransport, provider: &Provider) -> Result<Provider, String> {
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
/// 调用方必须把返回的 Provider 持久化：旋转令牌却不落盘会导致下次提交旧令牌，
/// 触发服务端「重用攻击」并吊销整个会话家族。
pub(super) async fn refresh_tokens(
    client: &ProviderTransport,
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
    // Some Sub2API deployments only return a new access token while keeping
    // the refresh token stable. Never erase a working rotating credential just
    // because that optional field is absent from the response.
    if let Some(refresh_token) = string_field(data, &["refresh_token", "refreshToken"])
        .filter(|value| !value.trim().is_empty())
    {
        provider.auth.refresh_token = refresh_token;
    }
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
    use super::{apply_token_response, is_auth_failure, is_refresh_chain_broken};
    use crate::models::{Provider, ProviderInput};
    use serde_json::json;

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

    #[test]
    fn auth_failure_detection_includes_plain_http_401_and_403() {
        assert!(is_auth_failure("HTTP 401: unauthorized"));
        assert!(is_auth_failure("HTTP 403: forbidden"));
    }

    #[test]
    fn token_response_without_refresh_token_preserves_previous_value() {
        let mut provider = Provider::from_input(ProviderInput::default(), "sub2-test".to_string());
        provider.auth.refresh_token = "refresh-old".to_string();

        apply_token_response(
            &mut provider,
            &json!({
                "access_token": "access-new",
                "expires_in": 3600
            }),
            "测试",
        )
        .expect("access token should parse");

        assert_eq!(provider.auth.access_token, "access-new");
        assert_eq!(provider.auth.refresh_token, "refresh-old");
        assert!(provider.auth.access_token_expires_at.is_some());
    }

    #[test]
    fn token_response_rotates_refresh_token_when_present() {
        let mut provider = Provider::from_input(ProviderInput::default(), "sub2-test".to_string());
        provider.auth.refresh_token = "refresh-old".to_string();

        apply_token_response(
            &mut provider,
            &json!({
                "access_token": "access-new",
                "refresh_token": "refresh-new"
            }),
            "测试",
        )
        .expect("access token should parse");

        assert_eq!(provider.auth.refresh_token, "refresh-new");
    }
}
