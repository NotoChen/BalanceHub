use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        sub2_api::{
            auth::{
                authenticate_account, is_auth_failure, is_refresh_chain_broken,
                needs_token_refresh, refresh_tokens,
            },
            json::string_field,
            key_usage::fetch_key_quota,
            profile::{apply_user, fetch_site, quota_display},
            response::{api_url, request_json_with_timeout, Credential},
            subscriptions::subscription_windows,
        },
        transport::{build_client, ProviderTransport},
    },
    limits,
    models::{
        AppSettings, AuthMode, Provider, ProviderConnectionTestResult, ProviderConnectionTestStep,
        ProviderQuotaDisplay, ProviderQuotaWindow, ProviderSiteProbeResult, ProviderStatus,
    },
};
use serde_json::Value;
use std::time::Duration;

impl Sub2ApiAdapter {
    pub(crate) async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, ProviderConnectionTestResult), String> {
        let client = build_client(settings, provider).await?;
        let site = fetch_site(&client, &provider.identity.base_url).await.ok();
        let mut steps = Vec::new();

        let refreshed = self
            .refresh_provider_with_client(&client, provider, site)
            .await;
        let value = match refreshed.runtime.status {
            ProviderStatus::Error => ProviderConnectionTestResult {
                ok: false,
                message: refreshed
                    .runtime
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "连接失败".to_string()),
                available: None,
                used: None,
                quota_display: quota_display(&refreshed),
                steps,
            },
            _ => {
                let available = refreshed.quota.known.then_some(refreshed.quota.available);
                let used = refreshed.quota.total_known.then_some(refreshed.quota.used);
                steps.push(ProviderConnectionTestStep {
                    name: "协议连接".to_string(),
                    ok: true,
                    message: if refreshed.quota.known {
                        "Sub2API 账号已连接，余额已读取".to_string()
                    } else {
                        "Sub2API API Key 已连接；该认证方式不公开账号余额".to_string()
                    },
                    available,
                    used,
                    quota_display: quota_display(&refreshed),
                });
                ProviderConnectionTestResult {
                    ok: true,
                    message: steps[0].message.clone(),
                    available,
                    used,
                    quota_display: quota_display(&refreshed),
                    steps,
                }
            }
        };
        Ok((refreshed, value))
    }

    pub(crate) async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        let client = build_client(settings, provider).await?;
        match fetch_site(&client, &provider.identity.base_url).await {
            Ok(site) => Ok(ProviderSiteProbeResult {
                ok: true,
                message: "Sub2API 站点可访问，已读取公开信息".to_string(),
                system_name: string_field(&site, &["site_name", "siteName", "name"]),
                logo: string_field(&site, &["site_logo", "siteLogo", "logo"])
                    .filter(|logo| limits::site_logo_allowed(logo)),
                quota_display: ProviderQuotaDisplay::default(),
            }),
            Err(message) => Ok(ProviderSiteProbeResult {
                ok: false,
                message,
                system_name: None,
                logo: None,
                quota_display: ProviderQuotaDisplay::default(),
            }),
        }
    }

    pub(crate) async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Provider {
        match build_client(settings, provider).await {
            Ok(client) => {
                self.refresh_provider_with_client(&client, provider, None)
                    .await
            }
            Err(message) => provider_with_error(provider, message),
        }
    }

    async fn refresh_provider_with_client(
        &self,
        client: &ProviderTransport,
        provider: &Provider,
        site_hint: Option<Value>,
    ) -> Provider {
        let mut next = provider.clone();
        if !provider.runtime.enabled {
            return next;
        }

        // 刷新路径会轮换 refresh_token。服务层在进入该适配器前持有网络认证闸门，
        // 交互式账号请求也复用同一闸门，避免已轮换的令牌被并发重复提交。
        let mut working = provider.clone();
        let mut token_refresh_succeeded = false;
        if needs_token_refresh(&working) {
            match refresh_tokens(client, &working).await {
                Ok(refreshed) => {
                    working = refreshed;
                    token_refresh_succeeded = true;
                }
                Err(err) if is_refresh_chain_broken(&err) => {
                    // 刷新链已断（过期/吊销/重用）：清空令牌，下面回退账号密码登录（无密码则报错）。
                    working.auth.access_token = String::new();
                    working.auth.refresh_token = String::new();
                    working.auth.access_token_expires_at = None;
                }
                // 瞬时失败：保留 refresh_token，本轮沿用旧令牌，交给后续认证或下一轮重试。
                Err(_) => {}
            }
        }

        let site = match site_hint {
            Some(value) => Some(value),
            None => fetch_site(client, &provider.identity.base_url).await.ok(),
        };
        if let Some(site) = site.as_ref() {
            if let Some(name) = string_field(site, &["site_name", "siteName", "name"]) {
                next.identity.name = name;
            }
            if let Some(logo) = string_field(site, &["site_logo", "siteLogo", "logo"])
                .filter(|logo| limits::site_logo_allowed(logo))
            {
                next.identity.site_logo = logo;
            }
        }

        let authentication =
            authenticate_account_with_refresh(client, &mut working, token_refresh_succeeded).await;
        next.auth.access_token = working.auth.access_token.clone();
        next.auth.refresh_token = working.auth.refresh_token.clone();
        next.auth.access_token_expires_at = working.auth.access_token_expires_at;

        match authentication {
            Ok((authenticated, user)) => {
                apply_rotated_tokens(&mut next, &authenticated);
                apply_user(&mut next, &user);
                // 订阅窗口是 Sub2API 账号真正的额度来源，钱包余额只是溢出账户；
                // 接口不存在（旧版站点）时保持为空，卡片仍显示钱包余额。
                next.quota.windows = fetch_subscription_windows(client, &next).await;
                if !next.auth.api_key.trim().is_empty() {
                    // The independent model state retains failures without losing a valid quota.
                    let _ = refresh_models_if_available(client, &mut next).await;
                }
                // 有窗口上限时由窗口决定状态：订阅账号的钱包只装溢出额度，
                // 余额为 0 并不代表额度用尽。
                next.runtime.status = match next.quota.windows_exhausted() {
                    Some(true) => ProviderStatus::Warning,
                    Some(false) => ProviderStatus::Ok,
                    None if next.quota.available <= 0.0 => ProviderStatus::Warning,
                    None => ProviderStatus::Ok,
                };
                next.quota.known = true;
                next.quota.total_known = false;
                next.quota.unlimited = false;
                next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
                next.runtime.error_message = None;
                next
            }
            Err(_message) if matches!(provider.auth.mode, AuthMode::ApiKey) => {
                // Key 的额度与模型列表是两条独立的读取：扣了额度但模型接口
                // 短暂失败时，卡片仍应显示刚读到的余额。
                let model_result = refresh_models_if_available(client, &mut next).await;
                let key_quota = fetch_key_quota(client, &next).await;
                next.quota.known = key_quota.is_some();
                next.quota.total_known = key_quota.as_ref().is_some_and(|quota| quota.total_known);
                next.quota.available = key_quota.as_ref().map_or(0.0, |quota| quota.available);
                next.quota.used = key_quota.as_ref().map_or(0.0, |quota| quota.used);
                next.quota.unlimited = false;
                next.quota.scope = crate::models::ProviderQuotaScope::Token;
                next.quota.windows = key_quota.map(|quota| quota.windows).unwrap_or_default();
                next.runtime.status = match next.quota.windows_exhausted() {
                    Some(true) => ProviderStatus::Warning,
                    Some(false) => ProviderStatus::Ok,
                    // 有 Key 总额度时余额为 0 才代表用完；额度未公开时不做判断。
                    None if next.quota.total_known && next.quota.available <= 0.0 => {
                        ProviderStatus::Warning
                    }
                    None => ProviderStatus::Ok,
                };
                next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
                next.runtime.error_message = None;
                match model_result {
                    Ok(()) => next,
                    Err(_) if next.quota.known => next,
                    Err(model_error) => provider_with_error(&next, model_error),
                }
            }
            Err(message) => provider_with_error(&next, message),
        }
    }
}

