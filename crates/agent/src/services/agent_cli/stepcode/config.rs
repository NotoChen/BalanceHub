use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, parse_jsonc_document, read_stable_optional, relay_snapshot,
    rewrite_json_document, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::services::agent_cli::config_support::text_document;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::{Map, Value as JsonValue};
use std::path::{Path, PathBuf};
use toml_edit::{value as toml_value, Document as TomlDocument};

const PROVIDER_ID: &str = "balancehub";

/// StepCode keeps `config.toml` and `models.json` in `$STEPCODE_CONFIG_DIR`
/// (or the parent of `$STEP_CODING_AGENT_DIR`), else `~/.stepcode`.
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    if let Some(dir) = configured_path("STEP_CODING_AGENT_DIR").filter(|dir| dir.is_absolute()) {
        return dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| dir.clone());
    }
    if let Some(dir) = configured_path("STEPCODE_CONFIG_DIR") {
        return if dir.is_absolute() {
            dir
        } else {
            home.join(dir)
        };
    }
    home.join(".stepcode")
}

pub(super) fn config_path(root: &Path) -> PathBuf {
    root.join("config.toml")
}

pub(super) fn models_path(root: &Path) -> PathBuf {
    root.join("models.json")
}

fn config_root() -> Option<PathBuf> {
    Some(config_dir_for_home(&user_home()?))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(root) = config_root() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let config = match read_stable_optional(&config_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 StepCode 配置失败"),
    };
    let models = match read_stable_optional(&models_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 StepCode 模型配置失败"),
    };
    let modified_at = latest_modified_at([config.as_ref(), models.as_ref()]);
    let Some(config) = config else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&config.text, models.as_ref().map(|file| file.text.as_str())) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "StepCode 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let root = config_root().ok_or_else(|| "无法定位用户目录".to_string())?;
    let config_path = config_path(&root);
    let models_path = models_path(&root);
    let config = read_stable_optional(&config_path)?.map(|file| file.text);
    let models = read_stable_optional(&models_path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 StepCode".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let next_config = merge_config(config.as_deref().unwrap_or_default(), model)?;
            let next_models = merge_models(
                models.as_deref().unwrap_or_default(),
                &provider.identity.name,
                model,
                &base_url,
                &api_key,
            )?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: config_path,
                    before: config,
                    after: next_config,
                },
                ProviderConfigurationCandidate {
                    path: models_path,
                    before: models,
                    after: next_models,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err("StepCode 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(
    config_text: &str,
    models_text: Option<&str>,
) -> Result<Option<(String, String)>, ()> {
    if config_text.trim().is_empty() {
        return Ok(None);
    }
    let config = text_document::body(config_text)
        .parse::<toml::Value>()
        .map_err(|_| ())?;
    let provider = config
        .get("defaultProvider")
        .and_then(toml::Value::as_str)
        .map(str::trim);
    if provider != Some(PROVIDER_ID) {
        return Ok(None);
    }
    let model = config
        .get("defaultModel")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if model.is_none() {
        return Ok(None);
    }
    let Some(models_text) = models_text.filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    let models = parse_jsonc_document(models_text).map_err(|_| ())?;
    models.as_object().ok_or(())?;
    let entry = models
        .pointer("/providers/balancehub")
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
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key.to_string())))
}

fn merge_config(source: &str, model: &str) -> Result<String, String> {
    let mut document = if source.trim().is_empty() {
        TomlDocument::new()
    } else {
        text_document::body(source)
            .parse::<TomlDocument>()
            .map_err(|_| "StepCode 配置文件格式无效".to_string())?
    };
    let table = document.as_table_mut();
    table.insert("defaultProvider", toml_value(PROVIDER_ID));
    table.insert("defaultModel", toml_value(model));
    Ok(text_document::restore_layout(source, &document.to_string()))
}

fn merge_models(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let mut root = if source.trim().is_empty() {
        Map::new()
    } else {
        parse_jsonc_document(source)?
            .as_object()
            .cloned()
            .ok_or_else(|| "StepCode 模型配置根节点不是对象".to_string())?
    };
    {
        let providers = object_member(&mut root, "providers")?;
        let entry = providers
            .entry(PROVIDER_ID.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "StepCode 模型配置中的 providers.balancehub 必须是对象".to_string())?;
        let display_name = provider_name.trim();
        entry.insert(
            "name".into(),
            JsonValue::String(if display_name.is_empty() {
                "BalanceHub".into()
            } else {
                format!("BalanceHub · {display_name}")
            }),
        );
        entry.insert("baseUrl".into(), JsonValue::String(base_url.to_string()));
        entry.insert("api".into(), JsonValue::String("openai-completions".into()));
        entry.insert("apiKey".into(), JsonValue::String(api_key.to_string()));
        let models = entry
            .entry("models".to_string())
            .or_insert_with(|| JsonValue::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| "StepCode 模型配置中的 models 必须是数组".to_string())?;
        match models
            .iter_mut()
            .find(|entry| entry.get("id").and_then(JsonValue::as_str) == Some(model))
        {
            Some(entry) => {
                let entry = entry
                    .as_object_mut()
                    .ok_or_else(|| "StepCode 模型条目必须是对象".to_string())?;
                let named = entry
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .map(str::trim)
                    .is_some_and(|name| !name.is_empty());
                if !named {
                    entry.insert("name".into(), JsonValue::String(model.to_string()));
                }
            }
            None => models.push(serde_json::json!({ "id": model, "name": model })),
        }
    }
    let desired = JsonValue::Object(root);
    if source.trim().is_empty() {
        serde_json::to_string_pretty(&desired)
            .map_err(|err| format!("生成 StepCode 模型配置失败: {err}"))
    } else {
        rewrite_json_document(source, &desired, true)
            .map_err(|err| format!("生成 StepCode 模型配置失败: {err}"))
    }
}

fn object_member<'a>(
    root: &'a mut Map<String, JsonValue>,
    key: &str,
) -> Result<&'a mut Map<String, JsonValue>, String> {
    root.entry(key.to_string())
        .or_insert_with(|| JsonValue::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("StepCode 模型配置中的 {key} 必须是对象"))
}
