use crate::{
    adapters::{detector::ProtocolDetector, protocol::ProtocolAdapter},
    models::{
        normalize_invite_link, provider_domain, AuthMode, CodexModelSyncResult, Provider,
        ProviderCapabilityProbeResult, ProviderInput, ProviderProtocol,
        ProviderProtocolDetectionResult, ProviderSiteProbeResult,
    },
    util::unix_millis as current_timestamp_millis,
};

use super::{
    codex_models::fetch_codex_models, find_provider, ProviderRequestContext, ProviderService,
};

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
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        let (mut capabilities, invite_link, error) = ProtocolAdapter
            .probe_capabilities(&data.settings, &provider)
            .await?;
        let models_result = if provider_domain::auth::has_api_key(&provider) {
            Some(fetch_codex_models(&data.settings, &provider).await)
        } else {
            None
        };
        let mut capability_errors = error.into_iter().collect::<Vec<_>>();
        let mut model_count = None;
        if let Some(result) = models_result {
            match result {
                Ok(models) => {
                    model_count = Some(models.len());
                    capabilities.available_models = models;
                }
                Err(err) => capability_errors.push(format!("模型列表: {err}")),
            }
        }
        capabilities.error_message = join_capability_errors(capability_errors);
        let probed_at = current_timestamp_millis().to_string();
        let message = model_count
            .filter(|count| *count > 0)
            .map(|count| format!("站点能力已探测，已获取 {count} 个模型"))
            .unwrap_or_else(|| "站点能力已探测".to_string());
        let provider_id = id.clone();
        let mutation_context = request_context.clone();
        let (providers, update_result) = self
            .mutate_async(move |data| {
                let update_result = match data
                    .providers
                    .iter_mut()
                    .find(|stored| stored.identity.id == provider_id)
                {
                    Some(stored_provider) if mutation_context.matches(stored_provider) => {
                        stored_provider.capabilities = capabilities;
                        stored_provider.capabilities.invite_link = invite_link;
                        stored_provider.capabilities.probed_at = Some(probed_at);
                        Ok(stored_provider.clone())
                    }
                    Some(_) => Err("本地配置已变更，本次能力探测结果已忽略".to_string()),
                    None => Err("中转站已删除，本次能力探测结果已忽略".to_string()),
                };
                (data.providers.clone(), update_result)
            })
            .await?;
        let updated_provider = update_result?;
        Ok(ProviderCapabilityProbeResult {
            providers,
            provider: updated_provider,
            message,
        })
    }

    pub async fn sync_codex_models(&self, id: String) -> Result<CodexModelSyncResult, String> {
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        let models = fetch_codex_models(&data.settings, &provider).await?;
        let stored_models = models.clone();
        let provider_id = id.clone();
        let mutation_context = request_context.clone();
        let (providers, updated_provider) = self
            .mutate_async(move |data| {
                let mut updated_provider = None;
                if let Some(stored_provider) = data
                    .providers
                    .iter_mut()
                    .find(|stored| stored.identity.id == provider_id)
                {
                    if mutation_context.matches(stored_provider) {
                        stored_provider.capabilities.available_models = stored_models;
                        updated_provider = Some(stored_provider.clone());
                    }
                }
                (data.providers.clone(), updated_provider)
            })
            .await?;
        let updated_provider =
            updated_provider.ok_or_else(|| "本地配置已变更，本次模型列表结果已忽略".to_string())?;
        Ok(CodexModelSyncResult {
            providers,
            provider: updated_provider,
            message: format!("已获取 {} 个模型", models.len()),
            models,
        })
    }

    pub async fn invite_link(&self, id: String) -> Result<String, String> {
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        if !provider.capabilities.invite_link.trim().is_empty() {
            let invite_link = normalize_invite_link(&provider.capabilities.invite_link);
            if invite_link != provider.capabilities.invite_link {
                let stored_link = invite_link.clone();
                let provider_id = id.clone();
                let mutation_context = request_context.clone();
                self.mutate_async(move |data| {
                    if let Some(stored_provider) = data.providers.iter_mut().find(|stored| {
                        stored.identity.id == provider_id && mutation_context.matches(stored)
                    }) {
                        stored_provider.capabilities.invite_link = stored_link;
                    }
                })
                .await?;
            }
            return Ok(invite_link);
        }

        let invite_link = ProtocolAdapter
            .invite_link(&data.settings, &provider)
            .await?;
        let stored_link = invite_link.clone();
        let provider_id = id.clone();
        let mutation_context = request_context.clone();
        let persisted = self
            .mutate_async(move |data| {
                if let Some(stored_provider) = data.providers.iter_mut().find(|stored| {
                    stored.identity.id == provider_id && mutation_context.matches(stored)
                }) {
                    stored_provider.capabilities.invite_link = stored_link;
                    stored_provider.capabilities.invitation_known = true;
                    stored_provider.capabilities.invitation_supported = true;
                    stored_provider.capabilities.probed_at =
                        Some(current_timestamp_millis().to_string());
                    stored_provider.capabilities.error_message = None;
                    true
                } else {
                    false
                }
            })
            .await?;
        if !persisted {
            return Err("本地配置已变更，本次邀请链接结果已忽略".to_string());
        }
        Ok(invite_link)
    }
}

fn join_capability_errors(errors: Vec<String>) -> Option<String> {
    let mut normalized = Vec::new();
    for error in errors {
        let error = error.trim().to_string();
        if !error.is_empty() && !normalized.contains(&error) {
            normalized.push(error);
        }
    }
    (!normalized.is_empty()).then(|| normalized.join("；"))
}

#[cfg(test)]
mod tests {
    use super::join_capability_errors;

    #[test]
    fn capability_errors_are_deduplicated_and_joined() {
        assert_eq!(
            join_capability_errors(vec![
                "密钥管理: 401".to_string(),
                "".to_string(),
                "密钥管理: 401".to_string(),
                "模型列表: timeout".to_string(),
            ]),
            Some("密钥管理: 401；模型列表: timeout".to_string())
        );
    }
}