/// Sub2API 的登录/刷新都会轮换令牌；任何一次账号请求拿到的新令牌都必须写回
/// provider，否则下一次提交旧令牌会触发服务端的重用检测。
fn apply_rotated_tokens(next: &mut Provider, authenticated: &Provider) {
    if !authenticated.auth.access_token.trim().is_empty() {
        next.auth.access_token = authenticated.auth.access_token.clone();
    }
    if !authenticated.auth.refresh_token.trim().is_empty() {
        next.auth.refresh_token = authenticated.auth.refresh_token.clone();
    }
    if authenticated.auth.access_token_expires_at.is_some() {
        next.auth.access_token_expires_at = authenticated.auth.access_token_expires_at;
    }
}

/// 订阅进度是账号维度的补充信息：接口失败不影响已读到的余额，因此所有错误都
/// 收口成空列表。这里刻意只带当前访问令牌、不走账号重登流程——一次补充请求
/// 不该触发令牌子链轮换，更不该在失败时丢掉刚轮换出来的刷新令牌。
async fn fetch_subscription_windows(
    client: &ProviderTransport,
    provider: &Provider,
) -> Vec<ProviderQuotaWindow> {
    let Ok(url) = api_url(&provider.identity.base_url, "/subscriptions/progress") else {
        return Vec::new();
    };
    let Ok(data) = request_json_with_timeout(
        client,
        reqwest::Method::GET,
        url,
        Some(Credential::Jwt(provider.auth.access_token.clone())),
        None,
        "读取订阅额度",
        Duration::from_secs(8),
    )
    .await
    else {
        return Vec::new();
    };
    let items = data.as_array().cloned().unwrap_or_default();
    subscription_windows(&items)
}

