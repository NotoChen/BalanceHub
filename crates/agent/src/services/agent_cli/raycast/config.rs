use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, plain_secret, read_stable_optional, relay_snapshot, yaml_source, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::user_home,
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";

/// Raycast reads custom providers from `~/.config/raycast/ai/providers.yaml`
/// in the user's folder on both macOS and Windows; XDG_CONFIG_HOME is not
/// consulted.
pub(super) fn config_path_for_home(home: &Path) -> PathBuf {
    home.join(".config/raycast/ai/providers.yaml")
}

pub(super) fn config_path() -> Option<PathBuf> {
    user_home().map(|home| config_path_for_home(&home))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(path) = config_path() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Raycast Provider 配置失败"),
    };
    let modified_at = latest_modified_at([file.as_ref()]);
    let Some(file) = file else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&file.text) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Raycast Provider 配置格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let path = config_path().ok_or_else(|| "无法定位用户目录".to_string())?;
    let before = read_stable_optional(&path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Raycast".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let after = merge_config(
                before.as_deref().unwrap_or_default(),
                model,
                &base_url,
                &api_key,
            )?;
            Ok(vec![ProviderConfigurationCandidate {
                path,
                before,
                after,
            }])
        }
        CliConfigTarget::OAuth { .. } => Err("Raycast 不支持 OAuth 账号切换".into()),
    }
}

pub(super) fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let root = yaml_source::value(text).map_err(|_| ())?;
    let entry = root
        .get("providers")
        .and_then(JsonValue::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(JsonValue::as_str) == Some(PROVIDER_ID))
        });
    let Some(entry) = entry else {
        return Ok(None);
    };
    let base_url = entry
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
    let api_key = entry
        .get("api_keys")
        .and_then(|keys| keys.get(PROVIDER_ID))
        .and_then(JsonValue::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

fn merge_config(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let base_url = super::super::config_support::openai_base_url(base_url);
    yaml_source::edit(source, "Raycast Provider 配置", |root| {
        let providers = yaml_source::sequence_member(root, "providers", "Raycast Provider 配置")?;
        let index = (0..providers.len())
            .find(|index| {
                providers.get(*index).as_ref().is_some_and(|node| {
                    node.as_mapping()
                        .and_then(|entry| yaml_source::text_at(entry, "id"))
                        .as_deref()
                        == Some(PROVIDER_ID)
                })
            })
            .unwrap_or_else(|| {
                providers.push(yaml_edit::Mapping::new());
                providers.len() - 1
            });
        let entry = providers
            .get(index)
            .and_then(|node| node.as_mapping().cloned())
            .ok_or_else(|| "Raycast Provider 配置中的 providers 条目必须是映射".to_string())?;
        entry.set("id", PROVIDER_ID);
        if yaml_source::nonempty_text_at(&entry, "name").is_none() {
            entry.set("name", "BalanceHub");
        }
        entry.set("base_url", base_url.as_str());
        let keys = yaml_source::mapping_member(&entry, "api_keys", "Raycast Provider 配置")?;
        keys.set(PROVIDER_ID, api_key);
        let models = yaml_source::sequence_member(&entry, "models", "Raycast Provider 配置")?;
        let existing = (0..models.len())
            .find(|index| {
                models.get(*index).as_ref().is_some_and(|node| {
                    node.as_mapping()
                        .and_then(|entry| yaml_source::text_at(entry, "id"))
                        .as_deref()
                        == Some(model)
                })
            });
        if existing.is_none() {
            let model_entry = yaml_edit::Mapping::new();
            model_entry.set("id", model);
            model_entry.set("name", model);
            let abilities =
                yaml_source::mapping_member(&model_entry, "abilities", "Raycast Provider 配置")?;
            for (key, supported) in [
                ("temperature", true),
                ("vision", false),
                ("system_message", true),
                ("tools", true),
                ("reasoning_effort", false),
            ] {
                let ability = yaml_source::mapping_member(&abilities, key, "Raycast Provider 配置")?;
                ability.set("supported", supported);
            }
            models.push(model_entry);
        }
        Ok(())
    })
}
