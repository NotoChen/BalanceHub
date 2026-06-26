use crate::{
    models::{AppSettings, AuthMode, Provider, ProxyMode},
    network,
};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, USER_AGENT},
    Client, Method, Proxy, Url,
};
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
    let api_user = provider.auth.api_user.trim().to_string();
    if api_user.is_empty() {
        return Err("缺少 API User ID，无法管理 API 密钥".to_string());
    }

    let credential = if !provider.auth.session_cookie.trim().is_empty() {
        UserCredential::Session(provider.auth.session_cookie.clone())
    } else if !provider.auth.access_token.trim().is_empty() {
        UserCredential::AccessToken(provider.auth.access_token.clone())
    } else {
        return Err("缺少访问令牌或会话 Cookie，无法管理 API 密钥".to_string());
    };

    let is_anyrouter = provider_is_anyrouter(provider);
    Ok((base_url, api_user, credential, is_anyrouter))
}

pub(crate) fn access_token_fallback_provider(provider: &Provider) -> Option<Provider> {
    if matches!(provider.auth.mode, AuthMode::AccessToken) {
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
    message.contains("Cloudflare")
        || message.contains("HTTP 403")
        || message.contains("API 密钥不支持用户签到")
        || message.contains("Cookie 签到需要")
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
    }
}

pub(crate) fn apply_session_cookie(
    request: reqwest::RequestBuilder,
    provider: &Provider,
) -> reqwest::RequestBuilder {
    if !matches!(provider.auth.mode, AuthMode::Session) {
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
                    provider_kind: Default::default(),
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
    fn user_management_prefers_cookie_over_access_token() {
        let provider =
            provider_with_user_credentials("access-token", "session=session-cookie", "1001");

        let (_, _, credential, _) = provider_user_management_context(&provider).unwrap();

        assert!(matches!(credential, UserCredential::Session(_)));
    }

    #[test]
    fn user_management_uses_access_token_when_cookie_missing() {
        let provider = provider_with_user_credentials("access-token", "", "1001");

        let (_, _, credential, _) = provider_user_management_context(&provider).unwrap();

        assert!(matches!(credential, UserCredential::AccessToken(_)));
    }
}
