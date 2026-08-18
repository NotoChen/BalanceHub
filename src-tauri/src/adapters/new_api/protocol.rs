use super::NewApiAdapter;
use crate::{
    adapters::protocol::contracts::{
        AccessTokenCapability, AccountCapability, AnnouncementCapability,
        ApiKeyManagementCapability, CapabilityProbe, CheckInCapability, ConnectionCapability,
        CredentialCapability, ProviderOperationOutcome, UsageCapability,
    },
    models::{
        AppSettings, Provider, ProviderApiKeyOption, ProviderCapabilities,
        ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
        ProviderCredentialCompletionResult, ProviderInput, ProviderRequestLogsQuery,
        ProviderRequestLogsResult, ProviderSiteProbeResult, ProviderUsageSummary, SiteAnnouncement,
    },
};
use async_trait::async_trait;

#[async_trait]
impl CredentialCapability for NewApiAdapter {
    async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        NewApiAdapter::complete_credentials(self, settings, input, provider_id).await
    }
}

#[async_trait]
impl AccessTokenCapability for NewApiAdapter {
    async fn generate_access_token(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        NewApiAdapter::generate_access_token(self, settings, provider).await
    }
}

#[async_trait]
impl ConnectionCapability for NewApiAdapter {
    async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderConnectionTestResult>, String> {
        NewApiAdapter::test_connection(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        NewApiAdapter::probe_site(self, settings, provider).await
    }

    async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderOperationOutcome<()> {
        ProviderOperationOutcome::refreshed(
            provider,
            NewApiAdapter::refresh_provider(self, settings, provider).await,
        )
    }
}

#[async_trait]
impl ApiKeyManagementCapability for NewApiAdapter {
    async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<Vec<ProviderApiKeyOption>>, String> {
        NewApiAdapter::list_api_keys(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderOperationOutcome<ProviderApiKeyOption>, String> {
        NewApiAdapter::create_api_key(self, settings, provider, name)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<ProviderOperationOutcome<()>, String> {
        NewApiAdapter::delete_api_key(self, settings, provider, token_id)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl UsageCapability for NewApiAdapter {
    async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderOperationOutcome<ProviderUsageSummary>, String> {
        NewApiAdapter::usage_summary(self, settings, provider, period)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderOperationOutcome<ProviderRequestLogsResult>, String> {
        NewApiAdapter::request_logs(self, settings, provider, query)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl AccountCapability for NewApiAdapter {
    async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<ProviderOperationOutcome<String>, String> {
        NewApiAdapter::change_password(self, settings, provider, original_password, password)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<String>, String> {
        NewApiAdapter::invite_link(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl CapabilityProbe for NewApiAdapter {
    async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<(ProviderCapabilities, String, Option<String>)>, String>
    {
        NewApiAdapter::probe_capabilities(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl CheckInCapability for NewApiAdapter {
    async fn check_in(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, String> {
        NewApiAdapter::check_in(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn check_in_records(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        month: &str,
    ) -> Result<ProviderOperationOutcome<ProviderCheckInRecordsResult>, String> {
        NewApiAdapter::check_in_records(self, settings, provider, month)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}

#[async_trait]
impl AnnouncementCapability for NewApiAdapter {
    async fn list_announcements(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<Vec<SiteAnnouncement>>, String> {
        NewApiAdapter::list_announcements(self, settings, provider)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }

    async fn mark_announcement_read(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        announcement_id: &str,
    ) -> Result<ProviderOperationOutcome<()>, String> {
        NewApiAdapter::mark_announcement_read(self, settings, provider, announcement_id)
            .await
            .map(|result| ProviderOperationOutcome::from_authenticated_result(provider, result))
    }
}
