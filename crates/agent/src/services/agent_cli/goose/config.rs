use super::super::config_support::{
    cli_config_target, config_error, json_array_member, json_object_member, latest_modified_at,
    match_provider_key, merge_json_config, normalize_endpoint, oauth_snapshot, plain_secret,
    provider_display_name, read_stable_optional, relay_snapshot, yaml_source, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";

/// Goose's config folder: `%APPDATA%\Block\goose\config` on Windows, else
/// `$XDG_CONFIG_HOME/goose` (or `~/.config/goose`).
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(app_data) = configured_path("APPDATA").filter(|path| path.is_absolute()) {
            return app_data.join("Block/goose/config");
        }
    }
    configured_path("XDG_CONFIG_HOME")
        .filter(|path| path.is_absolute())
        .map(|path| path.join("goose"))
        .unwrap_or_else(|| home.join(".config/goose"))
}

/// Goose's data folder. `GOOSE_PATH_ROOT` moves every Goose directory below
/// one root; otherwise the platform data directory is used.
pub(super) fn data_dir_for_home(home: &Path) -> PathBuf {
    if let Some(root) = configured_path("GOOSE_PATH_ROOT").filter(|path| path.is_absolute()) {
        return root.join("data");
    }
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/Block/goose")
    }
    #[cfg(target_os = "windows")]
    {
        configured_path("APPDATA")
            .filter(|path| path.is_absolute())
            .map(|path| path.join("Block/goose/data"))
            .unwrap_or_else(|| home.join("AppData/Roaming/Block/goose/data"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        configured_path("XDG_DATA_HOME")
            .filter(|path| path.is_absolute())
            .map(|path| path.join("goose"))
            .unwrap_or_else(|| home.join(".local/share/goose"))
    }
}

pub(super) fn data_dir() -> Option<PathBuf> {
    Some(data_dir_for_home(&user_home()?))
}

pub(super) fn sessions_db_path() -> Option<PathBuf> {
    Some(data_dir()?.join("sessions/sessions.db"))
}

pub(super) fn config_path(root: &Path) -> PathBuf {
    root.join("config.yaml")
}

pub(super) fn provider_path(root: &Path) -> PathBuf {
    root.join("custom_providers").join(format!("{PROVIDER_ID}.json"))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = user_home() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let root = config_dir_for_home(&home);
    let config = match read_stable_optional(&config_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Goose 配置失败"),
    };
    let provider_file = match read_stable_optional(&provider_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Goose Provider 配置失败"),
    };
    let modified_at = latest_modified_at([config.as_ref(), provider_file.as_ref()]);
    let (Some(config), Some(provider_file)) = (config, provider_file) else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&config.text, &provider_file.text) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Goose 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let root = config_dir_for_home(&home);
    let config_path = config_path(&root);
    let provider_path = provider_path(&root);
    let config_before = read_stable_optional(&config_path)?.map(|file| file.text);
    let provider_before = read_stable_optional(&provider_path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Goose".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let config_after = merge_config(
                config_before.as_deref().unwrap_or_default(),
                model,
            )?;
            let provider_after = merge_provider(
                provider_before.as_deref().unwrap_or_default(),
                provider.identity.name.as_str(),
                model,
                &base_url,
                &api_key,
            )?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: config_path,
                    before: config_before,
                    after: config_after,
                },
                ProviderConfigurationCandidate {
                    path: provider_path,
                    before: provider_before,
                    after: provider_after,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err("Goose 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(config: &str, provider: &str) -> Result<Option<(String, String)>, ()> {
    if config.trim().is_empty() || provider.trim().is_empty() {
        return Ok(None);
    }
    let root = yaml_source::value(config).map_err(|_| ())?;
    let active = root
        .get("active_provider")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            root.get("GOOSE_PROVIDER")
                .and_then(JsonValue::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        });
    if active != Some(PROVIDER_ID) {
        return Ok(None);
    }
    let model = root
        .pointer("/providers/balancehub/model")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            root.get("GOOSE_MODEL")
                .and_then(JsonValue::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        });
    if model.is_none() {
        return Ok(None);
    }
    let provider = super::super::config_support::parse_jsonc_document(provider).map_err(|_| ())?;
    let base_url = provider
        .get("base_url")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let api_key = provider
        .pointer("/headers/Authorization")
        .and_then(JsonValue::as_str)
        .and_then(|value| {
            value
                .trim()
                .strip_prefix("Bearer ")
                .or_else(|| value.trim().strip_prefix("bearer "))
        })
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

fn merge_config(source: &str, model: &str) -> Result<String, String> {
    yaml_source::edit(source, "Goose 配置", |root| {
        root.set("active_provider", PROVIDER_ID);
        let providers = yaml_source::mapping_member(root, "providers", "Goose 配置")?;
        let entry = yaml_source::mapping_member(&providers, PROVIDER_ID, "Goose 配置")?;
        entry.set("enabled", true);
        entry.set("model", model);
        entry.set("configured", true);
        // Old Goose reads the flat keys and new Goose migrates them away; both
        // are written so the switch works without guessing the file's layout.
        root.set("GOOSE_PROVIDER", PROVIDER_ID);
        root.set("GOOSE_MODEL", model);
        Ok(())
    })
}

fn merge_provider(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let base_url = super::super::config_support::openai_base_url(base_url);
    merge_json_config(source, "Goose Provider 配置", |root| {
        root.insert(
            "name".into(),
            JsonValue::String(PROVIDER_ID.to_string()),
        );
        root.insert("engine".into(), JsonValue::String("openai".into()));
        if root
            .get("display_name")
            .and_then(JsonValue::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            root.insert(
                "display_name".into(),
                JsonValue::String(provider_display_name(provider_name)),
            );
        }
        root.insert(
            "description".into(),
            JsonValue::String("BalanceHub 中转站".into()),
        );
        root.insert("api_key_env".into(), JsonValue::String(String::new()));
        root.insert("base_url".into(), JsonValue::String(base_url));
        let headers = json_object_member(root, "headers", "Goose Provider 配置")?;
        headers.insert(
            "Authorization".into(),
            JsonValue::String(format!("Bearer {api_key}")),
        );
        root.insert("requires_auth".into(), JsonValue::Bool(false));
        root.insert("skip_canonical_filtering".into(), JsonValue::Bool(true));
        root.insert("dynamic_models".into(), JsonValue::Bool(false));
        let models = json_array_member(root, "models", "Goose Provider 配置")?;
        let present = models.iter().any(|entry| {
            entry.get("name").and_then(JsonValue::as_str) == Some(model)
        });
        if !present {
            models.push(serde_json::json!({ "name": model, "reasoning": false }));
        }
        Ok(())
    })
}
