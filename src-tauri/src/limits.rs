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
pub const MAX_PROVIDER_REMARK_CHARS: usize = 160;
pub const MAX_API_KEY_REMARK_CHARS: usize = MAX_PROVIDER_REMARK_CHARS;
pub const MAX_LIVENESS_TIMEOUT_SECS: u64 = 600;
/// 设置项的可取值范围。超出范围一律夹到边界，而不是拒绝保存整份设置。
pub const MIN_UPDATE_CHECK_INTERVAL_HOURS: u64 = 1;
pub const MAX_UPDATE_CHECK_INTERVAL_HOURS: u64 = 168;
pub const MAX_NOTIFY_RETRY_COUNT: u64 = 3;
pub const MAX_NOTIFY_MERGE_WINDOW_SECS: u64 = 3_600;
pub const MAX_REFRESH_RETRY_COUNT: u64 = 3;
pub const MIN_LIVENESS_FAILURE_THRESHOLD: u64 = 1;
pub const MAX_LIVENESS_FAILURE_THRESHOLD: u64 = 10;
pub const MIN_LIVENESS_RECORD_LIMIT: u64 = 10;
pub const MAX_LIVENESS_RECORD_LIMIT: u64 = 500;
pub const MIN_CHECK_IN_RECORD_LIMIT: u64 = 30;
pub const MAX_CHECK_IN_RECORD_LIMIT: u64 = 3_650;
pub const MIN_HTTP_TIMEOUT_SECS: u64 = 5;
pub const MAX_HTTP_TIMEOUT_SECS: u64 = 300;
pub const MIN_LOG_RETENTION_DAYS: u64 = 1;
pub const MAX_LOG_RETENTION_DAYS: u64 = 90;
pub const MAX_PROXY_BYPASS_CHARS: usize = 2_048;
pub const MAX_HTTP_CLIENT_CACHE_ENTRIES: usize = 16;
pub const MAX_SITE_METADATA_CACHE_ENTRIES: usize = 128;
pub const MAX_SHIELD_CACHE_ENTRIES: usize = 64;
pub const MAX_AGENT_CLI_PATH_CHARS: usize = 4_096;
pub const MAX_SESSION_INDEX_DIRECTORY_CHARS: usize = 4_096;
pub const MIN_SESSION_INDEX_SIZE_MIB: u64 = 8;
pub const MAX_SESSION_INDEX_SIZE_MIB: u64 = 4_096;
pub const MAX_ANNOUNCEMENTS_PER_SOURCE: usize = 50;
pub const MAX_ANNOUNCEMENT_TITLE_CHARS: usize = 160;
/// 公告正文要能放下内嵌 base64 海报：一条 data URI 图片动辄几十万字符，
/// 20k 的上限会把 `<img src="data:…` 从中间截断，图片和后面的正文一起丢。
pub const MAX_ANNOUNCEMENT_CONTENT_CHARS: usize = 1_000_000;
pub const MAX_ANNOUNCEMENT_EXTRA_CHARS: usize = 2_000;

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
    let index_directory = settings
        .session_index_directory
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_SESSION_INDEX_DIRECTORY_CHARS)
        .collect::<String>();
    changed |= settings.session_index_directory != index_directory;
    settings.session_index_directory = index_directory;
    let index_size = settings
        .session_index_max_size_mib
        .clamp(MIN_SESSION_INDEX_SIZE_MIB, MAX_SESSION_INDEX_SIZE_MIB);
    changed |= settings.session_index_max_size_mib != index_size;
    settings.session_index_max_size_mib = index_size;
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
    changed |= clamp_numeric_settings(settings);
    changed
}

