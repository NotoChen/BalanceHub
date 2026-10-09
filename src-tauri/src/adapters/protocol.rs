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
    pub(crate) async fn fetch_available_models_with_account_auth<Prepare, Prepared>(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        prepare_account: Prepare,
    ) -> Result<ProviderOperationOutcome<crate::models::ProviderModelList>, String>
    where
        Prepare: FnOnce(Provider) -> Prepared,
        Prepared: std::future::Future<Output = Result<Provider, String>>,
    {
        if provider.identity.protocol != crate::models::ProviderProtocol::NewApi {
            return self.fetch_available_models(settings, provider).await;
        }
        if let Some(reason) =
            crate::models::provider_domain::model_list::action(provider).unavailable_reason
        {
            return Err(reason.to_string());
        }
        let client = crate::adapters::transport::build_client(settings, provider).await?;
        super::new_api::fetch_models_with_account_auth(&client, provider, prepare_account).await
    }

    pub(crate) async fn fetch_available_models(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<crate::models::ProviderModelList>, String> {
        if let Some(reason) =
            crate::models::provider_domain::model_list::action(provider).unavailable_reason
        {
            return Err(reason.to_string());
        }
        let client = crate::adapters::transport::build_client(settings, provider).await?;
        definition(provider.identity.protocol)
            .connection
            .fetch_available_models(&client, provider)
            .await
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
        patch: &crate::models::ProviderApiKeyPatch,
    ) -> Result<ProviderOperationOutcome<ProviderApiKeyOption>, String> {
        Self::require_key_management(provider)?;
        let definition = definition(provider.identity.protocol);
        definition
            .api_keys
            .ok_or_else(|| definition.unsupported("创建 API Key"))?
            .create_api_key(settings, provider, patch)
            .await
    }

    pub(crate) async fn api_key_editor_context(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: Option<&str>,
    ) -> Result<ProviderOperationOutcome<crate::models::ProviderApiKeyEditorContext>, String> {
        Self::require_key_management(provider)?;
        let definition = definition(provider.identity.protocol);
        definition
            .api_keys
            .ok_or_else(|| definition.unsupported("编辑 API Key"))?
            .api_key_editor_context(settings, provider, token_id)
            .await
    }

    pub(crate) async fn update_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
        patch: &crate::models::ProviderApiKeyPatch,
    ) -> Result<ProviderOperationOutcome<ProviderApiKeyOption>, String> {
        Self::require_key_management(provider)?;
        let definition = definition(provider.identity.protocol);
        definition
            .api_keys
            .ok_or_else(|| definition.unsupported("编辑 API Key"))?
            .update_api_key(settings, provider, token_id, patch)
            .await
    }

    fn require_key_management(provider: &Provider) -> Result<(), String> {
        if !crate::models::provider_domain::capabilities::supports_account_management(provider) {
            return Err("管理站点 Key 需要账号认证，请先登录".into());
        }
        Ok(())
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
