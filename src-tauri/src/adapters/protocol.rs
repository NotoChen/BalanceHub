use super::{
    api::ApiAdapter,
    new_api::{provider_is_anyrouter, NewApiAdapter},
    sub2_api::Sub2ApiAdapter,
};
use crate::models::{
    AppSettings, Provider, ProviderApiKeyOption, ProviderCapabilities,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderCredentialCompletionResult, ProviderInput, ProviderProtocol, ProviderRequestLogsQuery,
    ProviderRequestLogsResult, ProviderSiteProbeResult, ProviderUsageSummary,
};

/// Result of a provider operation together with the credentials that were
/// actually used. Account protocols may refresh or re-issue credentials while
/// serving an otherwise successful request; callers must persist this provider
/// atomically instead of discarding the rotated token/session.
#[derive(Debug)]
pub(crate) struct ProviderOperationResult<T> {
    pub(crate) provider: Provider,
    pub(crate) value: T,
}

pub(crate) struct ProtocolAdapter;

impl ProtocolAdapter {
    pub(crate) fn is_anyrouter(&self, provider: &Provider) -> bool {
        matches!(provider.identity.protocol, ProviderProtocol::NewApi)
            && provider_is_anyrouter(provider)
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
    ) -> Result<ProviderOperationResult<ProviderConnectionTestResult>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter.test_connection(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter.test_connection(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter.test_connection(settings, provider).await?,
            }),
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
    ) -> Result<ProviderOperationResult<Vec<ProviderApiKeyOption>>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter.list_api_keys(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter.list_api_keys(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter.list_api_keys(settings, provider).await?,
            }),
        }
    }

    pub(crate) async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderOperationResult<ProviderApiKeyOption>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter
                    .create_api_key(settings, provider, name)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter
                    .create_api_key(settings, provider, name)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter.create_api_key(settings, provider, name).await?,
            }),
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
    ) -> Result<ProviderOperationResult<()>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter
                    .delete_api_key(settings, provider, token_id)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter
                    .delete_api_key(settings, provider, token_id)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter
                    .delete_api_key(settings, provider, token_id)
                    .await?,
            }),
        }
    }

    pub(crate) async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderOperationResult<ProviderUsageSummary>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter
                    .usage_summary(settings, provider, period)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter
                    .usage_summary(settings, provider, period)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter.usage_summary(settings, provider, period).await?,
            }),
        }
    }

    pub(crate) async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderOperationResult<ProviderRequestLogsResult>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter
                    .request_logs(settings, provider, query)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter
                    .request_logs(settings, provider, query)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter.request_logs(settings, provider, query).await?,
            }),
        }
    }

    pub(crate) async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<ProviderOperationResult<String>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter
                    .change_password(settings, provider, original_password, password)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter
                    .change_password(settings, provider, original_password, password)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter
                    .change_password(settings, provider, original_password, password)
                    .await?,
            }),
        }
    }

    pub(crate) async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationResult<(ProviderCapabilities, String, Option<String>)>, String>
    {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) =
                    NewApiAdapter.probe_capabilities(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter
                    .probe_capabilities(settings, provider)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter.probe_capabilities(settings, provider).await?,
            }),
        }
    }

    pub(crate) async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationResult<String>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter.invite_link(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter.invite_link(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter.invite_link(settings, provider).await?,
            }),
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
    ) -> Result<ProviderOperationResult<ProviderCheckInResult>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter.check_in(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter.check_in(settings, provider).await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter.check_in(settings, provider).await?,
            }),
        }
    }

    pub(crate) async fn check_in_records(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        month: &str,
    ) -> Result<ProviderOperationResult<ProviderCheckInRecordsResult>, String> {
        match provider.identity.protocol {
            ProviderProtocol::NewApi => {
                let (provider, value) = NewApiAdapter
                    .check_in_records(settings, provider, month)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Sub2Api => {
                let (provider, value) = Sub2ApiAdapter
                    .check_in_records(settings, provider, month)
                    .await?;
                Ok(ProviderOperationResult { provider, value })
            }
            ProviderProtocol::Api => Ok(ProviderOperationResult {
                provider: provider.clone(),
                value: ApiAdapter
                    .check_in_records(settings, provider, month)
                    .await?,
            }),
        }
    }
}
