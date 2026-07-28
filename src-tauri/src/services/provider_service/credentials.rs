use crate::{
    adapters::protocol::ProtocolAdapter,
    models::{Provider, ProviderCredentialCompletionResult, ProviderInput},
    util::unix_millis as current_timestamp_millis,
};

use super::ProviderService;

impl<'a> ProviderService<'a> {
    pub async fn complete_credentials(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        let data = self.snapshot();
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        ProtocolAdapter
            .complete_credentials(&data.settings, input, provider_id)
            .await
    }

    pub async fn generate_access_token_for_input(
        &self,
        input: ProviderInput,
    ) -> Result<String, String> {
        let data = self.snapshot();
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(input, provider_id);
        ProtocolAdapter
            .generate_access_token(&data.settings, &provider)
            .await
    }
}
