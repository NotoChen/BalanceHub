use crate::models::{AppData, AppSettings, Provider};

pub const MAX_APP_DATA_FILE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_CLI_CONFIG_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CLI_RUNTIME_FILE_BYTES: usize = 256 * 1024;
pub const MAX_LIVENESS_OUTPUT_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_COMMAND_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SYSTEM_COMMAND_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_HTTP_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// Automatic shield replay keeps a bounded in-memory request clone. Larger or
/// streaming bodies must be retried by the caller after verification.
pub const MAX_HTTP_REPLAY_BODY_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_WEBHOOK_RESPONSE_BYTES: usize = 1024 * 1024;
pub const MAX_UPDATE_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_PROVIDERS: usize = 200;
pub const MAX_MODELS_PER_PROVIDER: usize = 2_000;
pub const MAX_NOTIFICATION_CHANNELS: usize = 32;
pub const MAX_API_KEYS_PER_PROVIDER: usize = 100;
pub const MAX_BACKUP_URLS_PER_PROVIDER: usize = 16;
pub const MAX_LIVENESS_PROMPTS: usize = 256;
pub const MAX_PLACEHOLDER_POOLS: usize = 64;
pub const MAX_PLACEHOLDER_VALUES: usize = 256;
pub const MAX_WORKSPACES: usize = 30;
pub const MAX_SITE_LOGO_BYTES: usize = 256 * 1024;
pub const MAX_LIVENESS_TIMEOUT_SECS: u64 = 600;
pub const MAX_LIVENESS_RECORDS: usize = 40;
pub const MAX_CHECK_IN_RECORDS: usize = 730;
pub const MAX_HTTP_CLIENT_CACHE_ENTRIES: usize = 16;
pub const MAX_SITE_METADATA_CACHE_ENTRIES: usize = 128;
pub const MAX_SHIELD_CACHE_ENTRIES: usize = 64;
pub const MAX_AGENT_CLI_PATH_CHARS: usize = 4_096;
pub const MAX_ANNOUNCEMENTS_PER_SOURCE: usize = 50;
pub const MAX_ANNOUNCEMENT_TITLE_CHARS: usize = 160;
pub const MAX_ANNOUNCEMENT_CONTENT_CHARS: usize = 20_000;

pub fn normalize_settings(settings: &mut AppSettings) -> bool {
    let mut changed = false;
    for path in settings.agent_cli_paths.values_mut() {
        let normalized = path
            .trim()
            .chars()
            .take(MAX_AGENT_CLI_PATH_CHARS)
            .collect::<String>();
        changed |= *path != normalized;
        *path = normalized;
    }
    let path_count = settings.agent_cli_paths.len();
    settings.agent_cli_paths.retain(|_, path| !path.is_empty());
    changed |= path_count != settings.agent_cli_paths.len();
    let timeout = settings
        .liveness_timeout
        .clamp(10, MAX_LIVENESS_TIMEOUT_SECS);
    changed |= settings.liveness_timeout != timeout;
    settings.liveness_timeout = timeout;
    let prompt_count = settings.liveness_prompt_library.len();
    settings
        .liveness_prompt_library
        .truncate(MAX_LIVENESS_PROMPTS);
    changed |= prompt_count != settings.liveness_prompt_library.len();
    let pool_count = settings.liveness_placeholder_pools.len();
    settings
        .liveness_placeholder_pools
        .truncate(MAX_PLACEHOLDER_POOLS);
    changed |= pool_count != settings.liveness_placeholder_pools.len();
    for pool in &mut settings.liveness_placeholder_pools {
        let value_count = pool.values.len();
        pool.values.truncate(MAX_PLACEHOLDER_VALUES);
        changed |= value_count != pool.values.len();
    }
    changed
}

pub fn normalize_provider(provider: &mut Provider) -> bool {
    let mut changed = false;

    let timeout = provider
        .liveness
        .timeout
        .clamp(10, MAX_LIVENESS_TIMEOUT_SECS);
    if provider.liveness.timeout != timeout {
        provider.liveness.timeout = timeout;
        changed = true;
    }

    if provider.capabilities.available_models.len() > MAX_MODELS_PER_PROVIDER {
        provider
            .capabilities
            .available_models
            .truncate(MAX_MODELS_PER_PROVIDER);
        changed = true;
    }

    if provider.auth.api_key_options.len() > MAX_API_KEYS_PER_PROVIDER {
        provider
            .auth
            .api_key_options
            .truncate(MAX_API_KEYS_PER_PROVIDER);
        changed = true;
    }

    if provider.identity.backup_urls.len() > MAX_BACKUP_URLS_PER_PROVIDER {
        provider
            .identity
            .backup_urls
            .truncate(MAX_BACKUP_URLS_PER_PROVIDER);
        changed = true;
    }

    if provider.notification.channel_ids.len() > MAX_NOTIFICATION_CHANNELS {
        provider
            .notification
            .channel_ids
            .truncate(MAX_NOTIFICATION_CHANNELS);
        changed = true;
    }

    if provider.identity.site_logo.len() > MAX_SITE_LOGO_BYTES {
        provider.identity.site_logo.clear();
        changed = true;
    }

    if provider.liveness.records.len() > MAX_LIVENESS_RECORDS {
        let keep_from = provider.liveness.records.len() - MAX_LIVENESS_RECORDS;
        provider.liveness.records.drain(0..keep_from);
        changed = true;
    }

    if provider.automation.check_in_records.len() > MAX_CHECK_IN_RECORDS {
        let keep_from = provider.automation.check_in_records.len() - MAX_CHECK_IN_RECORDS;
        provider.automation.check_in_records.drain(0..keep_from);
        changed = true;
    }

    changed
}

