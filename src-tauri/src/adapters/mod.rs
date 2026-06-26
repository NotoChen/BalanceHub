pub mod newapi;

use crate::models::{
    AppSettings, Provider, ProviderApiKeyOption, ProviderCapabilities,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderCredentialCompletionResult, ProviderInput, ProviderKind, ProviderRequestLogsQuery,
    ProviderRequestLogsResult, ProviderSiteProbeResult, ProviderUsageSummary,
};
use async_trait::async_trait;

/// 中转站风格适配器。
///
/// 后续接入 sub2api 等新风格时，只需新增一个实现本 trait 的类型，并在
/// [`resolve_provider_adapter`] 里按 `provider_kind` 增加一条分发分支即可，
/// 调用方（`ProviderService`）无需任何改动。
///
/// 注意：anyrouter 不是独立站点类型，而是 NewAPI 的一种接口方言，
/// 由 `NewApiAdapter` 内部按站点地址识别。
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String>;

    async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderConnectionTestResult, String>;

    async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String>;

    async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<Vec<ProviderApiKeyOption>, String>;

    async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderApiKeyOption, String>;

    async fn generate_access_token(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String>;

    async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<(), String>;

    async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderUsageSummary, String>;

    async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderRequestLogsResult, String>;

    async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<String, String>;

    async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(ProviderCapabilities, String, Option<String>), String>;

    async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String>;

    async fn refresh_provider(&self, settings: &AppSettings, provider: &Provider) -> Provider;

    async fn check_in(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderCheckInResult, String>;

    async fn check_in_records(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        month: &str,
    ) -> Result<ProviderCheckInRecordsResult, String>;
}

pub fn resolve_provider_adapter(provider: &Provider) -> Box<dyn ProviderAdapter> {
    match provider.identity.provider_kind {
        ProviderKind::NewApi => Box::new(newapi::NewApiAdapter),
    }
}
