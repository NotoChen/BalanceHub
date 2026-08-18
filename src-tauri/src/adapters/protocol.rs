pub(crate) mod contracts;
mod definition;
mod registry;

pub(crate) use definition::{
    ProtocolDetectionRole, ProviderProtocolAuthSchema, ProviderProtocolDefinition,
};
pub(crate) use registry::{definition, definitions};

use crate::models::{
    AppSettings, Provider, ProviderApiKeyOption, ProviderCapabilities,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderCredentialCompletionResult, ProviderInput, ProviderRequestLogsQuery,
    ProviderRequestLogsResult, ProviderSiteProbeResult, ProviderUsageSummary, SiteAnnouncement,
};
use contracts::ProviderOperationOutcome;

/// Runtime protocol facade. Registration metadata and capability objects live
/// in `protocol/registry`; this type only routes a business operation to the
/// selected definition.
pub(crate) struct ProtocolAdapter;

impl ProtocolAdapter {
    pub(crate) fn is_anyrouter(&self, provider: &Provider) -> bool {
        (definition(provider.identity.protocol).is_anyrouter)(provider)
    }

    pub(crate) async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        definition(input.identity.protocol)
            .credentials
            .complete_credentials(settings, input, provider_id)
            .await
    }

    pub(crate) async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderConnectionTestResult>, String> {
        definition(provider.identity.protocol)
            .connection
            .test_connection(settings, provider)
            .await
    }

    pub(crate) async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        definition(provider.identity.protocol)
            .connection
            .probe_site(settings, provider)
            .await
    }

    pub(crate) async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<Vec<ProviderApiKeyOption>>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .api_keys
            .ok_or_else(|| definition.unsupported("查询 API Key 列表"))?
            .list_api_keys(settings, provider)
            .await
    }

    pub(crate) async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderOperationOutcome<ProviderApiKeyOption>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .api_keys
            .ok_or_else(|| definition.unsupported("创建 API Key"))?
            .create_api_key(settings, provider, name)
            .await
    }

    pub(crate) async fn generate_access_token(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .access_token
            .ok_or_else(|| definition.unsupported("生成访问令牌"))?
            .generate_access_token(settings, provider)
            .await
    }

    pub(crate) async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<ProviderOperationOutcome<()>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .api_keys
            .ok_or_else(|| definition.unsupported("删除 API Key"))?
            .delete_api_key(settings, provider, token_id)
            .await
    }

    pub(crate) async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderOperationOutcome<ProviderUsageSummary>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .usage
            .ok_or_else(|| definition.unsupported("读取用量趋势"))?
            .usage_summary(settings, provider, period)
            .await
    }

    pub(crate) async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderOperationOutcome<ProviderRequestLogsResult>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .usage
            .ok_or_else(|| definition.unsupported("读取请求日志"))?
            .request_logs(settings, provider, query)
            .await
    }

    pub(crate) async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<ProviderOperationOutcome<String>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .account
            .ok_or_else(|| definition.unsupported("修改密码"))?
            .change_password(settings, provider, original_password, password)
            .await
    }

    pub(crate) async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<(ProviderCapabilities, String, Option<String>)>, String>
    {
        definition(provider.identity.protocol)
            .capability_probe
            .probe_capabilities(settings, provider)
            .await
    }

    pub(crate) async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<String>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .account
            .ok_or_else(|| definition.unsupported("读取邀请链接"))?
            .invite_link(settings, provider)
            .await
    }

    pub(crate) async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderOperationOutcome<()> {
        definition(provider.identity.protocol)
            .connection
            .refresh_provider(settings, provider)
            .await
    }

    pub(crate) async fn check_in(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .check_in
            .ok_or_else(|| definition.unsupported("用户签到"))?
            .check_in(settings, provider)
            .await
    }

    pub(crate) async fn check_in_records(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        month: &str,
    ) -> Result<ProviderOperationOutcome<ProviderCheckInRecordsResult>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .check_in
            .ok_or_else(|| definition.unsupported("读取签到记录"))?
            .check_in_records(settings, provider, month)
            .await
    }

    pub(crate) async fn list_announcements(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<Vec<SiteAnnouncement>>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .announcements
            .ok_or_else(|| definition.unsupported("读取站点公告"))?
            .list_announcements(settings, provider)
            .await
    }

    pub(crate) async fn mark_announcement_read(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        announcement_id: &str,
    ) -> Result<ProviderOperationOutcome<()>, String> {
        let definition = definition(provider.identity.protocol);
        definition
            .announcements
            .ok_or_else(|| definition.unsupported("标记公告已读"))?
            .mark_announcement_read(settings, provider, announcement_id)
            .await
    }
}