pub fn normalize_app_data(data: &mut AppData) -> bool {
    let mut changed = normalize_settings(&mut data.settings);
    for provider in &mut data.providers {
        changed |= normalize_provider(provider);
    }
    if data.workspaces.len() > MAX_WORKSPACES {
        data.workspaces.truncate(MAX_WORKSPACES);
        changed = true;
    }
    let provider_ids = data
        .providers
        .iter()
        .map(|provider| provider.identity.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let previous_preferences = data.temporary_cli_preferences.len();
    data.temporary_cli_preferences
        .retain(|preference| provider_ids.contains(&preference.provider_id));
    data.temporary_cli_preferences.truncate(MAX_PROVIDERS);
    changed |= data.temporary_cli_preferences.len() != previous_preferences;
    changed
}

pub fn validate_app_data_limits(data: &AppData) -> Result<(), String> {
    if data.providers.len() > MAX_PROVIDERS {
        return Err(format!(
            "中转站数量超过上限：最多支持 {MAX_PROVIDERS} 个，当前为 {} 个",
            data.providers.len()
        ));
    }
    if data.settings.notification_channels.len() > MAX_NOTIFICATION_CHANNELS {
        return Err(format!(
            "通知渠道数量超过上限：最多支持 {MAX_NOTIFICATION_CHANNELS} 个，当前为 {} 个",
            data.settings.notification_channels.len()
        ));
    }
    Ok(())
}

pub fn truncate_models(models: &mut Vec<String>) {
    if models.len() > MAX_MODELS_PER_PROVIDER {
        models.truncate(MAX_MODELS_PER_PROVIDER);
    }
}

pub fn site_logo_allowed(value: &str) -> bool {
    value.len() <= MAX_SITE_LOGO_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentCliKind, Provider, ProviderInput};

    #[test]
    fn normalization_bounds_and_trims_agent_cli_paths() {
        let mut settings = AppSettings::default();
        settings.agent_cli_paths.insert(
            AgentCliKind::Codex,
            format!("  {}  ", "x".repeat(MAX_AGENT_CLI_PATH_CHARS + 10)),
        );
        settings
            .agent_cli_paths
            .insert(AgentCliKind::ClaudeCode, "   ".to_string());

        assert!(normalize_settings(&mut settings));
        assert_eq!(
            settings.agent_cli_path(AgentCliKind::Codex).chars().count(),
            MAX_AGENT_CLI_PATH_CHARS
        );
        assert!(!settings
            .agent_cli_paths
            .contains_key(&AgentCliKind::ClaudeCode));
    }

    #[test]
    fn normalization_bounds_provider_runtime_collections() {
        let mut provider = Provider::from_input(ProviderInput::default(), "provider-1".to_string());
        provider.liveness.timeout = MAX_LIVENESS_TIMEOUT_SECS + 1;
        provider.capabilities.available_models = (0..MAX_MODELS_PER_PROVIDER + 10)
            .map(|index| format!("model-{index}"))
            .collect();
        provider.identity.site_logo = "x".repeat(MAX_SITE_LOGO_BYTES + 1);

        assert!(normalize_provider(&mut provider));
        assert_eq!(provider.liveness.timeout, MAX_LIVENESS_TIMEOUT_SECS);
        assert_eq!(
            provider.capabilities.available_models.len(),
            MAX_MODELS_PER_PROVIDER
        );
        assert!(provider.identity.site_logo.is_empty());
    }

    #[test]
    fn validation_rejects_unbounded_top_level_collections() {
        let data = AppData {
            providers: (0..MAX_PROVIDERS + 1)
                .map(|index| {
                    Provider::from_input(ProviderInput::default(), format!("provider-{index}"))
                })
                .collect(),
            ..AppData::default()
        };

        let error = validate_app_data_limits(&data).expect_err("provider limit should apply");
        assert!(error.contains("中转站数量超过上限"));
    }
}
