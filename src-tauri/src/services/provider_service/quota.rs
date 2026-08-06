use crate::{
    adapters::protocol::ProtocolAdapter,
    models::{Provider, ProviderConnectionTestResult, ProviderInput, ProviderStatus},
    util::{unix_millis as current_timestamp_millis, unix_secs},
};

use super::{ProviderRequestContext, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn test_connection(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderConnectionTestResult, String> {
        let data = self.snapshot_async().await?;
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(input, provider_id);
        let request_context = ProviderRequestContext::capture(&provider);
        let result = ProtocolAdapter
            .test_connection(&data.settings, &provider)
            .await?;
        if result.ok {
            self.apply_connection_test_result(&request_context, &result)
                .await?;
        }
        Ok(result)
    }

    async fn apply_connection_test_result(
        &self,
        request_context: &ProviderRequestContext,
        result: &ProviderConnectionTestResult,
    ) -> Result<(), String> {
        let Some(available) = result.available else {
            return Ok(());
        };
        let used = result.used;
        let quota_display = result.quota_display.clone();
        let synced_at = unix_secs().to_string();
        let mutation_context = request_context.clone();
        self.mutate_async(move |data| {
            if let Some(provider) = data
                .providers
                .iter_mut()
                .find(|provider| mutation_context.matches(provider))
            {
                provider.quota.available = available;
                provider.quota.used = used.unwrap_or_default();
                provider.quota.known = true;
                provider.quota.total_known = used.is_some();
                provider.quota.display_type = quota_display.quota_display_type;
                provider.quota.currency_symbol = quota_display.currency_symbol;
                provider.runtime.status = ProviderStatus::Ok;
                provider.automation.last_synced_at = Some(synced_at);
                provider.runtime.error_message = None;
            }
        })
        .await
    }
}
