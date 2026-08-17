use super::ApiAdapter;
use crate::{
    adapters::protocol::contracts::{
        CapabilityProbe, ConnectionCapability, CredentialCapability, ProviderOperationOutcome,
    },
    models::{
        AppSettings, Provider, ProviderCapabilities, ProviderConnectionTestResult,
        ProviderCredentialCompletionResult, ProviderInput, ProviderSiteProbeResult,
    },
};
use async_trait::async_trait;

#[async_trait]
impl CredentialCapability for ApiAdapter {
    async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        ApiAdapter::complete_credentials(self, settings, input, provider_id).await
    }
}

#[async_trait]
impl ConnectionCapability for ApiAdapter {
    async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderConnectionTestResult>, String> {
        ApiAdapter::test_connection(self, settings, provider)
            .await
            .map(ProviderOperationOutcome::unchanged)
    }

    async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        ApiAdapter::probe_site(self, settings, provider).await
    }

    async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderOperationOutcome<()> {
        ProviderOperationOutcome::refreshed(
            provider,
            ApiAdapter::refresh_provider(self, settings, provider).await,
        )
    }
}

#[async_trait]
impl CapabilityProbe for ApiAdapter {
    async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<(ProviderCapabilities, String, Option<String>)>, String>
    {
        ApiAdapter::probe_capabilities(self, settings, provider)
            .await
            .map(ProviderOperationOutcome::unchanged)
    }
}
