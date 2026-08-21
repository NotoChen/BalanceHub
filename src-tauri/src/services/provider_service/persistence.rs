use crate::{
    limits,
    models::{
        normalize_provider_endpoint, provider_duplicate_kind, AppData, AppDataTransferResult,
        AppSettings, Provider, ProviderDuplicateKind, ProviderInput, ProviderRemovalResult,
        ProviderSaveConflict, ProviderSaveOptions, ProviderSaveResult,
    },
    state::AppState,
    storage,
    util::unix_millis as current_timestamp_millis,
};
use std::path::PathBuf;
use tauri::Manager;

use super::{find_provider, MutationDecision, ProviderService};

impl ProviderService<'_> {
    pub fn save_provider(
        &self,
        input: ProviderInput,
        options: ProviderSaveOptions,
    ) -> Result<ProviderSaveResult, String> {
        let (saved, saved_provider_id, conflict) = self.mutate_decided(|data| {
            let separate_retry = options.create_separate_from_provider_id.is_some();
            let editing_existing = input.id.as_deref().is_some_and(|id| {
                data.providers
                    .iter()
                    .any(|provider| provider.identity.id == id)
            });
            let selected_conflict_id = options
                .create_separate_from_provider_id
                .as_deref()
                .or(options.merge_api_key_into_provider_id.as_deref());
            let endpoint_changed = input.id.as_deref().is_some_and(|id| {
                data.providers
                    .iter()
                    .find(|provider| provider.identity.id == id)
                    .is_some_and(|provider| {
                        normalize_provider_endpoint(&provider.identity.base_url)
                            != normalize_provider_endpoint(&input.identity.base_url)
                    })
            });
            let conflict = provider_save_conflict(
                &data.providers,
                &input,
                !editing_existing || endpoint_changed || separate_retry,
                separate_retry,
                selected_conflict_id,
            );
            let force_separate_provider = separate_retry;

            if let Some(conflict) = conflict {
                if conflict.kind == ProviderDuplicateKind::UrlDifferentApiKey
                    && options.merge_api_key_into_provider_id.as_deref()
                        == Some(conflict.existing_provider_id.as_str())
                {
                    let provider = data
                        .providers
                        .iter_mut()
                        .find(|provider| provider.identity.id == conflict.existing_provider_id)
                        .ok_or_else(|| "目标中转站已不存在，请重新保存".to_string())?;
                    provider.add_api_key(&input.auth.api_key)?;
                    return Ok(MutationDecision::changed((
                        true,
                        Some(conflict.existing_provider_id.clone()),
                        None,
                    )));
                }

                if conflict.kind == ProviderDuplicateKind::UrlDifferentApiKey
                    && options.create_separate_from_provider_id.as_deref()
                        == Some(conflict.existing_provider_id.as_str())
                {
                    // The user explicitly chose a separate card for this endpoint conflict.
                    // Exact API Key conflicts are selected before URL-only conflicts below,
                    // so this cannot bypass an identical Key stored on another card.
                } else if matches!(
                    conflict.kind,
                    ProviderDuplicateKind::Account | ProviderDuplicateKind::ApiKey
                ) && options.overwrite_provider_id.as_deref()
                    == Some(conflict.existing_provider_id.as_str())
                {
                    let mut overwrite_input = input.clone();
                    overwrite_input.id = Some(conflict.existing_provider_id.clone());
                    let provider = data
                        .providers
                        .iter_mut()
                        .find(|provider| provider.identity.id == conflict.existing_provider_id)
                        .ok_or_else(|| "目标中转站已不存在，请重新保存".to_string())?;
                    provider.apply_input(overwrite_input);
                    return Ok(MutationDecision::changed((
                        true,
                        Some(conflict.existing_provider_id.clone()),
                        None,
                    )));
                } else {
                    return Ok(MutationDecision::unchanged((false, None, Some(conflict))));
                }
            }
            let mut persisted_input = input;
            if force_separate_provider {
                persisted_input.id = None;
            }
            let saved_provider_id = if let Some(id) = persisted_input.id.clone() {
                if let Some(provider) = data
                    .providers
                    .iter_mut()
                    .find(|provider| provider.identity.id == id)
                {
                    provider.apply_input(persisted_input);
                } else {
                    if data.providers.len() >= limits::MAX_PROVIDERS {
                        return Err(format!(
                            "中转站数量已达到上限（{} 个）",
                            limits::MAX_PROVIDERS
                        ));
                    }
                    data.providers
                        .push(Provider::from_input(persisted_input, id.clone()));
                }
                Some(id)
            } else {
                if data.providers.len() >= limits::MAX_PROVIDERS {
                    return Err(format!(
                        "中转站数量已达到上限（{} 个）",
                        limits::MAX_PROVIDERS
                    ));
                }
                let id = format!("provider-{}", current_timestamp_millis());
                data.providers
                    .push(Provider::from_input(persisted_input, id.clone()));
                Some(id)
            };
            Ok(MutationDecision::changed((true, saved_provider_id, None)))
        })?;

        let provider = saved_provider_id
            .as_deref()
            .map(|id| find_provider(&self.snapshot(), id))
            .transpose()?;
        Ok(ProviderSaveResult {
            saved,
            provider,
            conflict,
        })
    }

    pub fn remove_provider(&self, id: String) -> Result<ProviderRemovalResult, String> {
        let removed_id = id.clone();
        let ((), revision) = self.mutate_decided_with_revision(|data| {
            let provider_count = data.providers.len();
            let preference_count = data.temporary_cli_preferences.len();
            data.providers.retain(|provider| provider.identity.id != id);
            data.temporary_cli_preferences
                .retain(|preference| preference.provider_id != id);
            let changed = provider_count != data.providers.len()
                || preference_count != data.temporary_cli_preferences.len();
            Ok(if changed {
                MutationDecision::changed(())
            } else {
                MutationDecision::unchanged(())
            })
        })?;
        Ok(ProviderRemovalResult {
            id: removed_id,
            revision,
        })
    }

    pub fn reorder_providers(&self, ids: Vec<String>) -> Result<Vec<String>, String> {
        self.mutate_decided(|data| {
            let previous_order = data
                .providers
                .iter()
                .map(|provider| provider.identity.id.clone())
                .collect::<Vec<_>>();
            let order: std::collections::HashMap<&str, usize> = ids
                .iter()
                .enumerate()
                .map(|(index, id)| (id.as_str(), index))
                .collect();
            let fallback = ids.len();
            data.providers.sort_by_key(|provider| {
                order
                    .get(provider.identity.id.as_str())
                    .copied()
                    .unwrap_or(fallback)
            });
            let final_order = data
                .providers
                .iter()
                .map(|provider| provider.identity.id.clone())
                .collect::<Vec<_>>();
            let changed = previous_order != final_order;
            Ok(if changed {
                MutationDecision::changed(final_order)
            } else {
                MutationDecision::unchanged(final_order)
            })
        })
    }

    pub fn save_settings(&self, mut settings: AppSettings) -> Result<AppSettings, String> {
        let _ = limits::normalize_settings(&mut settings);
        if settings.notification_channels.len() > limits::MAX_NOTIFICATION_CHANNELS {
            return Err(format!(
                "通知渠道数量超过上限（最多 {} 个）",
                limits::MAX_NOTIFICATION_CHANNELS
            ));
        }
        let (previous, saved) = self.mutate(|data| {
            let previous = std::mem::replace(&mut data.settings, settings);
            (previous, data.settings.clone())
        })?;
        crate::services::cli_sessions::reconfigure_index(self.app, &previous, &saved);
        Ok(saved)
    }

    pub fn export_app_data(&self, path: String) -> Result<AppDataTransferResult, String> {
        self.ensure_storage_ready()?;
        let target = PathBuf::from(path);
        let data = self.snapshot();
        storage::export_app_data(&target, &data)?;
        Ok(AppDataTransferResult {
            path: target.display().to_string(),
            schema_version: data.schema_version,
            provider_count: data.providers.len(),
        })
    }

    pub fn import_app_data(
        &self,
        path: String,
    ) -> Result<(AppData, AppDataTransferResult), String> {
        let source = PathBuf::from(path);
        let state = self.app.state::<AppState>();
        let _transaction = state
            .mutation_gate
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut data = storage::import_app_data(self.app, &source)?;
        let revision = state.next_revision();
        data.revision = revision;
        for provider in &mut data.providers {
            provider.revision = revision;
        }
        let previous_data = {
            let mut current = state.data.write().unwrap_or_else(|err| err.into_inner());
            std::mem::replace(&mut *current, data.clone())
        };
        state.clear_load_error();
        drop(previous_data);
        let result = AppDataTransferResult {
            path: source.display().to_string(),
            schema_version: data.schema_version,
            provider_count: data.providers.len(),
        };
        Ok((data, result))
    }
}

