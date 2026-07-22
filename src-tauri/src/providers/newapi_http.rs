use crate::{
    models::{AppSettings, AuthMode, Provider, ProxyMode},
    network,
};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, SET_COOKIE, USER_AGENT},
    Client, Method, Proxy, Url,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub(crate) const USER_AGENT_VALUE: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/131.0.0.0 Safari/537.36"
);

#[derive(Clone)]
pub(crate) enum UserCredential {
    AccessToken(String),
    Session(String),
}

pub fn build_client(settings: &AppSettings, provider: &Provider) -> Result<Client, String> {
    let proxy = network::resolve_proxy(settings, provider);
    build_client_for_proxy(proxy.mode, &proxy.url)
}

// 复用 reqwest::Client：内部维护连接池，按代理配置缓存并复用，
// 避免每次请求都重建客户端（重做 TLS 配置 + 丢弃连接池，刷新多个中转站时尤其浪费）。
fn client_cache() -> &'static Mutex<HashMap<String, Client>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn build_client_for_proxy(proxy_mode: ProxyMode, proxy_url: &str) -> Result<Client, String> {
    let cache_key = format!("{proxy_mode:?}|{}", proxy_url.trim());
    if let Some(client) = client_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        return Ok(client);
    }

    let client = build_client_uncached(proxy_mode, proxy_url)?;
    if let Ok(mut cache) = client_cache().lock() {
        cache.insert(cache_key, client.clone());
    }
    Ok(client)
}

/// 将 NewAPI 的账号密码登录转换为现有的会话认证上下文。
///
/// 登录凭据保留在 Provider 中，Session 只作为可复用缓存写回；后续请求仍走
/// Cookie + `new-api-user`，不会把账号密码发送到任何其他接口。
pub(crate) async fn authenticate_password_provider(
    client: &Client,
    provider: &Provider,
) -> Result<Provider, String> {
    authenticate_password_provider_inner(client, provider, false).await
}

/// 强制使用当前账号密码重新登录。交互式操作需要这个路径，避免用户修改密码后
/// 仍然复用旧的缓存 Session。
pub(crate) async fn login_password_provider(
    client: &Client,
    provider: &Provider,
) -> Result<Provider, String> {
    authenticate_password_provider_inner(client, provider, true).await
}

async fn authenticate_password_provider_inner(
    client: &Client,
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

    let mut request = client
        .post(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, &base_url)
        .header(REFERER, format!("{base_url}/"))
        .json(&json!({ "username": username, "password": password }));

    if provider_is_anyrouter(provider) {
        let challenge_cookie = super::anyrouter::challenge_cookie_header(client, &base_url).await?;
        if !challenge_cookie.trim().is_empty() {
            request = request.header(COOKIE, challenge_cookie);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("账号密码登录失败: {err}"))?;
    let status = response.status();
    let session_cookie = extract_session_cookie(response.headers());
    let body = response
        .text()
        .await
        .map_err(|err| format!("读取登录响应失败: {err}"))?;
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

fn extract_session_cookie(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers.get_all(SET_COOKIE).iter().find_map(|value| {
        let text = value.to_str().ok()?;
        text.split(';').find_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            if name.trim().eq_ignore_ascii_case("session") && !value.trim().is_empty() {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
    })
}

fn build_client_uncached(proxy_mode: ProxyMode, proxy_url: &str) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none());

    builder = match proxy_mode {
        ProxyMode::System => builder,
        ProxyMode::NoProxy => builder.no_proxy(),
        ProxyMode::Custom => {
            let proxy_url = proxy_url.trim();
            if proxy_url.is_empty() {
                return Err("自定义代理地址不能为空".to_string());
            }
            let proxy = Proxy::all(proxy_url).map_err(|err| format!("代理地址无效: {err}"))?;
            builder.proxy(proxy)
        }
    };

    builder
        .build()
        .map_err(|err| format!("初始化 HTTP 客户端失败: {err}"))
}

pub(crate) async fn build_user_request(
    client: &Client,
    method: Method,
    url: Url,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
) -> Result<reqwest::RequestBuilder, String> {
    let mut cookie_header = String::new();
    if is_anyrouter {
        cookie_header = super::anyrouter::challenge_cookie_header(client, base_url).await?;
    }

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
            let session_cookie = provider_cookie_header(&session_cookie, is_anyrouter);
            cookie_header =
                merge_cookie_headers(&[cookie_header.as_str(), session_cookie.as_str()]);
        }
    }

    if !cookie_header.trim().is_empty() {
        request = request.header(COOKIE, cookie_header);
    }

    Ok(request)
}

pub(crate) fn provider_user_management_context(
    provider: &Provider,
) -> Result<(String, String, UserCredential, bool), String> {
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

    let is_anyrouter = provider_is_anyrouter(provider);
    Ok((base_url, api_user, credential, is_anyrouter))
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
    message.contains("Cloudflare")
        || message.contains("HTTP 403")
        || message.contains("API 密钥不支持用户签到")
        || message.contains("Cookie 签到需要")
        || message.contains("未登录")
        || normalized.contains("unauthorized")
        || normalized.contains("not logged in")
        || normalized.contains("no access token")
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

    let is_anyrouter = provider_is_anyrouter(provider);
    request.header(
        COOKIE,
        provider_cookie_header(&provider.auth.session_cookie, is_anyrouter),
    )
}

pub(crate) fn provider_cookie_header(raw: &str, session_only: bool) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if session_only || !trimmed.contains('=') {
        let session = super::anyrouter::normalize_session_cookie(trimmed);
        return format!("session={session}");
    }

    trimmed.to_string()
}

pub(crate) fn merge_cookie_headers(cookie_sources: &[&str]) -> String {
    let mut pairs = std::collections::BTreeMap::new();
    for source in cookie_sources {
        for item in source
            .split(';')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let Some((name, value)) = item.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let value = value.trim();
            if !name.is_empty() {
                pairs.insert(name.to_string(), format!("{name}={value}"));
            }
        }
    }
    pairs.into_values().collect::<Vec<_>>().join("; ")
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

        let (_, _, credential, _) = provider_user_management_context(&provider).unwrap();

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

        let (_, _, credential, _) = provider_user_management_context(&provider).unwrap();

        assert!(matches!(credential, UserCredential::AccessToken(_)));
    }

    #[test]
    fn api_key_mode_never_falls_back_to_cached_access_token() {
        let mut provider = provider_with_user_credentials("access-token", "", "1001");
        provider.auth.mode = AuthMode::ApiKey;

        assert!(access_token_fallback_provider(&provider).is_none());
    }
}
