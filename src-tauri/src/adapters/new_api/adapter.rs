use super::anyrouter;
use super::check_in::probe_check_in_capability;
use super::http::{
    access_token_fallback_provider, build_url, build_user_request, normalize_base_url,
    provider_user_management_context, retry_with_access_token, should_retry_with_access_token,
    ProviderTransport,
};
pub(crate) use super::http::{
    authenticate_password_provider, build_client, is_anyrouter_base_url, login_password_provider,
    provider_is_anyrouter,
};
use super::keys::{
    create_api_key, delete_api_key, fetch_api_key_options, probe_api_key_management,
};
use super::response::{extract_string_field, parse_success_data, send_text};
use super::site::fetch_site_metadata;
pub use super::site::SiteMetadata;
use crate::models::{
    AppSettings, AuthMode, Provider, ProviderApiKeyOption, ProviderCapabilities,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderCredentialCompletionResult, ProviderInput, ProviderQuotaDisplay,
    ProviderRequestLogsQuery, ProviderRequestLogsResult, ProviderSiteProbeResult,
    ProviderUsageSummary,
};
use reqwest::Method;

pub(crate) struct NewApiAdapter;

impl NewApiAdapter {
    pub(crate) async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        if matches!(input.auth.mode, AuthMode::ApiKey) {
            return Err("API Key 认证没有可补全的账号凭据".to_string());
        }
        let provider = Provider::from_input(input.clone(), provider_id);
        let client = build_client(settings, &provider).await?;
        super::credentials::complete_credentials(&client, input).await
    }

    pub(crate) async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, ProviderConnectionTestResult), String> {
        match build_client(settings, provider).await {
            Ok(client) => super::quota::test_connection(&client, provider).await,
            Err(message) => Ok((
                provider.clone(),
                ProviderConnectionTestResult {
                    ok: false,
                    message,
                    available: None,
                    used: None,
                    quota_display: ProviderQuotaDisplay::default(),
                    steps: Vec::new(),
                },
            )),
        }
    }

    pub(crate) async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        let client = build_client(settings, provider).await?;
        match discover_site_metadata(&client, &provider.identity.base_url).await {
            Ok(site) => Ok(ProviderSiteProbeResult {
                ok: true,
                message: "站点可访问，已发现中转站名称".to_string(),
                system_name: Some(site.system_name),
                logo: if site.logo.trim().is_empty() {
                    None
                } else {
                    Some(site.logo)
                },
                quota_display: ProviderQuotaDisplay {
                    quota_display_type: site.quota_display_type,
                    currency_symbol: site.currency_symbol,
                },
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

    pub(crate) async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, Vec<ProviderApiKeyOption>), String> {
        let client = build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        let (provider, options) = retry_with_access_token(
            &client,
            &authenticated,
            list_api_keys(&client, &authenticated),
            |candidate| {
                let retry_client = client.clone();
                async move { list_api_keys(&retry_client, &candidate).await }
            },
        )
        .await?;
        Ok((provider, options))
    }

    pub(crate) async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<(Provider, ProviderApiKeyOption), String> {
        let client = build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        let (provider, option) = retry_with_access_token(
            &client,
            &authenticated,
            create_managed_api_key(&client, &authenticated, name),
            |candidate| {
                let retry_client = client.clone();
                async move { create_managed_api_key(&retry_client, &candidate, name).await }
            },
        )
        .await?;
        Ok((provider, option))
    }

    pub(crate) async fn generate_access_token(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Err("API Key 认证不支持生成访问令牌".to_string());
        }
        let client = build_client(settings, provider).await?;
        let provider = authenticated_provider(&client, provider).await?;
        super::credentials::create_access_token(&client, &provider).await
    }

    pub(crate) async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<(Provider, ()), String> {
        let client = build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        let (provider, ()) = retry_with_access_token(
            &client,
            &authenticated,
            delete_managed_api_key(&client, &authenticated, token_id),
            |candidate| {
                let retry_client = client.clone();
                async move { delete_managed_api_key(&retry_client, &candidate, token_id).await }
            },
        )
        .await?;
        Ok((provider, ()))
    }

    pub(crate) async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<(Provider, ProviderUsageSummary), String> {
        let client = build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        let (provider, value) =
            retry_with_access_token(
                &client,
                &authenticated,
                super::usage::fetch_usage_summary(&client, &authenticated, period),
                |candidate| {
                    let retry_client = client.clone();
                    async move {
                        super::usage::fetch_usage_summary(&retry_client, &candidate, period).await
                    }
                },
            )
            .await?;
        Ok((provider, value))
    }

    pub(crate) async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<(Provider, ProviderRequestLogsResult), String> {
        let client = build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        let (provider, value) = retry_with_access_token(
            &client,
            &authenticated,
            super::logs::fetch_request_logs(&client, &authenticated, query.clone()),
            |candidate| {
                let retry_client = client.clone();
                async move {
                    super::logs::fetch_request_logs(&retry_client, &candidate, query.clone()).await
                }
            },
        )
        .await?;
        Ok((provider, value))
    }

    pub(crate) async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<(Provider, String), String> {
        let client = build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        let (provider, value) = retry_with_access_token(
            &client,
            &authenticated,
            super::account::change_user_password(
                &client,
                &authenticated,
                original_password,
                password,
            ),
            |candidate| {
                let retry_client = client.clone();
                async move {
                    super::account::change_user_password(
                        &retry_client,
                        &candidate,
                        original_password,
                        password,
                    )
                    .await
                }
            },
        )
        .await?;
        Ok((provider, value))
    }

    pub(crate) async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, (ProviderCapabilities, String, Option<String>)), String> {
        match build_client(settings, provider).await {
            Ok(client) => match authenticated_provider(&client, provider).await {
                Ok(provider) => {
                    let first = probe_capabilities(&client, &provider).await;
                    if first
                        .2
                        .as_deref()
                        .is_some_and(should_retry_with_access_token)
                    {
                        if let Some(fallback) = access_token_fallback_provider(&provider) {
                            let retry = probe_capabilities(&client, &fallback).await;
                            return Ok((fallback, retry));
                        }
                    }
                    Ok((provider, first))
                }
                Err(message) => Ok((
                    provider.clone(),
                    (
                        ProviderCapabilities::default(),
                        String::new(),
                        Some(message),
                    ),
                )),
            },
            Err(message) => Ok((
                provider.clone(),
                (
                    ProviderCapabilities::default(),
                    String::new(),
                    Some(message),
                ),
            )),
        }
    }

    pub(crate) async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, String), String> {
        let client = build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        let (provider, value) = retry_with_access_token(
            &client,
            &authenticated,
            fetch_invite_link(&client, &authenticated),
            |candidate| {
                let retry_client = client.clone();
                async move { fetch_invite_link(&retry_client, &candidate).await }
            },
        )
        .await?;
        Ok((provider, value))
    }

    pub(crate) async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Provider {
        match crate::adapters::transport::build_client(settings, provider).await {
            Ok(client) => {
                let mut refreshed = super::quota::refresh_provider(&client, provider).await;
                if refreshed.auth.api_key.trim().is_empty() {
                    return refreshed;
                }

                let quota_failed = matches!(
                    refreshed.runtime.status,
                    crate::models::ProviderStatus::Error
                );
                match crate::adapters::api::fetch_models(&client, &refreshed).await {
                    Ok(models) => {
                        refreshed.capabilities.available_models = models;
                        if quota_failed && matches!(provider.auth.mode, AuthMode::ApiKey) {
                            // API Key endpoints often expose /models but not account quota.
                            // A model refresh is still useful and should not leave the card
                            // permanently in an error state just because quota is unavailable.
                            refreshed.quota.available = 0.0;
                            refreshed.quota.used = 0.0;
                            refreshed.quota.known = false;
                            refreshed.quota.total_known = false;
                            refreshed.quota.unlimited = false;
                            refreshed.quota.scope = crate::models::ProviderQuotaScope::Token;
                            refreshed.runtime.status = crate::models::ProviderStatus::Ok;
                            refreshed.runtime.error_message = None;
                            refreshed.automation.last_synced_at =
                                Some(crate::util::unix_secs().to_string());
                        }
                    }
                    Err(model_error) if matches!(provider.auth.mode, AuthMode::ApiKey) => {
                        if quota_failed {
                            let quota_error = refreshed
                                .runtime
                                .error_message
                                .take()
                                .unwrap_or_else(|| "额度刷新失败".to_string());
                            refreshed.runtime.error_message =
                                Some(format!("{quota_error}；模型列表获取失败: {model_error}"));
                        } else {
                            refreshed.runtime.status = crate::models::ProviderStatus::Warning;
                            refreshed.runtime.error_message =
                                Some(format!("额度已更新；模型列表获取失败: {model_error}"));
                        }
                    }
                    Err(_) => {}
                }
                refreshed
            }
            Err(message) => provider_with_error(provider, message),
        }
    }

    pub(crate) async fn check_in(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, ProviderCheckInResult), String> {
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Err("API Key 认证不支持用户签到，请切换到 Cookie 或访问令牌".to_string());
        }
        let client = crate::adapters::transport::build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        let first = check_in_for_provider(&client, &authenticated).await;
        match first {
            Ok(value)
                if value.ok
                    || client
                        .shield_blocked_for(&authenticated.identity.base_url)
                        .await
                        .is_some()
                    || !should_retry_with_access_token(&value.message) =>
            {
                Ok((authenticated, value))
            }
            Ok(value) => {
                let Some(fallback_provider) = access_token_fallback_provider(&authenticated) else {
                    return Ok((authenticated, value));
                };
                let retry_client = client.clone();
                let retry = check_in_for_provider(&retry_client, &fallback_provider)
                    .await
                    .map_err(|retry_error| {
                        format!(
                            "{}；已尝试改用访问令牌，仍失败: {retry_error}",
                            value.message
                        )
                    })?;
                Ok((fallback_provider, retry))
            }
            Err(message) => {
                let (provider, value) = retry_with_access_token(
                    &client,
                    &authenticated,
                    async { Err::<ProviderCheckInResult, String>(message.clone()) },
                    |candidate| {
                        let retry_client = client.clone();
                        async move { check_in_for_provider(&retry_client, &candidate).await }
                    },
                )
                .await?;
                Ok((provider, value))
            }
        }
    }

    pub(crate) async fn check_in_records(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        month: &str,
    ) -> Result<(Provider, ProviderCheckInRecordsResult), String> {
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Err("API Key 认证不支持签到记录，请切换到 Cookie 或访问令牌".to_string());
        }
        let client = build_client(settings, provider).await?;
        let authenticated = authenticated_provider(&client, provider).await?;
        if provider_is_anyrouter(&authenticated) {
            return Err("当前暂未发现 AnyRouter 的签到历史接口".to_string());
        }
        let (provider, value) = retry_with_access_token(
            &client,
            &authenticated,
            super::check_in::fetch_check_in_records(&client, &authenticated, month),
            |candidate| {
                let retry_client = client.clone();
                async move {
                    super::check_in::fetch_check_in_records(&retry_client, &candidate, month).await
                }
            },
        )
        .await?;
        Ok((provider, value))
    }
}

