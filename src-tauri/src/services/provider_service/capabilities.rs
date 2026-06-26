use crate::{
    adapters::resolve_provider_adapter,
    models::{
        normalize_invite_link, provider_domain, CodexModelSyncResult, Provider,
        ProviderCapabilityProbeResult, ProviderInput, ProviderSiteProbeResult,
    },
    util::unix_millis as current_timestamp_millis,
};

use super::{codex_models::fetch_codex_models, find_provider, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn probe_site(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderSiteProbeResult, String> {
        let data = self.snapshot();
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(input, provider_id);
        resolve_provider_adapter(&provider)
            .probe_site(&data.settings, &provider)
            .await
    }

    pub async fn sync_capabilities(
        &self,
        id: String,
    ) -> Result<ProviderCapabilityProbeResult, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        let (capabilities, invite_link, error) = resolve_provider_adapter(&provider)
            .probe_capabilities(&data.settings, &provider)
            .await?;
        let models_result = if provider_domain::auth::has_api_key(&provider) {
            Some(fetch_codex_models(&data.settings, &provider).await)
        } else {
            None
        };
        let synced_at = current_timestamp_millis().to_string();
        let (providers, updated_provider, message) = self.mutate(|data| {
            let mut updated_provider = provider.clone();
            let mut message = "站点能力已同步".to_string();
            if let Some(stored_provider) = data
                .providers
                .iter_mut()
                .find(|stored| stored.identity.id == id)
            {
                stored_provider.capabilities = capabilities;
                stored_provider.capabilities.invite_link = invite_link;
                stored_provider.capabilities.synced_at = Some(synced_at);
                stored_provider.capabilities.error_message = error;
                if let Some(result) = models_result {
                    match result {
                        Ok(models) => {
                            stored_provider.capabilities.available_models = models.clone();
                            if !models.is_empty() {
                                message = format!("站点能力已同步，已获取 {} 个模型", models.len());
                            }
                        }
                        Err(err) => {
                            message = format!("站点能力已同步，模型列表同步失败：{err}");
                        }
                    }
                }
                updated_provider = stored_provider.clone();
            }
            (data.providers.clone(), updated_provider, message)
        })?;
        Ok(ProviderCapabilityProbeResult {
            providers,
            provider: updated_provider,
            message,
        })
    }

    pub async fn sync_codex_models(&self, id: String) -> Result<CodexModelSyncResult, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        let models = fetch_codex_models(&data.settings, &provider).await?;
        let stored_models = models.clone();
        let (providers, updated_provider) = self.mutate(|data| {
            let mut updated_provider = provider.clone();
            if let Some(stored_provider) = data
                .providers
                .iter_mut()
                .find(|stored| stored.identity.id == id)
            {
                stored_provider.capabilities.available_models = stored_models;
                updated_provider = stored_provider.clone();
            }
            (data.providers.clone(), updated_provider)
        })?;
        Ok(CodexModelSyncResult {
            providers,
            provider: updated_provider,
            message: format!("已获取 {} 个模型", models.len()),
            models,
        })
    }

    pub async fn invite_link(&self, id: String) -> Result<String, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        if !provider.capabilities.invite_link.trim().is_empty() {
            let invite_link = normalize_invite_link(&provider.capabilities.invite_link);
            if invite_link != provider.capabilities.invite_link {
                let stored_link = invite_link.clone();
                self.mutate(|data| {
                    if let Some(stored_provider) = data
                        .providers
                        .iter_mut()
                        .find(|stored| stored.identity.id == id)
                    {
                        stored_provider.capabilities.invite_link = stored_link;
                    }
                })?;
            }
            return Ok(invite_link);
        }

        let invite_link = resolve_provider_adapter(&provider)
            .invite_link(&data.settings, &provider)
            .await?;
        let stored_link = invite_link.clone();
        self.mutate(|data| {
            if let Some(stored_provider) = data
                .providers
                .iter_mut()
                .find(|stored| stored.identity.id == id)
            {
                stored_provider.capabilities.invite_link = stored_link;
                stored_provider.capabilities.invitation_known = true;
                stored_provider.capabilities.invitation_supported = true;
                stored_provider.capabilities.synced_at =
                    Some(current_timestamp_millis().to_string());
                stored_provider.capabilities.error_message = None;
            }
        })?;
        Ok(invite_link)
    }
}