async fn authenticate_account_with_refresh(
    client: &ProviderTransport,
    working: &mut Provider,
    token_refresh_succeeded: bool,
) -> Result<(Provider, Value), String> {
    match authenticate_account(client, working).await {
        Ok(result) => Ok(result),
        Err(message)
            if !token_refresh_succeeded
                && is_auth_failure(&message)
                && !working.auth.refresh_token.trim().is_empty() =>
        {
            match refresh_tokens(client, working).await {
                Ok(refreshed) => {
                    *working = refreshed;
                    authenticate_account(client, working)
                        .await
                        .map_err(|retry_error| {
                            format!("{message}；刷新令牌后重试失败: {retry_error}")
                        })
                }
                Err(refresh_error) if is_refresh_chain_broken(&refresh_error) => {
                    working.auth.access_token.clear();
                    working.auth.refresh_token.clear();
                    working.auth.access_token_expires_at = None;
                    Err(format!("{message}；刷新 Sub2API 令牌失败: {refresh_error}"))
                }
                Err(refresh_error) => {
                    Err(format!("{message}；刷新 Sub2API 令牌失败: {refresh_error}"))
                }
            }
        }
        Err(message) => Err(message),
    }
}

async fn refresh_models_if_available(
    client: &ProviderTransport,
    provider: &mut Provider,
) -> Result<(), String> {
    match crate::adapters::api::fetch_models(client, provider).await {
        Ok(models) => {
            provider
                .capabilities
                .set_available_models(crate::models::ProviderModelList::new(
                    models,
                    crate::models::ProviderModelScope::ApiKey,
                ));
            Ok(())
        }
        Err(error) => {
            provider.capabilities.available_models_state.error = Some(error.clone());
            Err(error)
        }
    }
}

fn provider_with_error(provider: &Provider, message: String) -> Provider {
    let mut next = provider.clone();
    next.runtime.status = ProviderStatus::Error;
    next.runtime.error_message = Some(message);
    next
}