/// 数值型设置统一夹取，返回值表示是否被改动。
fn clamp_numeric_settings(settings: &mut AppSettings) -> bool {
    let mut changed = false;
    macro_rules! clamp {
        ($field:ident, $min:expr, $max:expr) => {{
            let bounded = settings.$field.clamp($min, $max);
            if settings.$field != bounded {
                settings.$field = bounded;
                changed = true;
            }
        }};
    }
    clamp!(
        update_check_interval_hours,
        MIN_UPDATE_CHECK_INTERVAL_HOURS,
        MAX_UPDATE_CHECK_INTERVAL_HOURS
    );
    clamp!(notify_retry_count, 0, MAX_NOTIFY_RETRY_COUNT);
    clamp!(notify_merge_window_secs, 0, MAX_NOTIFY_MERGE_WINDOW_SECS);
    clamp!(refresh_retry_count, 0, MAX_REFRESH_RETRY_COUNT);
    clamp!(
        liveness_failure_threshold,
        MIN_LIVENESS_FAILURE_THRESHOLD,
        MAX_LIVENESS_FAILURE_THRESHOLD
    );
    clamp!(
        liveness_record_limit,
        MIN_LIVENESS_RECORD_LIMIT,
        MAX_LIVENESS_RECORD_LIMIT
    );
    clamp!(
        check_in_record_limit,
        MIN_CHECK_IN_RECORD_LIMIT,
        MAX_CHECK_IN_RECORD_LIMIT
    );
    clamp!(
        http_timeout_secs,
        MIN_HTTP_TIMEOUT_SECS,
        MAX_HTTP_TIMEOUT_SECS
    );
    clamp!(
        log_retention_days,
        MIN_LOG_RETENTION_DAYS,
        MAX_LOG_RETENTION_DAYS
    );
    for (field, fallback) in [
        (&mut settings.notify_quiet_start, "22:00"),
        (&mut settings.notify_quiet_end, "08:00"),
    ] {
        let normalized = normalize_clock_time(field, fallback);
        changed |= *field != normalized;
        *field = normalized;
    }
    let bypass = settings
        .proxy_bypass
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_PROXY_BYPASS_CHARS)
        .collect::<String>();
    changed |= settings.proxy_bypass != bypass;
    settings.proxy_bypass = bypass;
    changed
}

/// 静默时段只接受 `HH:MM`；无法解析时退回默认值，避免整份设置因一个时间串失效。
fn normalize_clock_time(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    let Some((hour, minute)) = trimmed.split_once(':') else {
        return fallback.to_string();
    };
    if trimmed.len() != 5 {
        return fallback.to_string();
    }
    let (Ok(hour), Ok(minute)) = (hour.parse::<u32>(), minute.parse::<u32>()) else {
        return fallback.to_string();
    };
    if hour > 23 || minute > 59 {
        return fallback.to_string();
    }
    format!("{hour:02}:{minute:02}")
}

pub fn normalize_provider(provider: &mut Provider, settings: &AppSettings) -> bool {
    let mut changed = false;

    if provider.identity.protocol == crate::models::ProviderProtocol::OAuth {
        changed |= provider.liveness.use_global
            || provider.liveness.enabled
            || provider.automation.auto_shield
            || provider.quota.known
            || provider.quota.total_known;
        provider.liveness.use_global = false;
        provider.liveness.enabled = false;
        provider.automation.auto_shield = false;
        provider.quota.known = false;
        provider.quota.total_known = false;
        if let Some(oauth) = &provider.auth.oauth {
            changed |= provider.identity.base_url != oauth.vendor.base_url();
            provider.identity.base_url = oauth.vendor.base_url().into();
        }
    }
    if provider.identity.protocol == crate::models::ProviderProtocol::CliProxyApi {
        if let Ok(base_url) = crate::models::normalize_cpa_base_url(&provider.identity.base_url) {
            changed |= provider.identity.base_url != base_url;
            provider.identity.base_url = base_url;
        }
        changed |=
            provider.automation.auto_shield || provider.quota.known || provider.quota.total_known;
        provider.automation.auto_shield = false;
        provider.quota.known = false;
        provider.quota.total_known = false;
    }
    let remark = normalize_provider_remark(&provider.identity.remark);
    changed |= provider.identity.remark != remark;
    provider.identity.remark = remark;

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

    // 历史记录的保留条数由设置决定，加载时同步裁剪，避免设置调小后旧数据仍占着空间。
    let liveness_limit = settings.liveness_record_limit as usize;
    if provider.liveness.records.len() > liveness_limit {
        let keep_from = provider.liveness.records.len() - liveness_limit;
        provider.liveness.records.drain(0..keep_from);
        changed = true;
    }

    let check_in_limit = settings.check_in_record_limit as usize;
    if provider.automation.check_in_records.len() > check_in_limit {
        let keep_from = provider.automation.check_in_records.len() - check_in_limit;
        provider.automation.check_in_records.drain(0..keep_from);
        changed = true;
    }

    changed
}

