use crate::{
    limits,
    models::{
        provider_duplicate_kind, AppData, AppDataTransferResult, AppSettings, Provider,
        ProviderDuplicateKind, ProviderInput, ProviderRemovalResult, ProviderSaveConflict,
        ProviderSaveOptions, ProviderSaveResult,
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
            let conflict = data.providers.iter().find_map(|provider| {
                if Some(provider.identity.id.as_str()) == input.id.as_deref() {
                    return None;
                }
                provider_duplicate_kind(provider, &input).map(|kind| ProviderSaveConflict {
                    kind,
                    existing_provider_id: provider.identity.id.clone(),
                    existing_provider_name: provider.identity.name.clone(),
                })
            });

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

                if matches!(
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
                }

                return Ok(MutationDecision::unchanged((false, None, Some(conflict))));
            }
            let saved_provider_id = if let Some(id) = input.id.clone() {
                if let Some(provider) = data
                    .providers
                    .iter_mut()
                    .find(|provider| provider.identity.id == id)
                {
                    provider.apply_input(input);
                } else {
                    if data.providers.len() >= limits::MAX_PROVIDERS {
                        return Err(format!(
                            "中转站数量已达到上限（{} 个）",
                            limits::MAX_PROVIDERS
                        ));
                    }
                    data.providers.push(Provider::from_input(input, id.clone()));
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
                data.providers.push(Provider::from_input(input, id.clone()));
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
        self.mutate(|data| {
            data.settings = settings;
            data.settings.clone()
        })
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
