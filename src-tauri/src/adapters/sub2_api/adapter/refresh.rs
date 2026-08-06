use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        sub2_api::{
            auth::{
                authenticate_account, is_auth_failure, is_refresh_chain_broken,
                needs_token_refresh, refresh_tokens,
            },
            json::string_field,
            profile::{apply_user, fetch_site, quota_display},
        },
        transport::{build_client, ProviderTransport},
    },
    limits,
    models::{
        AppSettings, AuthMode, Provider, ProviderCheckInRecordsResult, ProviderCheckInResult,
        ProviderConnectionTestResult, ProviderConnectionTestStep, ProviderQuotaDisplay,
        ProviderSiteProbeResult, ProviderStatus,
    },
};
use serde_json::Value;

impl Sub2ApiAdapter {
    pub(crate) async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderConnectionTestResult, String> {
        let client = build_client(settings, provider).await?;
        let site = fetch_site(&client, &provider.identity.base_url).await.ok();
        let mut steps = Vec::new();

        // 测试连接只做只读验证，不能使用会轮换的 refresh_token。Sub2API 每次刷新都会
        // 立即作废旧 token，而连接测试的返回结构不负责持久化新 token；若在这里轮换，
        // 存储中就会留下已失效的旧 token。访问令牌失效时仍可用账号密码临时重登验证。
        let mut connection_provider = provider.clone();
        connection_provider.auth.refresh_token.clear();
        let refreshed = self
            .refresh_provider_with_client(&client, &connection_provider, site)
            .await;
        match refreshed.runtime.status {
            ProviderStatus::Error => Ok(ProviderConnectionTestResult {
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
            }),
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
                Ok(ProviderConnectionTestResult {
                    ok: true,
                    message: steps[0].message.clone(),
                    available,
                    used,
                    quota_display: quota_display(&refreshed),
                    steps,
                })
            }
        }
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

    pub(crate) async fn check_in(
        &self,
        _settings: &AppSettings,
        _provider: &Provider,
    ) -> Result<ProviderCheckInResult, String> {
        Err("Sub2API 不提供用户签到接口".to_string())
    }

    pub(crate) async fn check_in_records(
        &self,
        _settings: &AppSettings,
        _provider: &Provider,
        _month: &str,
    ) -> Result<ProviderCheckInRecordsResult, String> {
        Err("Sub2API 不提供签到记录接口".to_string())
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

        // 唯一使用 refresh_token 的地方：持久化刷新路径（配合刷新闸门单飞），避免
        // 已轮换的 refresh_token 被重复提交而触发服务端「重用攻击」吊销整个会话家族。
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
                if !authenticated.auth.access_token.trim().is_empty() {
                    next.auth.access_token = authenticated.auth.access_token.clone();
                }
                if !authenticated.auth.refresh_token.trim().is_empty() {
                    next.auth.refresh_token = authenticated.auth.refresh_token.clone();
                }
                if authenticated.auth.access_token_expires_at.is_some() {
                    next.auth.access_token_expires_at = authenticated.auth.access_token_expires_at;
                }
                apply_user(&mut next, &user);
                let _ = refresh_models_if_available(client, &mut next).await;
                next.runtime.status = if next.quota.available <= 0.0 {
                    ProviderStatus::Warning
                } else {
                    ProviderStatus::Ok
                };
                next.quota.known = true;
                next.quota.total_known = false;
                next.quota.unlimited = false;
                next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
                next.runtime.error_message = None;
                next
            }
            Err(_message) if matches!(provider.auth.mode, AuthMode::ApiKey) => {
                match refresh_models_if_available(client, &mut next).await {
                    Ok(()) => {
                        next.quota.known = false;
                        next.quota.total_known = false;
                        next.quota.available = 0.0;
                        next.quota.used = 0.0;
                        next.quota.unlimited = false;
                        next.quota.scope = crate::models::ProviderQuotaScope::Token;
                        next.runtime.status = ProviderStatus::Ok;
                        next.runtime.error_message = None;
                        next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
                        next
                    }
                    Err(model_error) => provider_with_error(&next, model_error),
                }
            }
            Err(message) => provider_with_error(&next, message),
        }
    }
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
    if provider.auth.api_key.trim().is_empty() {
        return Err("缺少 API Key，无法获取模型列表".to_string());
    }
    provider.capabilities.available_models =
        crate::adapters::api::fetch_models(client, provider).await?;
    Ok(())
}

fn provider_with_error(provider: &Provider, message: String) -> Provider {
    let mut next = provider.clone();
    next.runtime.status = ProviderStatus::Error;
    next.runtime.error_message = Some(message);
    next
}