fn provider_save_conflict(
    providers: &[Provider],
    input: &ProviderInput,
    include_url_conflicts: bool,
    check_self_exact_conflict: bool,
    preferred_url_conflict_id: Option<&str>,
) -> Option<ProviderSaveConflict> {
    let mut url_conflict = None;
    let mut preferred_url_conflict = None;
    for provider in providers {
        let is_self = Some(provider.identity.id.as_str()) == input.id.as_deref();
        let Some(kind) = provider_duplicate_kind(provider, input) else {
            continue;
        };
        let conflict = ProviderSaveConflict {
            kind,
            existing_provider_id: provider.identity.id.clone(),
            existing_provider_name: provider.display_label(),
        };
        // An edit of an existing card may legitimately keep the same endpoint
        // while another card uses a different key. In a separate-card retry,
        // the current card must still participate in exact-key checks, but it
        // must not become the URL-only choice shown to the user.
        if is_self {
            if kind == ProviderDuplicateKind::UrlDifferentApiKey {
                continue;
            }
            if check_self_exact_conflict {
                return Some(conflict);
            }
            continue;
        }
        if kind == ProviderDuplicateKind::UrlDifferentApiKey {
            if include_url_conflicts {
                if preferred_url_conflict_id == Some(provider.identity.id.as_str()) {
                    preferred_url_conflict = Some(conflict);
                } else {
                    url_conflict.get_or_insert(conflict);
                }
            }
        } else {
            return Some(conflict);
        }
    }
    preferred_url_conflict.or(url_conflict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthMode, ProviderProtocol};

    fn api_key_provider(id: &str, key: &str) -> Provider {
        let mut input = ProviderInput::default();
        input.identity.name = id.to_string();
        input.identity.protocol = ProviderProtocol::Api;
        input.identity.base_url = "http://anyrouter.top".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = key.to_string();
        Provider::from_input(input, id.to_string())
    }

    #[test]
    fn exact_api_key_conflict_takes_priority_over_same_url_conflict() {
        let providers = vec![
            api_key_provider("provider-a", "key-a"),
            api_key_provider("provider-b", "key-b"),
        ];
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Api;
        input.identity.base_url = "HTTP://ANYROUTER.TOP/".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "key-b".to_string();

        let conflict = provider_save_conflict(&providers, &input, true, false, None)
            .expect("conflict expected");
        assert_eq!(conflict.kind, ProviderDuplicateKind::ApiKey);
        assert_eq!(conflict.existing_provider_id, "provider-b");
    }

    #[test]
    fn different_api_key_reports_a_same_url_conflict() {
        let providers = vec![api_key_provider("provider-a", "key-a")];
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Api;
        input.identity.base_url = "http://anyrouter.top/".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "key-b".to_string();

        let conflict = provider_save_conflict(&providers, &input, true, false, None)
            .expect("conflict expected");
        assert_eq!(conflict.kind, ProviderDuplicateKind::UrlDifferentApiKey);
        assert_eq!(conflict.existing_provider_id, "provider-a");
    }

    #[test]
    fn protocol_redetection_still_reports_the_existing_api_key_card() {
        let providers = vec![api_key_provider("provider-a", "key-a")];
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Sub2Api;
        input.identity.base_url = "http://anyrouter.top/".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "key-b".to_string();

        let conflict = provider_save_conflict(&providers, &input, true, false, None)
            .expect("same endpoint conflict expected despite protocol drift");
        assert_eq!(conflict.kind, ProviderDuplicateKind::UrlDifferentApiKey);
        assert_eq!(conflict.existing_provider_id, "provider-a");
    }

    #[test]
    fn exact_key_in_merged_key_list_still_blocks_a_separate_card() {
        let mut first = api_key_provider("provider-a", "key-a");
        first.add_api_key("key-b").expect("key should merge");
        let providers = vec![first, api_key_provider("provider-b", "key-c")];
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Api;
        input.identity.base_url = "http://anyrouter.top".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "key-b".to_string();

        let conflict = provider_save_conflict(&providers, &input, true, false, None)
            .expect("conflict expected");
        assert_eq!(conflict.kind, ProviderDuplicateKind::ApiKey);
        assert_eq!(conflict.existing_provider_id, "provider-a");
    }

    #[test]
    fn editing_one_api_key_card_is_not_blocked_by_another_key_on_the_same_url() {
        let providers = vec![
            api_key_provider("provider-a", "key-a"),
            api_key_provider("provider-b", "key-b"),
        ];
        let mut input = ProviderInput {
            id: Some("provider-a".to_string()),
            ..ProviderInput::default()
        };
        input.identity.protocol = ProviderProtocol::Api;
        input.identity.base_url = "http://anyrouter.top".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "key-a".to_string();

        assert!(provider_save_conflict(&providers, &input, false, false, None).is_none());
    }

    #[test]
    fn separate_card_retry_still_checks_the_original_card_for_exact_key() {
        let providers = vec![
            api_key_provider("provider-a", "key-a"),
            api_key_provider("provider-b", "key-b"),
        ];
        let mut input = ProviderInput {
            id: Some("provider-a".to_string()),
            ..ProviderInput::default()
        };
        input.identity.protocol = ProviderProtocol::Api;
        input.identity.base_url = "http://anyrouter.top".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "key-a".to_string();

        let conflict = provider_save_conflict(&providers, &input, true, true, None)
            .expect("exact conflict expected");
        assert_eq!(conflict.kind, ProviderDuplicateKind::ApiKey);
        assert_eq!(conflict.existing_provider_id, "provider-a");
    }

    #[test]
    fn preferred_url_conflict_is_used_when_multiple_cards_share_an_endpoint() {
        let providers = vec![
            api_key_provider("provider-a", "key-a"),
            api_key_provider("provider-b", "key-b"),
        ];
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Api;
        input.identity.base_url = "http://anyrouter.top".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "key-c".to_string();

        let conflict = provider_save_conflict(&providers, &input, true, false, Some("provider-b"))
            .expect("conflict expected");
        assert_eq!(conflict.existing_provider_id, "provider-b");
    }
}
