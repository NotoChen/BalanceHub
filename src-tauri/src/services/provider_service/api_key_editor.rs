use tauri::Manager;

use crate::{
    adapters::protocol::{contracts::ProviderOperationOutcome, ProtocolAdapter},
    models::{
        ProviderApiKeyEditorContext, ProviderApiKeyOption, ProviderApiKeyPatch, ProviderInput,
    },
};

use super::{find_provider, ProviderRequestContext, ProviderService};

impl ProviderService<'_> {
    pub async fn api_key_editor_context(
        &self,
        id: String,
        token_id: Option<String>,
    ) -> Result<ProviderApiKeyEditorContext, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
        let context = ProviderRequestContext::capture(&provider);
        let operation = ProtocolAdapter
            .api_key_editor_context(&data.settings, &provider, token_id.as_deref())
            .await?;
        self.persist_operation_credentials(&context, &operation.credentials)
            .await?
            .ok_or("账号配置已变更，请重新打开 Key 设置")?;
        Ok(operation.value)
    }

    pub async fn api_key_editor_context_for_input(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderApiKeyEditorContext, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = self.prepare_input_provider(&data.settings, input).await?;
        let operation = ProtocolAdapter
            .api_key_editor_context(&data.settings, &provider, None)
            .await?;
        Ok(operation.value)
    }

    pub async fn create_api_key(
        &self,
        id: String,
        credential_revision: u64,
        patch: ProviderApiKeyPatch,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        self.save_remote_api_key(id, credential_revision, None, patch)
            .await
    }

    pub async fn update_api_key(
        &self,
        id: String,
        credential_revision: u64,
        token_id: String,
        patch: ProviderApiKeyPatch,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        self.save_remote_api_key(id, credential_revision, Some(token_id), patch)
            .await
    }

    async fn save_remote_api_key(
        &self,
        id: String,
        credential_revision: u64,
        token_id: Option<String>,
        patch: ProviderApiKeyPatch,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        if provider.auth.credential_revision != credential_revision {
            return Err("账号或当前 Key 已变更，请重新打开设置".into());
        }
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
        let context = ProviderRequestContext::capture(&provider);
        let operation = match token_id.as_deref() {
            Some(token_id) => {
                ProtocolAdapter
                    .update_api_key(&data.settings, &provider, token_id, &patch)
                    .await?
            }
            None => {
                ProtocolAdapter
                    .create_api_key(&data.settings, &provider, &patch)
                    .await?
            }
        };
        self.persist_remote_key(&context, operation)
            .await
            .map_err(|error| {
                format!("站点已保存 Key，但本地同步失败，请重新同步站点 Key 确认：{error}")
            })?;
        Ok(find_provider(&self.snapshot_async().await?, &id)?
            .auth
            .api_key_options)
    }

    async fn persist_remote_key(
        &self,
        context: &ProviderRequestContext,
        operation: ProviderOperationOutcome<ProviderApiKeyOption>,
    ) -> Result<(), String> {
        let provider = self
            .persist_operation_credentials(context, &operation.credentials)
            .await?
            .ok_or("账号配置已变更，本次 Key 结果已忽略")?;
        let context = ProviderRequestContext::capture(&provider);
        let mut updated = vec![operation.value];
        ProviderApiKeyOption::merge_cached_key_material(
            &mut updated,
            &provider.auth.api_key_options,
            provider.identity.protocol,
        );
        let updated = updated.remove(0);
        let mut options = provider.auth.api_key_options;
        if let Some(position) = options.iter().position(|old| {
            old.token_id == updated.token_id
                || (!updated.local_id.is_empty() && old.local_id == updated.local_id)
        }) {
            options[position] = updated;
        } else {
            options.insert(0, updated);
        }
        // Mutation responses update only the affected Key; listing/revealing the
        // entire account again adds latency and can turn a successful write into an error.
        self.persist_api_key_options(&context, &options, None).await
    }

    pub async fn create_api_key_for_input(
        &self,
        input: ProviderInput,
        patch: ProviderApiKeyPatch,
    ) -> Result<ProviderApiKeyOption, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = self.prepare_input_provider(&data.settings, input).await?;
        Ok(ProtocolAdapter
            .create_api_key(&data.settings, &provider, &patch)
            .await?
            .value)
    }
}
