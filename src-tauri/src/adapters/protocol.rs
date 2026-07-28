use super::{
    api::ApiAdapter,
    new_api::{
        anyrouter_message_indicates_already_checked_in, provider_is_anyrouter, NewApiAdapter,
    },
    sub2_api::Sub2ApiAdapter,
};
use crate::models::{
    AppSettings, Provider, ProviderApiKeyOption, ProviderCapabilities,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderCredentialCompletionResult, ProviderInput, ProviderProtocol, ProviderRequestLogsQuery,
    ProviderRequestLogsResult, ProviderSiteProbeResult, ProviderUsageSummary,
};

pub(crate) struct ProtocolAdapter;

impl ProtocolAdapter {
    pub(crate) fn is_anyrouter(&self, provider: &Provider) -> bool {
        matches!(provider.identity.protocol, ProviderProtocol::NewApi)
            && provider_is_anyrouter(provider)
    }

    pub(crate) fn check_in_message_indicates_already_checked_in(
        &self,
        provider: &Provider,
        message: &str,
    ) -> bool {
        self.is_anyrouter(provider) && anyrouter_message_indicates_already_checked_in(message)
    }

    pub(crate) async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        match input.identity.protocol {
            ProviderProtocol::NewApi => {
                NewApiAdapter
                    .complete_credentials(settings, input, provider_id)
                    .await
            }
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter
                    .complete_credentials(settings, input, provider_id)
                    .await
            }
            ProviderProtocol::Api => {
                ApiAdapter
                    .complete_credentials(settings, input, provider_id)
                    .await
            }
        }
    }

    pub(crate) async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderConnectionTestResult, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => NewApiAdapter.test_connection(settings, provider).await,
            ProviderProtocol::Sub2Api => Sub2ApiAdapter.test_connection(settings, provider).await,
            ProviderProtocol::Api => ApiAdapter.test_connection(settings, provider).await,
        }
    }

    pub(crate) async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => NewApiAdapter.probe_site(settings, provider).await,
            ProviderProtocol::Sub2Api => Sub2ApiAdapter.probe_site(settings, provider).await,
            ProviderProtocol::Api => ApiAdapter.probe_site(settings, provider).await,
        }
    }

    pub(crate) async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => NewApiAdapter.list_api_keys(settings, provider).await,
            ProviderProtocol::Sub2Api => Sub2ApiAdapter.list_api_keys(settings, provider).await,
            ProviderProtocol::Api => ApiAdapter.list_api_keys(settings, provider).await,
        }
    }

    pub(crate) async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderApiKeyOption, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                NewApiAdapter.create_api_key(settings, provider, name).await
            }
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter
                    .create_api_key(settings, provider, name)
                    .await
            }
            ProviderProtocol::Api => ApiAdapter.create_api_key(settings, provider, name).await,
        }
    }

    pub(crate) async fn generate_access_token(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                NewApiAdapter
                    .generate_access_token(settings, provider)
                    .await
            }
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter
                    .generate_access_token(settings, provider)
                    .await
            }
            ProviderProtocol::Api => ApiAdapter.generate_access_token(settings, provider).await,
        }
    }

    pub(crate) async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<(), String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                NewApiAdapter
                    .delete_api_key(settings, provider, token_id)
                    .await
            }
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter
                    .delete_api_key(settings, provider, token_id)
                    .await
            }
            ProviderProtocol::Api => {
                ApiAdapter
                    .delete_api_key(settings, provider, token_id)
                    .await
            }
        }
    }

    pub(crate) async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderUsageSummary, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                NewApiAdapter
                    .usage_summary(settings, provider, period)
                    .await
            }
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter
                    .usage_summary(settings, provider, period)
                    .await
            }
            ProviderProtocol::Api => ApiAdapter.usage_summary(settings, provider, period).await,
        }
    }

    pub(crate) async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderRequestLogsResult, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => NewApiAdapter.request_logs(settings, provider, query).await,
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter.request_logs(settings, provider, query).await
            }
            ProviderProtocol::Api => ApiAdapter.request_logs(settings, provider, query).await,
        }
    }

    pub(crate) async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<String, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                NewApiAdapter
                    .change_password(settings, provider, original_password, password)
                    .await
            }
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter
                    .change_password(settings, provider, original_password, password)
                    .await
            }
            ProviderProtocol::Api => {
                ApiAdapter
                    .change_password(settings, provider, original_password, password)
                    .await
            }
        }
    }

    pub(crate) async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(ProviderCapabilities, String, Option<String>), String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => NewApiAdapter.probe_capabilities(settings, provider).await,
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter.probe_capabilities(settings, provider).await
            }
            ProviderProtocol::Api => ApiAdapter.probe_capabilities(settings, provider).await,
        }
    }

    pub(crate) async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => NewApiAdapter.invite_link(settings, provider).await,
            ProviderProtocol::Sub2Api => Sub2ApiAdapter.invite_link(settings, provider).await,
            ProviderProtocol::Api => ApiAdapter.invite_link(settings, provider).await,
        }
    }

    pub(crate) async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Provider {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => NewApiAdapter.refresh_provider(settings, provider).await,
            ProviderProtocol::Sub2Api => Sub2ApiAdapter.refresh_provider(settings, provider).await,
            ProviderProtocol::Api => ApiAdapter.refresh_provider(settings, provider).await,
        }
    }

    pub(crate) async fn check_in(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderCheckInResult, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => NewApiAdapter.check_in(settings, provider).await,
            ProviderProtocol::Sub2Api => Sub2ApiAdapter.check_in(settings, provider).await,
            ProviderProtocol::Api => ApiAdapter.check_in(settings, provider).await,
        }
    }

    pub(crate) async fn check_in_records(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        month: &str,
    ) -> Result<ProviderCheckInRecordsResult, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                NewApiAdapter
                    .check_in_records(settings, provider, month)
                    .await
            }
            ProviderProtocol::Sub2Api => {
                Sub2ApiAdapter
                    .check_in_records(settings, provider, month)
                    .await
            }
            ProviderProtocol::Api => ApiAdapter.check_in_records(settings, provider, month).await,
        }
    }
}