fn provider_with_error(provider: &Provider, message: String) -> Provider {
    let mut next = provider.clone();
    next.runtime.status = crate::models::ProviderStatus::Error;
    next.runtime.error_message = Some(message);
    next
}

async fn authenticated_provider(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<Provider, String> {
    if matches!(provider.auth.mode, AuthMode::Password) {
        login_password_provider(client, provider).await
    } else {
        authenticate_password_provider(client, provider).await
    }
}

async fn check_in_for_provider(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<ProviderCheckInResult, String> {
    if provider_is_anyrouter(provider) {
        anyrouter::check_in_provider(client, provider).await
    } else {
        super::check_in::check_in_provider(client, provider).await
    }
}

pub async fn discover_site_metadata(
    client: &ProviderTransport,
    base_url: &str,
) -> Result<SiteMetadata, String> {
    let base_url = normalize_base_url(base_url);
    if base_url.is_empty() {
        return Err("请先填写中转站地址".to_string());
    }
    fetch_site_metadata(client, &base_url).await
}

pub async fn list_api_keys(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    fetch_api_key_options(client, &base_url, &api_user, credential).await
}

pub async fn create_managed_api_key(
    client: &ProviderTransport,
    provider: &Provider,
    name: &str,
) -> Result<ProviderApiKeyOption, String> {
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    create_api_key(client, &base_url, &api_user, credential, name).await
}

pub async fn delete_managed_api_key(
    client: &ProviderTransport,
    provider: &Provider,
    token_id: &str,
) -> Result<(), String> {
    if token_id.trim().is_empty() {
        return Err("缺少 API 密钥 ID".to_string());
    }
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    delete_api_key(client, &base_url, &api_user, credential, token_id).await
}

pub async fn probe_capabilities(
    client: &ProviderTransport,
    provider: &Provider,
) -> (ProviderCapabilities, String, Option<String>) {
    let mut capabilities = ProviderCapabilities::default();
    let mut invite_link = String::new();
    let mut errors = Vec::new();
    let base_url = normalize_base_url(&provider.identity.base_url);

    if base_url.is_empty() {
        return (
            capabilities,
            invite_link,
            Some("缺少中转站地址，无法探测站点能力".to_string()),
        );
    }

    let is_anyrouter = provider_is_anyrouter(provider);
    if matches!(provider.auth.mode, AuthMode::ApiKey) {
        capabilities.check_in_known = true;
        capabilities.check_in_supported = false;
        capabilities.api_key_management_known = true;
        capabilities.api_key_management_supported = false;
        capabilities.invitation_known = true;
        capabilities.invitation_supported = false;
        return (capabilities, invite_link, None);
    }

    if is_anyrouter {
        capabilities.check_in_known = true;
        capabilities.check_in_supported = !provider.auth.session_cookie.trim().is_empty();
        if capabilities.check_in_supported {
            capabilities.check_in_auth_modes.push(AuthMode::Session);
        }
    } else {
        match probe_check_in_capability(client, provider, &base_url).await {
            Ok(modes) => {
                capabilities.check_in_known = true;
                capabilities.check_in_supported = !modes.is_empty();
                capabilities.check_in_auth_modes = modes;
            }
            Err(message) => errors.push(format!("签到能力: {message}")),
        }
    }

    let api_key_management_context = provider_user_management_context(provider);
    match api_key_management_context {
        Ok((base_url, api_user, credential)) => {
            match probe_api_key_management(client, &base_url, &api_user, credential).await {
                Ok(()) => {
                    capabilities.api_key_management_known = true;
                    capabilities.api_key_management_supported = true;
                }
                Err(message) => {
                    capabilities.api_key_management_known = true;
                    capabilities.api_key_management_supported = false;
                    errors.push(format!("密钥管理: {message}"));
                }
            }
        }
        Err(message) => {
            capabilities.api_key_management_known = true;
            capabilities.api_key_management_supported = false;
            errors.push(format!("密钥管理: {message}"));
        }
    }

    match fetch_invite_link(client, provider).await {
        Ok(link) => {
            capabilities.invitation_known = true;
            capabilities.invitation_supported = true;
            invite_link = link;
        }
        Err(message) => {
            capabilities.invitation_known = true;
            capabilities.invitation_supported = false;
            errors.push(format!("邀请链接: {message}"));
        }
    }

    let error = if errors.is_empty() {
        None
    } else {
        Some(errors.join("；"))
    };

    (capabilities, invite_link, error)
}

pub async fn fetch_invite_link(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<String, String> {
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    let url = build_url(&base_url, "/api/user/aff")?;
    let request = build_user_request(client, Method::GET, url, &base_url, &api_user, credential);
    let (status, body) = send_text(client, request, "读取邀请链接").await?;
    let data = parse_success_data(&status, body, "邀请链接")?;
    let code = data
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            extract_string_field(
                &data,
                &["aff_code", "affCode", "code", "invite_code", "inviteCode"],
            )
        })
        .ok_or_else(|| "接口没有返回邀请码".to_string())?;
    Ok(format!(
        "{}/register?aff={}",
        base_url.trim_end_matches('/'),
        code
    ))
}
