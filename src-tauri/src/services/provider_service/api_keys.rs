use crate::{
    adapters::protocol::ProtocolAdapter,
    limits,
    models::{
        is_full_api_key_value, normalize_api_key_for_protocol, Provider, ProviderApiKeyOption,
        ProviderAuth, ProviderProtocol,
    },
};
use tauri::Manager;

use super::{find_provider, MutationDecision, ProviderRequestContext, ProviderService};

impl<'a> ProviderService<'a> {
    pub fn local_api_keys(&self, id: String) -> Result<Vec<ProviderApiKeyOption>, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        Ok(provider.auth.api_key_options)
    }

    pub fn add_local_api_key(
        &self,
        id: String,
        key: String,
        remark: String,
    ) -> Result<Provider, String> {
        let provider_id = id.clone();
        self.mutate_decided(|data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|provider| provider.identity.id == provider_id)
                .ok_or_else(|| "中转站不存在".to_string())?;
            provider.add_named_api_key(&key, &remark)?;
            Ok(MutationDecision::changed(()))
        })?;
        find_provider(&self.snapshot(), &id)
    }

    pub fn set_local_api_key_remark(
        &self,
        id: String,
        local_id: String,
        remark: String,
    ) -> Result<Provider, String> {
        let provider_id = id.clone();
        self.mutate_decided(|data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|provider| provider.identity.id == provider_id)
                .ok_or_else(|| "中转站不存在".to_string())?;
            if provider.set_api_key_remark(&local_id, &remark)? {
                Ok(MutationDecision::changed(()))
            } else {
                Ok(MutationDecision::unchanged(()))
            }
        })?;
        find_provider(&self.snapshot(), &id)
    }

    pub fn set_default_local_api_key(
        &self,
        id: String,
        local_id: String,
    ) -> Result<Provider, String> {
        let provider_id = id.clone();
        self.mutate_decided(|data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|provider| provider.identity.id == provider_id)
                .ok_or_else(|| "中转站不存在".to_string())?;
            provider.set_default_api_key(&local_id)?;
            Ok(MutationDecision::changed(()))
        })?;
        find_provider(&self.snapshot(), &id)
    }

    pub fn remove_local_api_key(&self, id: String, local_id: String) -> Result<Provider, String> {
        let provider_id = id.clone();
        self.mutate_decided(|data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|provider| provider.identity.id == provider_id)
                .ok_or_else(|| "中转站不存在".to_string())?;
            provider.remove_local_api_key(&local_id)?;
            Ok(MutationDecision::changed(()))
        })?;
        find_provider(&self.snapshot(), &id)
    }

    pub async fn list_api_keys(&self, id: String) -> Result<Vec<ProviderApiKeyOption>, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
        let request_context = ProviderRequestContext::capture(&provider);
        let operation = ProtocolAdapter
            .list_api_keys(&data.settings, &provider)
            .await?;
        let options = operation.value;
        let persisted_provider = self
            .persist_operation_credentials(&request_context, &operation.credentials)
            .await?;
        let options_context = persisted_provider
            .as_ref()
            .map(ProviderRequestContext::capture)
            .unwrap_or(request_context);
        self.persist_api_key_options(&options_context, &options, None)
            .await?;
        let data = self.snapshot_async().await?;
        Ok(find_provider(&data, &id)?.auth.api_key_options)
    }

    pub async fn delete_api_key(
        &self,
        id: String,
        token_id: String,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
        let request_context = ProviderRequestContext::capture(&provider);
        let adapter = ProtocolAdapter;
        let deleted = adapter
            .delete_api_key(&data.settings, &provider, &token_id)
            .await?;
        let persisted_deleted = self
            .persist_operation_credentials(&request_context, &deleted.credentials)
            .await?;
        let provider = persisted_deleted
            .ok_or_else(|| "站点 Key 已删除，但本地配置已变更，请同步确认".to_string())?;
        let context = ProviderRequestContext::capture(&provider);
        let options = provider
            .auth
            .api_key_options
            .into_iter()
            .filter(|option| option.token_id != token_id)
            .collect::<Vec<_>>();
        self.persist_api_key_options(&context, &options, Some(&token_id))
            .await
            .map_err(|error| format!("站点 Key 已删除，但本地同步失败：{error}"))?;
        let data = self.snapshot_async().await?;
        Ok(find_provider(&data, &id)?.auth.api_key_options)
    }

    pub(super) async fn persist_api_key_options(
        &self,
        request_context: &ProviderRequestContext,
        options: &[ProviderApiKeyOption],
        removed_token_id: Option<&str>,
    ) -> Result<(), String> {
        let mutation_context = request_context.clone();
        let options = options.to_vec();
        let removed_token_id = removed_token_id.map(str::to_string);
        let persisted = self
            .mutate_decided_async(move |data| {
                if let Some(provider) = data
                    .providers
                    .iter_mut()
                    .find(|provider| mutation_context.matches(provider))
                {
                    let previous_auth = provider.auth.clone();
                    let changed = sync_api_key_options(
                        &mut provider.auth,
                        provider.identity.protocol,
                        &options,
                        removed_token_id.as_deref(),
                    );
                    if model_permissions_changed(&previous_auth, &provider.auth) {
                        provider.capabilities.clear_available_models();
                        provider.auth.credential_revision += 1;
                    }
                    Ok(if changed {
                        MutationDecision::changed(true)
                    } else {
                        MutationDecision::unchanged(true)
                    })
                } else {
                    Ok(MutationDecision::unchanged(false))
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

fn model_permissions_changed(previous: &ProviderAuth, current: &ProviderAuth) -> bool {
    if previous.api_key != current.api_key {
        return true;
    }
    if current.api_key.trim().is_empty() {
        return false;
    }
    let before = previous
        .api_key_options
        .iter()
        .find(|key| key.key == previous.api_key);
    let after = current
        .api_key_options
        .iter()
        .find(|key| key.key == current.api_key);
    match (before, after) {
        (Some(before), Some(after)) => {
            before.group != after.group
                || before.group_id != after.group_id
                || before.model_limits_enabled != after.model_limits_enabled
                || before.model_limits != after.model_limits
                || before.auto_groups != after.auto_groups
                || before.cross_group_retry != after.cross_group_retry
                || before.status != after.status
                || before.allow_ips != after.allow_ips
                || before.deny_ips != after.deny_ips
                || before.expired_time != after.expired_time
        }
        (None, None) => false,
        _ => !current.api_key.is_empty(),
    }
}

fn sync_api_key_options(
    auth: &mut ProviderAuth,
    protocol: ProviderProtocol,
    options: &[ProviderApiKeyOption],
    removed_token_id: Option<&str>,
) -> bool {
    let previous = auth.clone();
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

    // Remote list endpoints do not know about manually stored keys. Preserve
    // those entries across every metadata refresh instead of making them
    // disappear from the local vault.
    for previous in previous_options.iter().filter(|option| {
        option.token_id.trim().is_empty()
            && option.key_available
            && is_full_api_key_value(&option.key)
    }) {
        let exists = cached.iter().any(|option| {
            (!previous.local_id.is_empty() && option.local_id == previous.local_id)
                || option.key == previous.key
        });
        if !exists {
            cached.push(previous.clone().normalize_for_protocol(protocol));
        }
    }

    let selected = cached
        .iter()
        .find(|option| {
            !current_key.is_empty()
                && option.key == current_key
                && option.token_id != removed_token_id
                && option.key_available
        })
        .or_else(|| {
            cached.iter().find(|option| {
                !current_token_id.is_empty()
                    && option.token_id == current_token_id
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
        if let Some(replacement) = cached.iter().find(|option| {
            option.token_id != removed_token_id
                && option.key_available
                && is_full_api_key_value(&option.key)
        }) {
            auth.api_key = replacement.key.clone();
            auth.api_key_token_id = replacement.token_id.clone();
        } else {
            auth.api_key.clear();
            auth.api_key_token_id.clear();
        }
    } else if !current_key.is_empty() && !cached.iter().any(|option| option.key == current_key) {
        // The list endpoint is intentionally capped at 100 items. Keep a
        // previously revealed default key when it falls outside that window.
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
    auth != &previous
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;

    #[test]
    fn model_cache_changes_only_for_current_key_access_changes() {
        let mut previous = ProviderInput::default().auth;
        previous.api_key = "sk-first".into();
        previous.api_key_options = vec![
            option(ProviderProtocol::NewApi, "1", "sk-first", "First"),
            option(ProviderProtocol::NewApi, "2", "sk-second", "Second"),
        ];
        let mut refreshed = previous.clone();
        refreshed.api_key_options[0].name = "renamed".into();
        refreshed.api_key_options[0].remain_quota -= 1.0;
        refreshed.api_key_options[1].group = "different".into();
        assert!(!model_permissions_changed(&previous, &refreshed));
        refreshed.api_key_options[0].group = "different".into();
        assert!(model_permissions_changed(&previous, &refreshed));
        refreshed = previous.clone();
        refreshed.api_key_options[0].model_limits = vec!["restricted-model".into()];
        assert!(model_permissions_changed(&previous, &refreshed));
        previous.api_key.clear();
        refreshed.api_key.clear();
        previous.api_key_options[0].key.clear();
        refreshed.api_key_options[0].key.clear();
        assert!(!model_permissions_changed(&previous, &refreshed));
    }

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
    fn sync_reports_unchanged_when_cached_keys_are_identical() {
        let options = vec![option(ProviderProtocol::NewApi, "11", "sk-only", "Only")];
        let mut auth = ProviderInput::default().auth;
        assert!(sync_api_key_options(
            &mut auth,
            ProviderProtocol::NewApi,
            &options,
            None,
        ));

        assert!(!sync_api_key_options(
            &mut auth,
            ProviderProtocol::NewApi,
            &options,
            None,
        ));
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
            local_name: "远端不应覆盖".to_string(),
            key: "sk-secret".to_string(),
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
        assert_eq!(auth.api_key_options[0].local_name, "");
        assert!(auth.api_key_options[0].key_available);
    }

    #[test]
    fn sync_preserves_local_identity_and_remark_when_remote_token_id_changes() {
        let mut auth = ProviderInput::default().auth;
        let mut cached = option(
            ProviderProtocol::NewApi,
            "old-token-id",
            "sk-stable-secret",
            "Remote name",
        )
        .normalize_for_protocol(ProviderProtocol::NewApi);
        cached.local_name = "我的备用 Key".to_string();
        let stable_local_id = cached.local_id.clone();
        let masked_key = cached.masked_key.clone();
        auth.api_key = cached.key.clone();
        auth.api_key_token_id = cached.token_id.clone();
        auth.api_key_options = vec![cached];

        let remote = ProviderApiKeyOption {
            name: "Remote name".to_string(),
            local_name: "远端不应覆盖".to_string(),
            masked_key,
            token_id: "new-token-id".to_string(),
            remain_quota: 84.0,
            ..ProviderApiKeyOption::default()
        }
        .normalize_for_protocol(ProviderProtocol::NewApi);

        sync_api_key_options(
            &mut auth,
            ProviderProtocol::NewApi,
            std::slice::from_ref(&remote),
            None,
        );

        let refreshed = auth
            .api_key_options
            .iter()
            .find(|option| option.token_id == "new-token-id")
            .expect("refreshed key should be present");
        assert_eq!(refreshed.local_id, stable_local_id);
        assert_eq!(refreshed.local_name, "我的备用 Key");
        assert_eq!(refreshed.key, "sk-stable-secret");
        assert!(refreshed.key_available);
        assert_eq!(refreshed.remain_quota, 84.0);
    }

    #[test]
    fn sync_prefers_the_configured_key_over_a_stale_token_id() {
        let mut auth = ProviderInput::default().auth;
        auth.api_key = "sk-first".to_string();
        auth.api_key_token_id = "token-second".to_string();
        let options = vec![
            option(ProviderProtocol::NewApi, "token-first", "sk-first", "First"),
            option(
                ProviderProtocol::NewApi,
                "token-second",
                "sk-second",
                "Second",
            ),
        ];

        sync_api_key_options(&mut auth, ProviderProtocol::NewApi, &options, None);

        assert_eq!(auth.api_key, "sk-first");
        assert_eq!(auth.api_key_token_id, "token-first");
    }

    #[test]
    fn sync_preserves_default_key_outside_the_first_hundred_items() {
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
    fn sync_selects_the_next_usable_key_when_the_default_was_removed() {
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

        assert_eq!(auth.api_key, "sk-replacement");
        assert_eq!(auth.api_key_token_id, "12");
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
