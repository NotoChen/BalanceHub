use crate::{
    adapters::{detector::ProtocolDetector, protocol::ProtocolAdapter},
    models::{
        normalize_invite_link, provider_domain, AuthMode, Provider, ProviderCapabilityProbeResult,
        ProviderInput, ProviderModelSyncResult, ProviderProtocol, ProviderProtocolDetectionResult,
        ProviderSiteProbeResult,
    },
    util::unix_millis as current_timestamp_millis,
};
use tauri::Manager;

use super::{find_provider, MutationDecision, ProviderRequestContext, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn detect_protocol(&self, input: ProviderInput) -> ProviderProtocolDetectionResult {
        let data = match self.snapshot_async().await {
            Ok(data) => data,
            Err(error) => {
                return ProviderProtocolDetectionResult {
                    detected_protocol: None,
                    message: format!("读取本地配置失败：{error}"),
                    site: None,
                    ambiguous: false,
                }
            }
        };
        let mut detection_input = input;
        // 协议尚未识别时不能先按 NewAPI 规则给 Key 补 `sk-`，否则 Sub2API 或
        // 通用 API 的自定义前缀会在真正探测前被改写。账号协议的公开探测接口不依赖
        // identity.protocol；API Key 模式先按通用协议保留原值，再由识别结果决定保存规则。
        if matches!(detection_input.auth.mode, AuthMode::ApiKey) {
            detection_input.identity.protocol = ProviderProtocol::Api;
        }
        let provider_id = detection_input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(detection_input, provider_id);
        ProtocolDetector.detect(&data.settings, &provider).await
    }

    pub async fn probe_site(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderSiteProbeResult, String> {
        let data = self.snapshot_async().await?;
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(input, provider_id);
        ProtocolAdapter.probe_site(&data.settings, &provider).await
    }

    pub async fn probe_capabilities(
        &self,
        id: String,
    ) -> Result<ProviderCapabilityProbeResult, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
        let request_context = ProviderRequestContext::capture(&provider);
        let operation = ProtocolAdapter
            .probe_capabilities(&data.settings, &provider)
            .await?;
        let persisted_provider = self
            .persist_operation_credentials(&request_context, &operation.credentials)
            .await?;
        let effective_provider = persisted_provider
            .ok_or_else(|| "本地配置已变更，本次能力探测结果已忽略".to_string())?;
        let mut operation_context = ProviderRequestContext::capture(&effective_provider);
        let (mut capabilities, invite_link, error) = operation.value;
        // Only a successful model response (including []) replaces the snapshot.
        capabilities.available_models = effective_provider.capabilities.available_models.clone();
        capabilities.available_models_state = effective_provider
            .capabilities
            .available_models_state
            .clone();
        let models_result = if provider_domain::model_list::action(&effective_provider).can_sync {
            Some(
                ProtocolAdapter
                    .fetch_available_models(&data.settings, &effective_provider)
                    .await,
            )
        } else {
            None
        };
        let mut model_count = None;
        if let Some(result) = models_result {
            match result {
                Ok(operation) => {
                    let persisted = self
                        .persist_operation_credentials(&operation_context, &operation.credentials)
                        .await?
                        .ok_or_else(|| "本地配置已变更，本次模型结果已忽略".to_string())?;
                    operation_context = ProviderRequestContext::capture(&persisted);
                    model_count = Some(operation.value.models.len());
                    capabilities.set_available_models(operation.value);
                }
                Err(err) => {
                    capabilities.available_models_state.error = Some(err);
                }
            }
        }
        capabilities.error_message = error;
        let probed_at = current_timestamp_millis().to_string();
        let message = model_count
            .map(|count| format!("站点能力已探测，已获取 {count} 个模型"))
            .unwrap_or_else(|| "站点能力已探测".to_string());
        let provider_id = id.clone();
        let mutation_context = operation_context;
        self.mutate_decided_async(move |data| {
            match data
                .providers
                .iter_mut()
                .find(|stored| stored.identity.id == provider_id)
            {
                Some(stored_provider) if mutation_context.matches(stored_provider) => {
                    stored_provider.capabilities = capabilities;
                    stored_provider.capabilities.invite_link = invite_link;
                    stored_provider.capabilities.probed_at = Some(probed_at);
                    Ok(MutationDecision::changed(()))
                }
                Some(_) => Err("本地配置已变更，本次能力探测结果已忽略".to_string()),
                None => Err("中转站已删除，本次能力探测结果已忽略".to_string()),
            }
        })
        .await?;
        let updated_provider = find_provider(&self.snapshot_async().await?, &id)?;
        Ok(ProviderCapabilityProbeResult {
            provider: updated_provider,
            message,
        })
    }

    pub async fn sync_available_models(
        &self,
        id: String,
    ) -> Result<ProviderModelSyncResult, String> {
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        // Key discovery must not wait for, or depend on, account login.
        // Persist any session rotation before account fallback, even when its
        // model request subsequently fails or the frontend stops waiting.
        let mut request_context = ProviderRequestContext::capture(&provider);
        let fallback_context = &mut request_context;
        let settings = &data.settings;
        let models_result = ProtocolAdapter
            .fetch_available_models_with_account_auth(settings, &provider, |candidate| async move {
                let (prepared, authenticated) =
                    self.prepare_operation_auth(settings, &candidate).await?;
                *fallback_context = ProviderRequestContext::capture(&prepared);
                authenticated?;
                Ok(prepared)
            })
            .await;
        if let Ok(operation) = &models_result {
            let persisted = self
                .persist_operation_credentials(&request_context, &operation.credentials)
                .await?
                .ok_or_else(|| "本地配置已变更，本次模型结果已忽略".to_string())?;
            request_context = ProviderRequestContext::capture(&persisted);
        }
        let models_result = models_result.map(|operation| operation.value);
        let stored_result = models_result.clone();
        let provider_id = id.clone();
        let mutation_context = request_context.clone();
        let updated = self
            .mutate_decided_async(move |data| {
                if let Some(stored_provider) = data
                    .providers
                    .iter_mut()
                    .find(|stored| stored.identity.id == provider_id)
                {
                    if mutation_context.matches(stored_provider) {
                        match stored_result {
                            Ok(models) => {
                                stored_provider.capabilities.set_available_models(models);
                            }
                            Err(error) => {
                                if stored_provider
                                    .capabilities
                                    .available_models_state
                                    .error
                                    .as_ref()
                                    == Some(&error)
                                {
                                    return Ok(MutationDecision::unchanged(true));
                                }
                                stored_provider.capabilities.available_models_state.error =
                                    Some(error);
                            }
                        }
                        return Ok(MutationDecision::changed(true));
                    }
                }
                Ok(MutationDecision::unchanged(false))
            })
            .await?;
        if !updated {
            return Err("本地配置已变更，本次模型列表结果已忽略".to_string());
        }
        let result = models_result?;
        let models = result.models;
        let updated_provider = find_provider(&self.snapshot_async().await?, &id)?;
        Ok(ProviderModelSyncResult {
            provider: updated_provider,
            message: format!(
                "已获取 {} 个{}模型",
                models.len(),
                if result.scope == crate::models::ProviderModelScope::Account {
                    "账号可用"
                } else {
                    "Key 可用"
                }
            ),
            models,
        })
    }

    pub async fn invite_link(&self, id: String) -> Result<String, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
        let request_context = ProviderRequestContext::capture(&provider);
        if !provider.capabilities.invite_link.trim().is_empty() {
            let invite_link = normalize_invite_link(&provider.capabilities.invite_link);
            if invite_link != provider.capabilities.invite_link {
                let stored_link = invite_link.clone();
                let provider_id = id.clone();
                let mutation_context = request_context.clone();
                let persisted = self
                    .mutate_decided_async(move |data| {
                        if let Some(stored_provider) = data.providers.iter_mut().find(|stored| {
                            stored.identity.id == provider_id && mutation_context.matches(stored)
                        }) {
                            if stored_provider.capabilities.invite_link == stored_link {
                                return Ok(MutationDecision::unchanged(true));
                            }
                            stored_provider.capabilities.invite_link = stored_link;
                            return Ok(MutationDecision::changed(true));
                        }
                        Ok(MutationDecision::unchanged(false))
                    })
                    .await?;
                if !persisted {
                    return Err("本地配置已变更，本次邀请链接结果已忽略".to_string());
                }
            } else {
                self.current_operation_provider(&request_context)
                    .await?
                    .ok_or_else(|| "本地配置已变更，本次邀请链接结果已忽略".to_string())?;
            }
            return Ok(invite_link);
        }

        let operation = ProtocolAdapter
            .invite_link(&data.settings, &provider)
            .await?;
        let persisted_provider = self
            .persist_operation_credentials(&request_context, &operation.credentials)
            .await?;
        let mutation_context = persisted_provider
            .as_ref()
            .map(ProviderRequestContext::capture)
            .unwrap_or(request_context);
        let invite_link = operation.value;
        let stored_link = invite_link.clone();
        let provider_id = id.clone();
        let persisted = self
            .mutate_decided_async(move |data| {
                if let Some(stored_provider) = data.providers.iter_mut().find(|stored| {
                    stored.identity.id == provider_id && mutation_context.matches(stored)
                }) {
                    stored_provider.capabilities.invite_link = stored_link;
                    stored_provider.capabilities.invitation_known = true;
                    stored_provider.capabilities.invitation_supported = true;
                    stored_provider.capabilities.probed_at =
                        Some(current_timestamp_millis().to_string());
                    stored_provider.capabilities.error_message = None;
                    Ok(MutationDecision::changed(true))
                } else {
                    Ok(MutationDecision::unchanged(false))
                }
            })
            .await?;
        if !persisted {
            return Err("本地配置已变更，本次邀请链接结果已忽略".to_string());
        }
        Ok(invite_link)
    }
}
