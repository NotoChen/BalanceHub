use crate::{
    adapters::newapi::NewApiAdapter,
    models::{Provider, ProviderConnectionTestResult, ProviderInput, ProviderStatus},
    util::{unix_millis as current_timestamp_millis, unix_secs},
};

use super::ProviderService;

impl<'a> ProviderService<'a> {
    pub async fn test_connection(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderConnectionTestResult, String> {
        let data = self.snapshot();
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(input, provider_id);
        let result = NewApiAdapter
            .test_connection(&data.settings, &provider)
            .await?;
        if result.ok {
            self.apply_connection_test_result(&provider.identity.id, &result)?;
        }
        Ok(result)
    }

    fn apply_connection_test_result(
        &self,
        provider_id: &str,
        result: &ProviderConnectionTestResult,
    ) -> Result<(), String> {
        let Some(available) = result.available else {
            return Ok(());
        };
        let Some(used) = result.used else {
            return Ok(());
        };
        let quota_display = result.quota_display.clone();
        let synced_at = unix_secs().to_string();
        self.mutate(|data| {
            if let Some(provider) = data
                .providers
                .iter_mut()
                .find(|provider| provider.identity.id == provider_id)
            {
                provider.quota.available = available;
                provider.quota.used = used;
                provider.quota.display_type = quota_display.quota_display_type;
                provider.quota.currency_symbol = quota_display.currency_symbol;
                provider.runtime.status = ProviderStatus::Ok;
                provider.automation.last_synced_at = Some(synced_at);
                provider.runtime.error_message = None;
            }
        })
    }
}
