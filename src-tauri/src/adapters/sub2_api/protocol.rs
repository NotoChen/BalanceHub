use super::Sub2ApiAdapter;
use crate::{
    adapters::protocol::contracts::{
        AccessTokenCapability, AccountCapability, AnnouncementCapability,
        ApiKeyManagementCapability, CapabilityProbe, ConnectionCapability, CredentialCapability,
        ProviderOperationOutcome, UsageCapability,
    },
    models::{
        AppSettings, Provider, ProviderApiKeyOption, ProviderCapabilities,
        ProviderConnectionTestResult, ProviderCredentialCompletionResult, ProviderInput,
        ProviderRequestLogsQuery, ProviderRequestLogsResult, ProviderSiteProbeResult,
        ProviderUsageSummary, SiteAnnouncement,
    },
};
use async_trait::async_trait;

#[async_trait]
impl CredentialCapability for Sub2ApiAdapter {
    async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        Sub2ApiAdapter::complete_credentials(self, settings, input, provider_id).await
    }
}

#[async_trait]
impl AccessTokenCapability for Sub2ApiAdapter {
    async fn generate_access_token(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        Sub2ApiAdapter::generate_access_token(self, settings, provider).await
    }
}

#[async_trait]
impl ConnectionCapability for Sub2ApiAdapter {
    async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderConnectionTestResult>, String> {
        Sub2ApiAdapter::test_connection(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        Sub2ApiAdapter::probe_site(self, settings, provider).await
    }

    async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderOperationOutcome<()> {
        ProviderOperationOutcome::refreshed(
            provider,
            Sub2ApiAdapter::refresh_provider(self, settings, provider).await,
        )
    }
}

#[async_trait]
impl ApiKeyManagementCapability for Sub2ApiAdapter {
    async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<Vec<ProviderApiKeyOption>>, String> {
        Sub2ApiAdapter::list_api_keys(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderOperationOutcome<ProviderApiKeyOption>, String> {
        Sub2ApiAdapter::create_api_key(self, settings, provider, name)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<ProviderOperationOutcome<()>, String> {
        Sub2ApiAdapter::delete_api_key(self, settings, provider, token_id)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl UsageCapability for Sub2ApiAdapter {
    async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderOperationOutcome<ProviderUsageSummary>, String> {
        Sub2ApiAdapter::usage_summary(self, settings, provider, period)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderOperationOutcome<ProviderRequestLogsResult>, String> {
        Sub2ApiAdapter::request_logs(self, settings, provider, query)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl AccountCapability for Sub2ApiAdapter {
    async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<ProviderOperationOutcome<String>, String> {
        Sub2ApiAdapter::change_password(self, settings, provider, original_password, password)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<String>, String> {
        Sub2ApiAdapter::invite_link(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl CapabilityProbe for Sub2ApiAdapter {
    async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<(ProviderCapabilities, String, Option<String>)>, String>
    {
        Sub2ApiAdapter::probe_capabilities(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl AnnouncementCapability for Sub2ApiAdapter {
    async fn list_announcements(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<Vec<SiteAnnouncement>>, String> {
        Sub2ApiAdapter::list_announcements(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn mark_announcement_read(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        announcement_id: &str,
    ) -> Result<ProviderOperationOutcome<()>, String> {
        Sub2ApiAdapter::mark_announcement_read(self, settings, provider, announcement_id)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}
