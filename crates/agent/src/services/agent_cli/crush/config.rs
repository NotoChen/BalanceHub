use super::super::config_support::{
    cli_config_target, config_error, json_array_member, json_object_member, latest_modified_at,
    match_provider_key, merge_json_config, normalize_endpoint, oauth_snapshot, parse_jsonc_document,
    plain_secret, provider_display_name, read_stable_optional, relay_snapshot, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::{validate_native_value, xdg_config_home, xdg_data_home};
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::user_home,
};
use serde_json::{json, Map, Value as JsonValue};
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";

/// Crush reads `crush.json` from the platform config directory, then reads the
/// same-named file in its data directory with the model picks its own picker
/// saved. On Windows both are `%LOCALAPPDATA%\crush\crush.json`.
pub(super) fn config_path_for_home(home: &Path) -> PathBuf {
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
            return PathBuf::from(local).join("crush/crush.json");
        }
    }
    xdg_config_home(home).join("crush/crush.json")
}

pub(super) fn data_path_for_home(home: &Path) -> PathBuf {
    if cfg!(windows) {
        return config_path_for_home(home);
    }
    xdg_data_home(home).join("crush/crush.json")
}

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    config_path_for_home(home)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.to_path_buf())
}

fn config_path() -> Option<PathBuf> {
    user_home().map(|home| config_path_for_home(&home))
}

fn data_path() -> Option<PathBuf> {
    user_home().map(|home| data_path_for_home(&home))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(config_path) = config_path() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let Some(data_path) = data_path() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let config = match read_stable_optional(&config_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Crush 配置失败"),
    };
    let data = if data_path == config_path {
        None
    } else {
        match read_stable_optional(&data_path) {
            Ok(value) => value,
            Err(_) => return config_error(cli_kind, "读取 Crush 数据文件失败"),
        }
    };
    let modified_at = latest_modified_at([config.as_ref(), data.as_ref()]);
    let Some(config) = config else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&config.text, data.as_ref().map(|file| file.text.as_str())) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Crush 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let config_path = config_path().ok_or_else(|| "无法定位用户目录".to_string())?;
    let data_path = data_path().ok_or_else(|| "无法定位用户目录".to_string())?;
    let config = read_stable_optional(&config_path)?.map(|file| file.text);
    let data = if data_path == config_path {
        None
    } else {
        read_stable_optional(&data_path)?.map(|file| file.text)
    };
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Crush".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let config_after = merge_config(
                config.as_deref().unwrap_or_default(),
                &provider.identity.name,
                model,
                &base_url,
                &api_key,
            )?;
            if data_path == config_path {
                let after = merge_selection(&config_after, model)?;
                return Ok(vec![ProviderConfigurationCandidate {
                    path: config_path,
                    before: config,
                    after,
                }]);
            }
            let data_after =
                merge_selection(data.as_deref().unwrap_or_default(), model)?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: config_path,
                    before: config,
                    after: config_after,
                },
                ProviderConfigurationCandidate {
                    path: data_path,
                    before: data,
                    after: data_after,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err("Crush 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(config_text: &str, data_text: Option<&str>) -> Result<Option<(String, String)>, ()> {
    let config = parse_jsonc_document(config_text).map_err(|_| ())?;
    let config = config.as_object().ok_or(())?;
    let data = match data_text {
        Some(text) if !text.trim().is_empty() => {
            let value = parse_jsonc_document(text).map_err(|_| ())?;
            Some(value.as_object().ok_or(())?.clone())
        }
        _ => None,
    };
    // Crush merges its data file over crush.json key by key, so each pick
    // field comes from the data file when it has one and from crush.json
    // otherwise.
    fn pick<'a>(
        data: Option<&'a Map<String, JsonValue>>,
        config: &'a Map<String, JsonValue>,
        slot: &str,
        field: &str,
    ) -> Option<&'a str> {
        data.and_then(|data| {
            data.get("models")
                .and_then(|models| models.get(slot))
                .and_then(|entry| entry.get(field))
                .and_then(JsonValue::as_str)
        })
        .or_else(|| {
            config
                .get("models")
                .and_then(|models| models.get(slot))
                .and_then(|entry| entry.get(field))
                .and_then(JsonValue::as_str)
        })
    }
    let selected = ["large", "small"]
        .iter()
        .find(|slot| pick(data.as_ref(), config, slot, "provider").map(str::trim) == Some(PROVIDER_ID))
        .copied();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let model = pick(data.as_ref(), config, selected, "model")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if model.is_none() {
        return Ok(None);
    }
    let entry = config
        .get("providers")
        .and_then(|providers| providers.get(PROVIDER_ID))
        .and_then(JsonValue::as_object);
    let base_url = entry
        .and_then(|entry| entry.get("base_url"))
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
        .and_then(|entry| entry.get("api_key"))
        .and_then(JsonValue::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

fn merge_config(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    merge_json_config(source, "Crush 配置", |root| {
        let providers = json_object_member(root, "providers", "Crush 配置")?;
        let entry = providers
            .entry(PROVIDER_ID.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "Crush 配置中的 providers.balancehub 必须是对象".to_string())?;
        entry.insert("type".into(), JsonValue::String("openai".into()));
        entry.insert(
            "name".into(),
            JsonValue::String(provider_display_name(provider_name)),
        );
        entry.insert("base_url".into(), JsonValue::String(base_url.into()));
        entry.insert("api_key".into(), JsonValue::String(api_key.into()));
        let models = json_array_member(entry, "models", "Crush 配置")?;
        match models
            .iter_mut()
            .find(|entry| entry.get("id").and_then(JsonValue::as_str) == Some(model))
        {
            Some(entry) => {
                let entry = entry
                    .as_object_mut()
                    .ok_or_else(|| "Crush 模型条目必须是对象".to_string())?;
                let named = entry
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .map(str::trim)
                    .is_some_and(|name| !name.is_empty());
                if !named {
                    entry.insert("name".into(), JsonValue::String(model.into()));
                }
            }
            None => models.push(json!({ "id": model, "name": model })),
        }
        Ok(())
    })
}

/// Crush applies the pick's per-model settings (`max_tokens`, sampling,
/// provider options) to whatever model the object names, so the object is
/// replaced as a whole rather than merged into.
fn merge_selection(source: &str, model: &str) -> Result<String, String> {
    merge_json_config(source, "Crush 数据文件", |root| {
        let models = json_object_member(root, "models", "Crush 数据文件")?;
        models.insert(
            "large".into(),
            json!({ "provider": PROVIDER_ID, "model": model }),
        );
        Ok(())
    })
}
