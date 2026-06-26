use crate::models::{AuthMode, Provider, ProviderQuotaDisplay, ProviderQuotaScope, ProviderStatus};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, USER_AGENT},
    Client,
};
use serde::Deserialize;
use serde_json::Value;

use super::newapi_http::{
    access_token_fallback_provider, apply_auth_headers, build_url, merge_cookie_headers,
    normalize_base_url, provider_cookie_header, provider_is_anyrouter,
    should_retry_with_access_token, USER_AGENT_VALUE,
};
use super::newapi_response::{
    cloudflare_challenge_message, extract_bool_field, extract_i64_field, extract_string_field,
    is_cloudflare_challenge, parse_success_data, trim_message,
};
use super::newapi_site::{
    apply_site_metadata, convert_quota_value, fetch_site_metadata, site_metadata_from_provider,
    value_to_string, SiteMetadata,
};

#[path = "newapi_quota/connection.rs"]
mod connection;

pub use connection::test_connection;

#[derive(Debug, Deserialize)]
struct NewApiResponse {
    success: bool,
    message: Option<String>,
    data: Option<UserData>,
}

#[derive(Debug, Deserialize)]
struct UserData {
    id: Option<Value>,
    username: Option<String>,
    #[serde(rename = "display_name")]
    display_name: Option<String>,
    quota: i64,
    #[serde(rename = "used_quota")]
    used_quota: i64,
}

struct QuotaProfile {
    available: f64,
    used: f64,
    quota_scope: ProviderQuotaScope,
    quota_unlimited: bool,
    quota_display: ProviderQuotaDisplay,
    site: SiteMetadata,
    display_name: String,
    username: String,
    user_id: String,
}

pub async fn refresh_provider(client: &Client, provider: &Provider) -> Provider {
    let mut next = provider.clone();

    if !provider.runtime.enabled {
        return next;
    }

    match fetch_quota_with_access_token_fallback(client, provider).await {
        Ok(profile) => {
            next.quota.available = profile.available;
            next.quota.used = profile.used;
            next.quota.scope = profile.quota_scope;
            next.quota.unlimited = profile.quota_unlimited;
            next.quota.display_type = profile.quota_display.quota_display_type;
            next.quota.currency_symbol = profile.quota_display.currency_symbol;
            next.identity.display_name = profile.display_name;
            next.identity.username = profile.username;
            next.identity.user_id = profile.user_id;
            apply_site_metadata(&mut next, profile.site);
            next.runtime.status = if !next.quota.unlimited && next.quota.available <= 20.0 {
                ProviderStatus::Warning
            } else {
                ProviderStatus::Ok
            };
            next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
            next.runtime.error_message = None;
        }
        Err(message) => {
            next.quota.scope = ProviderQuotaScope::Account;
            next.quota.unlimited = false;
            next.runtime.status = ProviderStatus::Error;
            next.runtime.error_message = Some(message);
        }
    }

    next
}

async fn fetch_quota_with_access_token_fallback(
    client: &Client,
    provider: &Provider,
) -> Result<QuotaProfile, String> {
    match fetch_quota(client, provider).await {
        Ok(profile) => Ok(profile),
        Err(message) => {
            if should_retry_with_access_token(&message) {
                if let Some(fallback_provider) = access_token_fallback_provider(provider) {
                    return fetch_quota(client, &fallback_provider).await.map_err(
                        |fallback_message| {
                            format!("{message}；已尝试改用访问令牌，仍失败: {fallback_message}")
                        },
                    );
                }
            }
            Err(message)
        }
    }
}

