use crate::{
    adapters::resolve_provider_adapter,
    models::{Provider, ProviderCredentialCompletionResult, ProviderInput},
    util::unix_millis as current_timestamp_millis,
};

use super::{find_provider, ProviderService};

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
        resolve_provider_adapter(&Provider::from_input(input.clone(), provider_id.clone()))
            .complete_credentials(&data.settings, input, provider_id)
            .await
    }

    pub async fn generate_access_token(&self, id: String) -> Result<Vec<Provider>, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        let access_token = resolve_provider_adapter(&provider)
            .generate_access_token(&data.settings, &provider)
            .await?;
        self.mutate(|data| {
            if let Some(stored_provider) = data
                .providers
                .iter_mut()
                .find(|stored| stored.identity.id == id)
            {
                stored_provider.auth.access_token = access_token;
            }
            data.providers.clone()
        })
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
        resolve_provider_adapter(&provider)
            .generate_access_token(&data.settings, &provider)
            .await
    }
}
