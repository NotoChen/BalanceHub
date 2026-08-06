use crate::{
    adapters::protocol::ProtocolAdapter,
    limits,
    models::{
        normalize_api_key_for_protocol, Provider, ProviderApiKeyOption, ProviderAuth,
        ProviderInput, ProviderProtocol,
    },
    util::unix_millis as current_timestamp_millis,
};

use super::{find_provider, ProviderRequestContext, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn list_api_keys(&self, id: String) -> Result<Vec<ProviderApiKeyOption>, String> {
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        let options = ProtocolAdapter
            .list_api_keys(&data.settings, &provider)
            .await?;
        self.persist_api_key_options(&request_context, &options, None)
            .await?;
        Ok(options)
    }

    pub async fn create_api_key(
        &self,
        id: String,
        name: String,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        let adapter = ProtocolAdapter;
        adapter
            .create_api_key(&data.settings, &provider, &name)
            .await?;
        let options = adapter.list_api_keys(&data.settings, &provider).await?;
        self.persist_api_key_options(&request_context, &options, None)
            .await?;
        Ok(options)
    }

    pub async fn create_api_key_for_input(
        &self,
        input: ProviderInput,
        name: String,
    ) -> Result<ProviderApiKeyOption, String> {
        let data = self.snapshot_async().await?;
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let provider = Provider::from_input(input, provider_id);
        ProtocolAdapter
            .create_api_key(&data.settings, &provider, &name)
            .await
    }

    pub async fn delete_api_key(
        &self,
        id: String,
        token_id: String,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        let adapter = ProtocolAdapter;
        adapter
            .delete_api_key(&data.settings, &provider, &token_id)
            .await?;
        let options = adapter.list_api_keys(&data.settings, &provider).await?;
        self.persist_api_key_options(&request_context, &options, Some(&token_id))
            .await?;
        Ok(options)
    }

    async fn persist_api_key_options(
        &self,
        request_context: &ProviderRequestContext,
        options: &[ProviderApiKeyOption],
        removed_token_id: Option<&str>,
    ) -> Result<(), String> {
        let mutation_context = request_context.clone();
        let options = options.to_vec();
        let removed_token_id = removed_token_id.map(str::to_string);
        let persisted = self
            .mutate_async(move |data| {
                if let Some(provider) = data
                    .providers
                    .iter_mut()
                    .find(|provider| mutation_context.matches(provider))
                {
                    sync_api_key_options(
                        &mut provider.auth,
                        provider.identity.protocol,
                        &options,
                        removed_token_id.as_deref(),
                    );
                    true
                } else {
                    false
                }
            })
            .await?;
        if persisted {
            Ok(())
        } else {
            Err("本地配置已变更，本次 API Key 结果已忽略".to_string())
        }
    }
}

