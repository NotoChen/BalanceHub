use crate::{
    adapters::newapi::NewApiAdapter,
    models::{ProviderRequestLogsQuery, ProviderRequestLogsResult, ProviderUsageSummary},
};

use super::{find_provider, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn usage_summary(
        &self,
        id: String,
        period: String,
    ) -> Result<ProviderUsageSummary, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        NewApiAdapter
            .usage_summary(&data.settings, &provider, &period)
            .await
    }

    pub async fn request_logs(
        &self,
        id: String,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderRequestLogsResult, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        NewApiAdapter
            .request_logs(&data.settings, &provider, query)
            .await
    }
}