async fn fetch_quota(client: &Client, provider: &Provider) -> Result<QuotaProfile, String> {
    let provider = effective_quota_provider(provider);
    validate_credentials(&provider)?;

    let base_url = normalize_base_url(&provider.identity.base_url);
    let is_anyrouter = provider_is_anyrouter(&provider);
    let site = fetch_site_metadata(client, &base_url, is_anyrouter)
        .await
        .unwrap_or_else(|_| site_metadata_from_provider(&provider));
    if matches!(provider.auth.mode, AuthMode::ApiKey) {
        return fetch_token_quota(client, &provider, &base_url, site).await;
    }
    let url = build_url(&base_url, "/api/user/self")?;

    let mut cookie_header = String::new();
    if is_anyrouter {
        cookie_header = super::anyrouter::challenge_cookie_header(client, &base_url).await?;
    }

    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, &base_url)
        .header(REFERER, format!("{base_url}/"));

    request = apply_auth_headers(request, &provider);

    let provider_cookie_header =
        provider_cookie_header(&provider.auth.session_cookie, is_anyrouter);
    if matches!(provider.auth.mode, AuthMode::Session) {
        cookie_header =
            merge_cookie_headers(&[cookie_header.as_str(), provider_cookie_header.as_str()]);
    }

    if !cookie_header.trim().is_empty() {
        request = request.header(COOKIE, cookie_header);
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("请求余额失败: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("读取余额响应失败: {err}"))?;

    if !status.is_success() {
        if is_cloudflare_challenge(&body) {
            return Err(cloudflare_challenge_message());
        }
        return Err(format!("HTTP {}: {}", status.as_u16(), trim_message(&body)));
    }

    if is_cloudflare_challenge(&body) {
        return Err(cloudflare_challenge_message());
    }

    if body.contains("var arg1") {
        return Err("命中 AnyRouter 验证页，动态 Cookie 未通过".to_string());
    }

    let decoded = serde_json::from_str::<NewApiResponse>(&body)
        .map_err(|err| format!("解析余额响应失败: {err}: {}", trim_message(&body)))?;
    if !decoded.success {
        return Err(decoded
            .message
            .unwrap_or_else(|| "接口返回失败".to_string()));
    }

    let data = decoded
        .data
        .ok_or_else(|| "接口缺少 data 字段".to_string())?;
    let (available, quota_display) = convert_quota_value(data.quota, &site);
    let (used, _) = convert_quota_value(data.used_quota, &site);
    Ok(QuotaProfile {
        available,
        used,
        quota_scope: ProviderQuotaScope::Account,
        quota_unlimited: false,
        quota_display,
        site,
        display_name: data.display_name.unwrap_or_default(),
        username: data.username.unwrap_or_default(),
        user_id: value_to_string(data.id),
    })
}

async fn fetch_token_quota(
    client: &Client,
    provider: &Provider,
    base_url: &str,
    site: SiteMetadata,
) -> Result<QuotaProfile, String> {
    let url = build_url(base_url, "/api/usage/token/")?;
    let response = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json, text/plain, */*")
        .bearer_auth(provider.auth.api_key.trim())
        .send()
        .await
        .map_err(|err| format!("请求 API 密钥额度失败: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("读取 API 密钥额度响应失败: {err}"))?;
    let data = parse_success_data(&status, body, "API 密钥额度")?;
    let quota_unlimited =
        extract_bool_field(&data, &["unlimited_quota", "unlimitedQuota"]).unwrap_or(false);
    let available_raw = extract_i64_field(&data, &["total_available", "totalAvailable"])
        .unwrap_or(0)
        .max(0);
    let used_raw = extract_i64_field(&data, &["total_used", "totalUsed"]).unwrap_or(0);
    let (available, quota_display) = if quota_unlimited {
        let (_, quota_display) = convert_quota_value(0, &site);
        (0.0, quota_display)
    } else {
        convert_quota_value(available_raw, &site)
    };
    let (used, _) = convert_quota_value(used_raw, &site);

    Ok(QuotaProfile {
        available,
        used,
        quota_scope: ProviderQuotaScope::Token,
        quota_unlimited,
        quota_display,
        site,
        display_name: String::new(),
        username: extract_string_field(&data, &["name", "Name"]).unwrap_or_default(),
        user_id: String::new(),
    })
}

fn effective_quota_provider(provider: &Provider) -> Provider {
    if !matches!(provider.auth.mode, AuthMode::ApiKey) {
        return provider.clone();
    }

    let api_user = provider.auth.api_user.trim();
    if api_user.is_empty() {
        return provider.clone();
    }

    let mut next = provider.clone();
    if provider_is_anyrouter(provider) && !provider.auth.session_cookie.trim().is_empty() {
        next.auth.mode = AuthMode::Session;
        return next;
    }

    if !provider.auth.session_cookie.trim().is_empty() {
        next.auth.mode = AuthMode::Session;
        return next;
    }

    if !provider.auth.access_token.trim().is_empty() {
        next.auth.mode = AuthMode::AccessToken;
        return next;
    }

    provider.clone()
}

fn validate_credentials(provider: &Provider) -> Result<(), String> {
    match provider.auth.mode {
        AuthMode::ApiKey if provider.auth.api_key.trim().is_empty() => {
            Err("缺少 API 密钥".to_string())
        }
        AuthMode::AccessToken
            if provider.auth.access_token.trim().is_empty()
                || provider.auth.api_user.trim().is_empty() =>
        {
            Err("缺少访问令牌或 API User ID".to_string())
        }
        AuthMode::Session
            if provider.auth.session_cookie.trim().is_empty()
                || provider.auth.api_user.trim().is_empty() =>
        {
            Err("缺少会话 Cookie 或 API User ID".to_string())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;

    fn provider_with_credentials(
        auth_mode: AuthMode,
        api_key: &str,
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
                    mode: auth_mode,
                    api_key: api_key.to_string(),
                    access_token: access_token.to_string(),
                    session_cookie: session_cookie.to_string(),
                    api_user: api_user.to_string(),
                },
                ..ProviderInput::default()
            },
            "provider-test".to_string(),
        )
    }

    #[test]
    fn api_key_quota_refresh_prefers_cookie_over_access_token() {
        let provider = provider_with_credentials(
            AuthMode::ApiKey,
            "sk-test",
            "access-token",
            "session=session-cookie",
            "1001",
        );

        let effective = effective_quota_provider(&provider);

        assert!(matches!(effective.auth.mode, AuthMode::Session));
    }

    #[test]
    fn api_key_quota_refresh_uses_access_token_when_cookie_missing() {
        let provider =
            provider_with_credentials(AuthMode::ApiKey, "sk-test", "access-token", "", "1001");

        let effective = effective_quota_provider(&provider);

        assert!(matches!(effective.auth.mode, AuthMode::AccessToken));
    }

    #[test]
    fn api_key_connection_test_isolates_user_credentials() {
        let mut provider = provider_with_credentials(
            AuthMode::ApiKey,
            "sk-test",
            "access-token",
            "session=session-cookie",
            "1001",
        );

        connection::isolate_test_credentials(&mut provider, AuthMode::ApiKey);

        assert_eq!(provider.auth.api_key, "sk-test");
        assert!(provider.auth.access_token.is_empty());
        assert!(provider.auth.session_cookie.is_empty());
        assert!(provider.auth.api_user.is_empty());
        assert!(matches!(
            effective_quota_provider(&provider).auth.mode,
            AuthMode::ApiKey
        ));
    }
}
