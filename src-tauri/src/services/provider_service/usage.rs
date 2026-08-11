use crate::{
    adapters::protocol::ProtocolAdapter,
    models::{ProviderRequestLogsQuery, ProviderRequestLogsResult, ProviderUsageSummary},
};
use tauri::Manager;

use super::{find_provider, ProviderRequestContext, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn usage_summary(
        &self,
        id: String,
        period: String,
    ) -> Result<ProviderUsageSummary, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let operation = ProtocolAdapter
            .usage_summary(&data.settings, &provider, &period)
            .await?;
        self.persist_operation_provider(
            &ProviderRequestContext::capture(&provider),
            &operation.provider,
        )
        .await?;
        Ok(operation.value)
    }

    pub async fn request_logs(
        &self,
        id: String,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderRequestLogsResult, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let operation = ProtocolAdapter
            .request_logs(&data.settings, &provider, query)
            .await?;
        self.persist_operation_provider(
            &ProviderRequestContext::capture(&provider),
            &operation.provider,
        )
        .await?;
        Ok(operation.value)
    }
}
