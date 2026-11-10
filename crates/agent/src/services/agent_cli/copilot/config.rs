use super::super::config_support::{
    cli_config_target, config_error, json_array_member, latest_modified_at, match_provider_key,
    merge_json_config, normalize_endpoint, oauth_snapshot, parse_jsonc_document, plain_secret,
    read_stable_optional, relay_snapshot, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::{Map, Value as JsonValue};
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";

/// Copilot CLI keeps `settings.json` and its BYOK file beside each other;
/// `COPILOT_HOME` moves the whole folder, and `COPILOT_PROVIDERS_CONFIG`
/// moves only the provider file.
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_path("COPILOT_HOME").unwrap_or_else(|| home.join(".copilot"))
}

pub(super) fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

pub(super) fn providers_path(root: &Path) -> PathBuf {
    configured_path("COPILOT_PROVIDERS_CONFIG").unwrap_or_else(|| root.join("providers.json"))
}

pub(super) fn config_root() -> Option<PathBuf> {
    Some(config_dir_for_home(&user_home()?))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(root) = config_root() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let settings = match read_stable_optional(&settings_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Copilot CLI 设置失败"),
    };
    let provider = match read_stable_optional(&providers_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Copilot CLI Provider 配置失败"),
    };
    let modified_at = latest_modified_at([settings.as_ref(), provider.as_ref()]);
    let Some(settings) = settings else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(
        &settings.text,
        provider.as_ref().map(|file| file.text.as_str()),
    ) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Copilot CLI 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let root = config_root().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = settings_path(&root);
    let providers_path = providers_path(&root);
    if settings_path == providers_path {
        return Err("Copilot CLI 的设置文件与 Provider 配置不能指向同一路径".into());
    }
    let settings = read_stable_optional(&settings_path)?.map(|file| file.text);
    let providers = read_stable_optional(&providers_path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Copilot CLI".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let settings_after =
                merge_settings(settings.as_deref().unwrap_or_default(), model)?;
            let providers_after = merge_providers(
                providers.as_deref().unwrap_or_default(),
                model,
                &base_url,
                &api_key,
            )?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: settings_path,
                    before: settings,
                    after: settings_after,
                },
                ProviderConfigurationCandidate {
                    path: providers_path,
                    before: providers,
                    after: providers_after,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err("Copilot CLI 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(
    settings_text: &str,
    providers_text: Option<&str>,
) -> Result<Option<(String, String)>, ()> {
    if settings_text.trim().is_empty() {
        return Ok(None);
    }
    let settings = parse_jsonc_document(settings_text).map_err(|_| ())?;
    let model = settings
        .get("model")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .and_then(|value| value.strip_prefix("balancehub/"))
        .filter(|value| !value.is_empty());
    if model.is_none() {
        return Ok(None);
    }
    let Some(providers_text) = providers_text.filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    let providers = parse_jsonc_document(providers_text).map_err(|_| ())?;
    let entry = providers
        .get("providers")
        .and_then(JsonValue::as_array)
        .and_then(|providers| {
            providers
                .iter()
                .find(|entry| entry.get("name").and_then(JsonValue::as_str) == Some(PROVIDER_ID))
        })
        .and_then(JsonValue::as_object);
    let base_url = entry
        .and_then(|entry| entry.get("baseUrl"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let api_key = entry
        .and_then(|entry| entry.get("apiKey"))
        .and_then(JsonValue::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

fn merge_settings(source: &str, model: &str) -> Result<String, String> {
    merge_json_config(source, "Copilot CLI 设置", |root| {
        root.insert(
            "model".into(),
            JsonValue::String(format!("{PROVIDER_ID}/{model}")),
        );
        Ok(())
    })
}

fn merge_providers(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    merge_json_config(source, "Copilot CLI Provider 配置", |root| {
        let providers = json_array_member(root, "providers", "Copilot CLI Provider 配置")?;
        let position = providers.iter().position(|entry| {
            entry.get("name").and_then(JsonValue::as_str) == Some(PROVIDER_ID)
        });
        let entry = match position {
            Some(position) => providers[position]
                .as_object_mut()
                .ok_or_else(|| "Copilot CLI 的 Provider 条目必须是对象".to_string())?,
            None => {
                providers.push(JsonValue::Object(Map::new()));
                providers
                    .last_mut()
                    .and_then(JsonValue::as_object_mut)
                    .ok_or_else(|| "Copilot CLI 的 Provider 条目必须是对象".to_string())?
            }
        };
        entry.insert(
            "name".into(),
            JsonValue::String(PROVIDER_ID.to_string()),
        );
        entry.insert("type".into(), JsonValue::String("openai".into()));
        entry.insert("baseUrl".into(), JsonValue::String(base_url.to_string()));
        entry.insert("apiKey".into(), JsonValue::String(api_key.to_string()));

        let models = json_array_member(root, "models", "Copilot CLI Provider 配置")?;
        models.retain(|entry| {
            entry.get("provider").and_then(JsonValue::as_str) != Some(PROVIDER_ID)
        });
        models.push(serde_json::json!({
            "id": model,
            "provider": PROVIDER_ID,
            "name": model,
        }));
        Ok(())
    })
}
