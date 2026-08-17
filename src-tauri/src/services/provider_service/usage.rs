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
        self.persist_operation_credentials(
            &ProviderRequestContext::capture(&provider),
            &operation.credentials,
        )
        .await?
        .ok_or_else(|| "本地配置已变更，本次用量结果已忽略".to_string())?;
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
        self.persist_operation_credentials(
            &ProviderRequestContext::capture(&provider),
            &operation.credentials,
        )
        .await?
        .ok_or_else(|| "本地配置已变更，本次请求日志结果已忽略".to_string())?;
        Ok(operation.value)
    }
}
