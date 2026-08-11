use crate::models::{AuthMode, Provider};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, SET_COOKIE, USER_AGENT},
    Method, Url,
};
use serde_json::{json, Value};
use std::future::Future;

pub(crate) use crate::adapters::transport::{
    build_client, merge_cookie_headers, ProviderTransport, USER_AGENT_VALUE,
};

#[derive(Clone)]
pub(crate) enum UserCredential {
    AccessToken(String),
    Session(String),
}

/// 将 NewAPI 的账号密码登录转换为现有的会话认证上下文。
///
/// 登录凭据保留在 Provider 中，Session 只作为可复用缓存写回；后续请求仍走
/// Cookie + `new-api-user`，不会把账号密码发送到任何其他接口。
pub(crate) async fn authenticate_password_provider(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<Provider, String> {
    authenticate_password_provider_inner(client, provider, false).await
}

/// 强制使用当前账号密码重新登录。交互式操作需要这个路径，避免用户修改密码后
/// 仍然复用旧的缓存 Session。
pub(crate) async fn login_password_provider(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<Provider, String> {
    authenticate_password_provider_inner(client, provider, true).await
}

pub(crate) fn extract_login_cookie(
    headers: &reqwest::header::HeaderMap,
    base_url: &str,
) -> Option<String> {
    let pairs = headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .filter_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty() && !value.is_empty()).then(|| (name.to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();
    if is_anyrouter_base_url(base_url) {
        return pairs
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("session"))
            .map(|(name, value)| format!("{name}={value}"));
    }
    if !pairs
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("session"))
    {
        return None;
    }
    Some(
        pairs
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

async fn authenticate_password_provider_inner(
    client: &ProviderTransport,
    provider: &Provider,
    force_login: bool,
) -> Result<Provider, String> {
    if !matches!(provider.auth.mode, AuthMode::Password) {
        return Ok(provider.clone());
    }

    if !force_login
        && !provider.auth.session_cookie.trim().is_empty()
        && !provider.auth.api_user.trim().is_empty()
    {
        let mut cached = provider.clone();
        cached.auth.mode = AuthMode::Session;
        return Ok(cached);
    }

    let username = provider.auth.login_username.trim();
    let password = provider.auth.login_password.as_str();
    if username.is_empty() || password.trim().is_empty() {
        return Err("账号密码模式需要填写用户名和密码".to_string());
    }

    let base_url = normalize_base_url(&provider.identity.base_url);
    if base_url.is_empty() {
        return Err("缺少中转站地址".to_string());
    }
    let mut url = build_url(&base_url, "/api/user/login")?;
    url.query_pairs_mut().append_pair("turnstile", "");

    let request = client
        .post(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, &base_url)
        .header(REFERER, format!("{base_url}/"))
        .json(&json!({ "username": username, "password": password }));

    let response = client.send(request, "账号密码登录").await?;
    let status = response.status;
    let session_cookie = extract_login_cookie(&response.headers, &base_url);
    let body = response.body;
    let payload = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
    let success = payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();

    if !status.is_success() || !success {
        let detail = if message.is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            message.to_string()
        };
        return Err(format!("账号密码登录失败: {detail}"));
    }

    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    if data
        .get("require_2fa")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(
            "该账号启用了 2FA，当前无法在本地自动完成验证码登录，请改用 Cookie".to_string(),
        );
    }

    let session_cookie = session_cookie.ok_or_else(|| {
        "登录成功但站点没有返回 Session Cookie，请改用 Cookie 或检查站点配置".to_string()
    })?;
    let api_user = data
        .get("id")
        .and_then(|value| {
            value
                .as_i64()
                .map(|id| id.to_string())
                .or_else(|| value.as_u64().map(|id| id.to_string()))
                .or_else(|| value.as_str().map(str::to_string))
        })
        .filter(|id| !id.trim().is_empty())
        .or_else(|| {
            (!provider.auth.api_user.trim().is_empty()).then(|| provider.auth.api_user.clone())
        })
        .ok_or_else(|| "登录成功但响应中没有用户 ID".to_string())?;

    let mut authenticated = provider.clone();
    authenticated.auth.mode = AuthMode::Session;
    authenticated.auth.session_cookie = session_cookie;
    authenticated.auth.api_user = api_user;
    Ok(authenticated)
}

pub(crate) fn build_user_request(
    client: &ProviderTransport,
    method: Method,
    url: Url,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
) -> reqwest::RequestBuilder {
    // 过盾由 ProviderTransport 在响应阶段统一处理，这里只组装业务认证。
    let mut cookie_header = String::new();

    let mut request = client
        .request(method, url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, base_url)
        .header(REFERER, format!("{base_url}/"))
        .header("new-api-user", api_user.trim());

    match credential {
        UserCredential::AccessToken(access_token) => {
            request = request.bearer_auth(access_token.trim());
        }
        UserCredential::Session(session_cookie) => {
            let session_cookie = provider_cookie_header_for_base(&session_cookie, base_url);
            cookie_header =
                merge_cookie_headers(&[cookie_header.as_str(), session_cookie.as_str()]);
        }
    }

    if !cookie_header.trim().is_empty() {
        request = request.header(COOKIE, cookie_header);
    }

    request
}

pub(crate) fn provider_user_management_context(
    provider: &Provider,
) -> Result<(String, String, UserCredential), String> {
    let base_url = normalize_base_url(&provider.identity.base_url);
    if base_url.is_empty() {
        return Err("缺少中转站地址".to_string());
    }
    if matches!(provider.auth.mode, AuthMode::ApiKey) {
        return Err("API Key 认证不支持账号管理，请切换到 Cookie 或访问令牌".to_string());
    }
    let api_user = provider.auth.api_user.trim().to_string();
    if api_user.is_empty() {
        return Err("缺少 API User ID，无法管理 API 密钥".to_string());
    }

    let credential = user_management_credential(provider)?;

    Ok((base_url, api_user, credential))
}

fn user_management_credential(provider: &Provider) -> Result<UserCredential, String> {
    let session = provider.auth.session_cookie.trim();
    let access_token = provider.auth.access_token.trim();

    match provider.auth.mode {
        AuthMode::Session if !session.is_empty() => Ok(UserCredential::Session(
            provider.auth.session_cookie.clone(),
        )),
        AuthMode::AccessToken if !access_token.is_empty() => Ok(UserCredential::AccessToken(
            provider.auth.access_token.clone(),
        )),
        AuthMode::ApiKey => Err("API Key 认证不支持账号管理".to_string()),
        AuthMode::Password if !session.is_empty() => Ok(UserCredential::Session(
            provider.auth.session_cookie.clone(),
        )),
        _ => fallback_user_management_credential(provider),
    }
}

fn fallback_user_management_credential(provider: &Provider) -> Result<UserCredential, String> {
    if !provider.auth.session_cookie.trim().is_empty() {
        return Ok(UserCredential::Session(
            provider.auth.session_cookie.clone(),
        ));
    }
    if !provider.auth.access_token.trim().is_empty() {
        return Ok(UserCredential::AccessToken(
            provider.auth.access_token.clone(),
        ));
    }
    Err("缺少访问令牌或会话 Cookie，无法管理 API 密钥".to_string())
}

pub(crate) fn access_token_fallback_provider(provider: &Provider) -> Option<Provider> {
    if matches!(provider.auth.mode, AuthMode::ApiKey | AuthMode::AccessToken) {
        return None;
    }

    if is_anyrouter_base_url(&normalize_base_url(&provider.identity.base_url)) {
        return None;
    }

    if provider.auth.access_token.trim().is_empty() || provider.auth.api_user.trim().is_empty() {
        return None;
    }

    let mut fallback = provider.clone();
    fallback.auth.mode = AuthMode::AccessToken;
    Some(fallback)
}

pub(crate) fn should_retry_with_access_token(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("http 401")
        || normalized.contains("http 403")
        || message.contains("API 密钥不支持用户签到")
        || message.contains("Cookie 签到需要")
        || message.contains("未登录")
        || normalized.contains("unauthorized")
        || normalized.contains("not logged in")
        || normalized.contains("no access token")
}

/// Execute an authenticated account operation and retry it once with the
/// stored access token when a Session/Cookie credential is rejected. The
/// shield breaker is checked by the caller's transport so a WAF challenge is
/// never mistaken for a credential failure.
pub(crate) async fn retry_with_access_token<T, Initial, Fallback, Fut>(
    client: &ProviderTransport,
    provider: &Provider,
    initial: Initial,
    fallback: Fallback,
) -> Result<(Provider, T), String>
where
    T: Send,
    Initial: Future<Output = Result<T, String>>,
    Fallback: FnOnce(Provider) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    match initial.await {
        Ok(value) => Ok((provider.clone(), value)),
        Err(message)
            if client
                .shield_blocked_for(&provider.identity.base_url)
                .await
                .is_none()
                && should_retry_with_access_token(&message) =>
        {
            let Some(fallback_provider) = access_token_fallback_provider(provider) else {
                return Err(message);
            };
            let value = fallback(fallback_provider.clone())
                .await
                .map_err(|fallback_error| {
                    format!("{message}；已尝试改用访问令牌，仍失败: {fallback_error}")
                })?;
            Ok((fallback_provider, value))
        }
        Err(message) => Err(message),
    }
}