fn sync_api_key_options(
    auth: &mut ProviderAuth,
    protocol: ProviderProtocol,
    options: &[ProviderApiKeyOption],
    removed_token_id: Option<&str>,
) {
    let removed_token_id = removed_token_id.unwrap_or("").trim();
    let current_key = normalize_api_key_for_protocol(&auth.api_key, protocol);
    let current_token_id = auth.api_key_token_id.trim().to_string();
    let mut previous_options = auth.api_key_options.clone();
    if !current_key.is_empty() {
        let mut current = ProviderApiKeyOption::current_for_protocol(&current_key, protocol);
        current.token_id = current_token_id.clone();
        previous_options.push(current);
    }
    let mut cached = options
        .iter()
        .cloned()
        .map(|option| option.normalize_for_protocol(protocol))
        .collect::<Vec<_>>();
    ProviderApiKeyOption::merge_cached_key_material(&mut cached, &previous_options, protocol);

    let selected = cached
        .iter()
        .find(|option| {
            !current_token_id.is_empty()
                && option.token_id == current_token_id
                && option.token_id != removed_token_id
                && option.key_available
        })
        .or_else(|| {
            cached.iter().find(|option| {
                !current_key.is_empty()
                    && option.key == current_key
                    && option.token_id != removed_token_id
                    && option.key_available
            })
        })
        .cloned();

    let selected_was_removed = !removed_token_id.is_empty()
        && (current_token_id == removed_token_id
            || previous_options.iter().any(|option| {
                option.token_id == removed_token_id
                    && !current_key.is_empty()
                    && option.key == current_key
            }));

    if let Some(selected) = selected {
        auth.api_key = selected.key;
        auth.api_key_token_id = selected.token_id;
    } else if selected_was_removed {
        auth.api_key.clear();
        auth.api_key_token_id.clear();
    } else if !current_key.is_empty() && !cached.iter().any(|option| option.key == current_key) {
        // The list endpoint is intentionally capped at 100 items. Keep a
        // previously revealed primary key when it falls outside that window.
        let mut current = ProviderApiKeyOption::current_for_protocol(&current_key, protocol);
        current.token_id = current_token_id.clone();
        cached.insert(0, current);
        auth.api_key = current_key;
    } else if current_key.is_empty() && !current_token_id.is_empty() {
        auth.api_key_token_id.clear();
    }

    if auth.api_key.trim().is_empty() {
        let mut usable = cached.iter().filter(|option| option.key_available);
        if let (Some(option), None) = (usable.next(), usable.next()) {
            auth.api_key = option.key.clone();
            auth.api_key_token_id = option.token_id.clone();
        }
    }
    cached.truncate(limits::MAX_API_KEYS_PER_PROVIDER);
    auth.api_key_options = cached;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(
        protocol: ProviderProtocol,
        token_id: &str,
        key: &str,
        name: &str,
    ) -> ProviderApiKeyOption {
        let mut option = ProviderApiKeyOption::current_for_protocol(key, protocol);
        option.token_id = token_id.to_string();
        option.name = name.to_string();
        option
    }

    #[test]
    fn sync_selects_the_only_available_key() {
        let mut auth = ProviderInput::default().auth;
        let only = option(ProviderProtocol::NewApi, "11", "sk-only", "Only");

        sync_api_key_options(
            &mut auth,
            ProviderProtocol::NewApi,
            std::slice::from_ref(&only),
            None,
        );

        assert_eq!(auth.api_key, "sk-only");
        assert_eq!(auth.api_key_token_id, "11");
        assert_eq!(auth.api_key_options, vec![only]);
    }

    #[test]
    fn sync_keeps_multiple_keys_unselected() {
        let mut auth = ProviderInput::default().auth;
        let options = vec![
            option(ProviderProtocol::NewApi, "11", "sk-first", "First"),
            option(ProviderProtocol::NewApi, "12", "sk-second", "Second"),
        ];

        sync_api_key_options(&mut auth, ProviderProtocol::NewApi, &options, None);

        assert!(auth.api_key.is_empty());
        assert!(auth.api_key_token_id.is_empty());
        assert_eq!(auth.api_key_options, options);
    }

    #[test]
    fn sync_refreshes_metadata_without_losing_a_cached_full_key() {
        let mut auth = ProviderInput::default().auth;
        auth.api_key = "sk-secret".to_string();
        auth.api_key_token_id = "11".to_string();
        auth.api_key_options = vec![option(
            ProviderProtocol::NewApi,
            "11",
            "sk-secret",
            "Old name",
        )];
        let remote = ProviderApiKeyOption {
            name: "New name".to_string(),
            masked_key: "sk-s**********cret".to_string(),
            token_id: "11".to_string(),
            remain_quota: 42.0,
            ..ProviderApiKeyOption::default()
        };

        sync_api_key_options(
            &mut auth,
            ProviderProtocol::NewApi,
            std::slice::from_ref(&remote),
            None,
        );

        assert_eq!(auth.api_key, "sk-secret");
        assert_eq!(auth.api_key_token_id, "11");
        assert_eq!(auth.api_key_options.len(), 1);
        assert_eq!(auth.api_key_options[0].name, "New name");
        assert_eq!(auth.api_key_options[0].remain_quota, 42.0);
        assert_eq!(auth.api_key_options[0].key, "sk-secret");
        assert!(auth.api_key_options[0].key_available);
    }

    #[test]
    fn sync_preserves_primary_key_outside_the_first_hundred_items() {
        let mut auth = ProviderInput::default().auth;
        auth.api_key = "sk-older".to_string();
        auth.api_key_token_id = "101".to_string();
        auth.api_key_options = vec![option(ProviderProtocol::NewApi, "101", "sk-older", "Older")];
        let remote = option(ProviderProtocol::NewApi, "1", "sk-newer", "Newer");

        sync_api_key_options(
            &mut auth,
            ProviderProtocol::NewApi,
            std::slice::from_ref(&remote),
            None,
        );

        assert_eq!(auth.api_key, "sk-older");
        assert_eq!(auth.api_key_token_id, "101");
        assert!(auth
            .api_key_options
            .iter()
            .any(|item| item.token_id == "101" && item.key == "sk-older"));
    }

    #[test]
    fn sync_clears_a_primary_key_only_when_it_was_explicitly_removed() {
        let mut auth = ProviderInput::default().auth;
        auth.api_key = "sk-removed".to_string();
        auth.api_key_token_id = "11".to_string();
        auth.api_key_options = vec![option(
            ProviderProtocol::NewApi,
            "11",
            "sk-removed",
            "Removed",
        )];
        let replacements = vec![
            option(
                ProviderProtocol::NewApi,
                "12",
                "sk-replacement",
                "Replacement",
            ),
            option(ProviderProtocol::NewApi, "13", "sk-other", "Other"),
        ];

        sync_api_key_options(
            &mut auth,
            ProviderProtocol::NewApi,
            &replacements,
            Some("11"),
        );

        assert!(auth.api_key.is_empty());
        assert!(auth.api_key_token_id.is_empty());
        assert_eq!(auth.api_key_options, replacements);
    }

    #[test]
    fn sync_preserves_sub2api_custom_key_prefix() {
        let mut auth = ProviderInput::default().auth;
        auth.api_key = "custom-sub2-key".to_string();
        auth.api_key_token_id = "21".to_string();
        let remote = option(ProviderProtocol::Sub2Api, "21", "custom-sub2-key", "Custom");

        sync_api_key_options(
            &mut auth,
            ProviderProtocol::Sub2Api,
            std::slice::from_ref(&remote),
            None,
        );

        assert_eq!(auth.api_key, "custom-sub2-key");
        assert_eq!(auth.api_key_options[0].key, "custom-sub2-key");
    }
}
