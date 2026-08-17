use crate::{
    adapters::protocol::{contracts::ProviderCredentialPatch, ProtocolAdapter},
    models::{Provider, ProviderCredentialCompletionResult, ProviderInput},
    state::AppState,
    util::unix_millis as current_timestamp_millis,
};
use tauri::Manager;

use super::{MutationDecision, ProviderRequestContext, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn complete_credentials(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
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
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(input, provider_id);
        ProtocolAdapter
            .generate_access_token(&data.settings, &provider)
            .await
    }

    /// Persist credentials produced by an authenticated adapter operation.
    ///
    /// Authentication can refresh a JWT, rotate a refresh token, or turn a
    /// password login into a reusable NewAPI session. The operation started
    /// from a snapshot, so merge only when that exact snapshot is still active.
    pub(super) async fn current_operation_provider(
        &self,
        request_context: &ProviderRequestContext,
    ) -> Result<Option<Provider>, String> {
        let app = self.app.clone();
        let request_context = request_context.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<AppState>()
                .data
                .read()
                .unwrap_or_else(|err| err.into_inner())
                .providers
                .iter()
                .find(|stored| request_context.matches(stored))
                .cloned()
        })
        .await
        .map_err(|err| format!("读取认证快照任务异常: {err}"))
    }

    pub(super) async fn persist_operation_credentials(
        &self,
        request_context: &ProviderRequestContext,
        credentials: &ProviderCredentialPatch,
    ) -> Result<Option<Provider>, String> {
        if credentials.is_empty() {
            return self.current_operation_provider(request_context).await;
        }

        let request_context = request_context.clone();
        let credentials = credentials.clone();
        self.mutate_decided_async(move |data| {
            let Some(stored) = data
                .providers
                .iter_mut()
                .find(|stored| request_context.matches(stored))
            else {
                return Ok(MutationDecision::unchanged(None));
            };
            let changed = credentials.apply(stored);
            let provider = Some(stored.clone());
            Ok(if changed {
                MutationDecision::changed(provider)
            } else {
                MutationDecision::unchanged(provider)
            })
        })
        .await
    }
}
