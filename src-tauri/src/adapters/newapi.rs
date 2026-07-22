use crate::models::{
    AppSettings, AuthMode, Provider, ProviderApiKeyOption, ProviderCapabilities,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderCredentialCompletionResult, ProviderInput, ProviderQuotaDisplay,
    ProviderRequestLogsQuery, ProviderRequestLogsResult, ProviderSiteProbeResult,
    ProviderUsageSummary,
};
use crate::providers::{anyrouter, newapi};

pub struct NewApiAdapter;

impl NewApiAdapter {
    pub async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        if matches!(input.auth.mode, AuthMode::ApiKey) {
            return Err("API Key 认证没有可补全的账号凭据".to_string());
        }
        let provider = Provider::from_input(input.clone(), provider_id);
        let client = newapi::build_client(settings, &provider)?;
        newapi::complete_credentials(&client, input).await
    }

    pub async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderConnectionTestResult, String> {
        match newapi::build_client(settings, provider) {
            Ok(client) => newapi::test_connection(&client, provider).await,
            Err(message) => Ok(ProviderConnectionTestResult {
                ok: false,
                message,
                available: None,
                used: None,
                quota_display: ProviderQuotaDisplay::default(),
                steps: Vec::new(),
            }),
        }
    }

    pub async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        let client = newapi::build_client(settings, provider)?;
        match newapi::discover_site_metadata(&client, &provider.identity.base_url).await {
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

    pub async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        newapi::list_api_keys(&client, &provider).await
    }

    pub async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderApiKeyOption, String> {
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        newapi::create_managed_api_key(&client, &provider, name).await
    }

    pub async fn generate_access_token(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Err("API Key 认证不支持生成访问令牌".to_string());
        }
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        newapi::create_access_token(&client, &provider).await
    }

    pub async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<(), String> {
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        newapi::delete_managed_api_key(&client, &provider, token_id).await
    }

    pub async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderUsageSummary, String> {
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        newapi::fetch_usage_summary(&client, &provider, period).await
    }

    pub async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderRequestLogsResult, String> {
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        newapi::fetch_request_logs(&client, &provider, query).await
    }

    pub async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<String, String> {
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        newapi::change_user_password(&client, &provider, original_password, password).await
    }

    pub async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(ProviderCapabilities, String, Option<String>), String> {
        match newapi::build_client(settings, provider) {
            Ok(client) => match authenticated_provider(&client, provider).await {
                Ok(provider) => Ok(newapi::probe_capabilities(&client, &provider).await),
                Err(message) => Ok((
                    ProviderCapabilities::default(),
                    String::new(),
                    Some(message),
                )),
            },
            Err(message) => Ok((
                ProviderCapabilities::default(),
                String::new(),
                Some(message),
            )),
        }
    }

    pub async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        newapi::fetch_invite_link(&client, &provider).await
    }

    pub async fn refresh_provider(&self, settings: &AppSettings, provider: &Provider) -> Provider {
        match newapi::build_client(settings, provider) {
            Ok(client) => newapi::refresh_provider(&client, provider).await,
            Err(message) => provider_with_error(provider, message),
        }
    }

    pub async fn check_in(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderCheckInResult, String> {
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Err("API Key 认证不支持用户签到，请切换到 Cookie 或访问令牌".to_string());
        }
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        if newapi::provider_is_anyrouter(&provider) {
            anyrouter::check_in_provider(&client, &provider).await
        } else {
            newapi::check_in_provider(&client, &provider).await
        }
    }

    pub async fn check_in_records(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        month: &str,
    ) -> Result<ProviderCheckInRecordsResult, String> {
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Err("API Key 认证不支持签到记录，请切换到 Cookie 或访问令牌".to_string());
        }
        let client = newapi::build_client(settings, provider)?;
        let provider = authenticated_provider(&client, provider).await?;
        if newapi::provider_is_anyrouter(&provider) {
            return Err("当前暂未发现 AnyRouter 的签到历史接口".to_string());
        }
        newapi::fetch_check_in_records(&client, &provider, month).await
    }
}

fn provider_with_error(provider: &Provider, message: String) -> Provider {
    let mut next = provider.clone();
    next.runtime.status = crate::models::ProviderStatus::Error;
    next.runtime.error_message = Some(message);
    next
}

async fn authenticated_provider(
    client: &reqwest::Client,
    provider: &Provider,
) -> Result<Provider, String> {
    if matches!(provider.auth.mode, AuthMode::Password) {
        newapi::login_password_provider(client, provider).await
    } else {
        newapi::authenticate_password_provider(client, provider).await
    }
}