pub(crate) fn apply_auth_headers(
    request: reqwest::RequestBuilder,
    provider: &Provider,
) -> reqwest::RequestBuilder {
    match provider.auth.mode {
        AuthMode::ApiKey => request.bearer_auth(provider.auth.api_key.trim()),
        AuthMode::AccessToken => request
            .bearer_auth(provider.auth.access_token.trim())
            .header("new-api-user", provider.auth.api_user.trim()),
        AuthMode::Session => request.header("new-api-user", provider.auth.api_user.trim()),
        AuthMode::Password => request.header("new-api-user", provider.auth.api_user.trim()),
    }
}

pub(crate) fn apply_session_cookie(
    request: reqwest::RequestBuilder,
    provider: &Provider,
) -> reqwest::RequestBuilder {
    if !matches!(provider.auth.mode, AuthMode::Session | AuthMode::Password) {
        return request;
    }

    request.header(
        COOKIE,
        provider_cookie_header_for_base(&provider.auth.session_cookie, &provider.identity.base_url),
    )
}

fn provider_cookie_header_for_base(raw: &str, base_url: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if is_anyrouter_base_url(base_url) {
        let session = super::anyrouter::normalize_session_cookie(trimmed);
        return if session.is_empty() {
            String::new()
        } else {
            format!("session={session}")
        };
    }

    // Cookie input may be copied from a browser as `name=value; ...`. Keep all
    // cookie pairs for ordinary NewAPI sites, dropping Set-Cookie attributes
    // such as Path/HttpOnly that are not valid request cookies.
    trimmed
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty()
                || value.is_empty()
                || matches!(
                    name.to_ascii_lowercase().as_str(),
                    "path" | "domain" | "expires" | "max-age" | "secure" | "httponly" | "samesite"
                )
            {
                return None;
            }
            Some(format!("{name}={value}"))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn build_url(base_url: &str, path: &str) -> Result<Url, String> {
    // 直接拼接而非 Url::join：所有调用方传入的 path 都以 "/" 开头，
    // 而 join 对绝对路径会整段替换 base 的 path，导致子路径部署（如 https://host/relay）
    // 的接口地址被错误地截断为 https://host/api/...。拼接可保留前缀。
    let base = base_url.trim_end_matches('/');
    Url::parse(&format!("{base}{path}")).map_err(|err| format!("中转站地址无效: {err}"))
}

pub(crate) fn normalize_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

pub fn is_anyrouter_base_url(base_url: &str) -> bool {
    base_url.to_lowercase().contains("anyrouter")
}

/// 识别 NewAPI 的特殊接口方言。anyrouter 不作为独立站点类型暴露，
/// 当前统一按站点地址启发式识别。
pub fn provider_is_anyrouter(provider: &Provider) -> bool {
    is_anyrouter_base_url(&normalize_base_url(&provider.identity.base_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;

    fn provider_with_user_credentials(
        access_token: &str,
        session_cookie: &str,
        api_user: &str,
    ) -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: crate::models::ProviderIdentityInput {
                    name: "Relay".to_string(),
                    base_url: "https://relay.example.com".to_string(),
                    ..crate::models::ProviderIdentityInput::default()
                },
                auth: crate::models::ProviderAuth {
                    mode: AuthMode::Session,
                    access_token: access_token.to_string(),
                    session_cookie: session_cookie.to_string(),
                    api_user: api_user.to_string(),
                    ..ProviderInput::default().auth
                },
                ..ProviderInput::default()
            },
            "provider-test".to_string(),
        )
    }

    #[test]
    fn user_management_respects_access_token_mode_even_when_cookie_exists() {
        let mut provider =
            provider_with_user_credentials("access-token", "session=session-cookie", "1001");
        provider.auth.mode = AuthMode::AccessToken;

        let (_, _, credential) = provider_user_management_context(&provider).unwrap();

        assert!(matches!(credential, UserCredential::AccessToken(_)));
    }

    #[test]
    fn user_management_rejects_api_key_mode_even_when_account_credentials_are_cached() {
        let mut provider =
            provider_with_user_credentials("access-token", "session=session-cookie", "1001");
        provider.auth.mode = AuthMode::ApiKey;

        let error = provider_user_management_context(&provider)
            .err()
            .expect("API Key mode must reject account management");
        assert!(error.contains("API Key"));
    }

    #[test]
    fn user_management_uses_access_token_when_cookie_missing() {
        let provider = provider_with_user_credentials("access-token", "", "1001");

        let (_, _, credential) = provider_user_management_context(&provider).unwrap();

        assert!(matches!(credential, UserCredential::AccessToken(_)));
    }

    #[test]
    fn api_key_mode_never_falls_back_to_cached_access_token() {
        let mut provider = provider_with_user_credentials("access-token", "", "1001");
        provider.auth.mode = AuthMode::ApiKey;

        assert!(access_token_fallback_provider(&provider).is_none());
    }

    #[test]
    fn shield_failures_never_enter_the_access_token_fallback() {
        assert!(!should_retry_with_access_token(
            "命中阿里云 WAF 验证，自动过盾后仍未通过。"
        ));
        assert!(!should_retry_with_access_token(
            "命中阿里云 WAF 验证，自动过盾后仍未通过。；已尝试改用访问令牌，仍失败"
        ));
        assert!(should_retry_with_access_token("HTTP 403: 未登录"));
        assert!(should_retry_with_access_token("HTTP 401: session expired"));
    }

    #[test]
    fn ordinary_newapi_cookie_keeps_all_cookie_pairs() {
        assert_eq!(
            provider_cookie_header_for_base("session=abc; csrf=def; Path=/; HttpOnly", ""),
            "session=abc; csrf=def"
        );
    }

    #[test]
    fn anyrouter_cookie_is_reduced_to_session_only() {
        assert_eq!(
            provider_cookie_header_for_base(
                "session=abc; csrf=def; Path=/",
                "https://anyrouter.top"
            ),
            "session=abc"
        );
    }

    #[test]
    fn login_cookie_extraction_keeps_non_session_cookies_for_newapi() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(SET_COOKIE, "session=abc; Path=/".parse().expect("header"));
        headers.append(SET_COOKIE, "csrf=def; Path=/".parse().expect("header"));

        let cookie = extract_login_cookie(&headers, "https://relay.example.com")
            .expect("session cookie should be present");
        assert_eq!(cookie, "session=abc; csrf=def");
    }
}
