use crate::{
    adapters::resolve_provider_adapter,
    models::{Provider, ProviderApiKeyOption, ProviderInput},
    util::unix_millis as current_timestamp_millis,
};

use super::{find_provider, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn list_api_keys(&self, id: String) -> Result<Vec<ProviderApiKeyOption>, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        resolve_provider_adapter(&provider)
            .list_api_keys(&data.settings, &provider)
            .await
    }

    pub async fn create_api_key(
        &self,
        id: String,
        name: String,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        let adapter = resolve_provider_adapter(&provider);
        adapter
            .create_api_key(&data.settings, &provider, &name)
            .await?;
        adapter.list_api_keys(&data.settings, &provider).await
    }

    pub async fn create_api_key_for_input(
        &self,
        input: ProviderInput,
        name: String,
    ) -> Result<ProviderApiKeyOption, String> {
        let data = self.snapshot();
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(input, provider_id);
        resolve_provider_adapter(&provider)
            .create_api_key(&data.settings, &provider, &name)
            .await
    }

    pub async fn delete_api_key(
        &self,
        id: String,
        token_id: String,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        let adapter = resolve_provider_adapter(&provider);
        adapter
            .delete_api_key(&data.settings, &provider, &token_id)
            .await?;
        adapter.list_api_keys(&data.settings, &provider).await
    }
}