pub fn normalize_app_data(data: &mut AppData) -> bool {
    let mut changed = normalize_settings(&mut data.settings);
    for provider in &mut data.providers {
        changed |= normalize_provider(provider, &data.settings);
    }
    for path in data
        .workspaces
        .iter_mut()
        .map(|workspace| &mut workspace.path)
        .chain(
            data.temporary_cli_preferences
                .iter_mut()
                .map(|preference| &mut preference.workspace_path),
        )
    {
        let normalized = crate::platform::paths::compatible_path(std::path::Path::new(path))
            .to_string_lossy()
            .into_owned();
        if *path != normalized {
            *path = normalized;
            changed = true;
        }
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

pub fn normalize_provider_remark(value: &str) -> String {
    normalize_local_remark(value, MAX_PROVIDER_REMARK_CHARS)
}

pub fn normalize_api_key_remark(value: &str) -> String {
    normalize_local_remark(value, MAX_API_KEY_REMARK_CHARS)
}

fn normalize_local_remark(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentCliKind, Provider, ProviderInput};

    #[cfg(windows)]
    #[test]
    fn saved_workspace_paths_and_launch_preferences_share_the_ordinary_spelling() {
        let mut data = AppData::default();
        data.providers.push(Provider::from_input(
            ProviderInput::default(),
            "provider-test".into(),
        ));
        data.workspaces.push(crate::models::Workspace {
            path: r"\\?\C:\Users\demo".into(),
            use_count: 7,
        });
        data.temporary_cli_preferences
            .push(crate::models::TemporaryCliPreference {
                provider_id: "provider-test".into(),
                workspace_path: r"\\?\C:\Users\demo".into(),
                ..Default::default()
            });

        assert!(normalize_app_data(&mut data));
        assert_eq!(data.workspaces[0].path, r"C:\Users\demo");
        assert_eq!(data.workspaces[0].use_count, 7);
        assert_eq!(
            data.temporary_cli_preferences[0].workspace_path,
            data.workspaces[0].path
        );
        assert!(!normalize_app_data(&mut data));
    }

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
    fn normalization_bounds_session_index_settings() {
        let mut settings = AppSettings {
            session_index_directory: format!(
                "  {}\n",
                "x".repeat(MAX_SESSION_INDEX_DIRECTORY_CHARS + 10)
            ),
            session_index_max_size_mib: MAX_SESSION_INDEX_SIZE_MIB + 100,
            ..AppSettings::default()
        };

        assert!(normalize_settings(&mut settings));
        assert_eq!(
            settings.session_index_directory.chars().count(),
            MAX_SESSION_INDEX_DIRECTORY_CHARS
        );
        assert!(!settings.session_index_directory.contains('\n'));
        assert_eq!(
            settings.session_index_max_size_mib,
            MAX_SESSION_INDEX_SIZE_MIB
        );
    }

    #[test]
    fn normalization_bounds_provider_runtime_collections() {
        let mut provider = Provider::from_input(ProviderInput::default(), "provider-1".to_string());
        provider.liveness.timeout = MAX_LIVENESS_TIMEOUT_SECS + 1;
        provider.capabilities.available_models = (0..MAX_MODELS_PER_PROVIDER + 10)
            .map(|index| format!("model-{index}"))
            .collect();
        provider.identity.site_logo = "x".repeat(MAX_SITE_LOGO_BYTES + 1);

        assert!(normalize_provider(&mut provider, &AppSettings::default()));
        assert_eq!(provider.liveness.timeout, MAX_LIVENESS_TIMEOUT_SECS);
        assert_eq!(
            provider.capabilities.available_models.len(),
            MAX_MODELS_PER_PROVIDER
        );
        assert!(provider.identity.site_logo.is_empty());
    }

    #[test]
    fn provider_remark_normalization_trims_controls_and_limits_characters() {
        let value = format!(
            "  \n主\u{0000}用{}\t  ",
            "备".repeat(MAX_PROVIDER_REMARK_CHARS)
        );

        let normalized = normalize_provider_remark(&value);

        assert_eq!(normalized.chars().count(), MAX_PROVIDER_REMARK_CHARS);
        assert!(normalized.starts_with("主用"));
        assert!(!normalized.chars().any(char::is_control));
    }

    #[test]
    fn api_key_remark_normalization_allows_empty_values_and_bounds_content() {
        assert!(normalize_api_key_remark(" \n\t ").is_empty());

        let normalized = normalize_api_key_remark(&format!(
            "  主\u{0000}用{}  ",
            "备".repeat(MAX_API_KEY_REMARK_CHARS)
        ));

        assert_eq!(normalized.chars().count(), MAX_API_KEY_REMARK_CHARS);
        assert!(normalized.starts_with("主用"));
        assert!(!normalized.chars().any(char::is_control));
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
