use super::super::config_support::{
    cli_config_target, config_error, json_array_member, latest_modified_at, match_provider_key,
    merge_json_config, normalize_endpoint, oauth_snapshot, parse_jsonc_document,
    read_stable_optional, relay_snapshot, rewrite_json_document, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::{json, Map, Value as JsonValue};
use std::path::{Path, PathBuf};

const VENDOR: &str = "BalanceHub";

/// CodeBuddy Code keeps both files in one folder; `$CODEBUDDY_CONFIG_DIR`
/// moves it.
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_path("CODEBUDDY_CONFIG_DIR").unwrap_or_else(|| home.join(".codebuddy"))
}

pub(super) fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

pub(super) fn models_path(root: &Path) -> PathBuf {
    root.join("models.json")
}

fn root() -> Option<PathBuf> {
    Some(config_dir_for_home(&user_home()?))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(root) = root() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let settings = match read_stable_optional(&settings_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 CodeBuddy 设置失败"),
    };
    let models = match read_stable_optional(&models_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 CodeBuddy 模型配置失败"),
    };
    let modified_at = latest_modified_at([settings.as_ref(), models.as_ref()]);
    let Some(settings) = settings else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&settings.text, models.as_ref().map(|file| file.text.as_str())) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "CodeBuddy 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let root = root().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = settings_path(&root);
    let models_path = models_path(&root);
    let settings = read_stable_optional(&settings_path)?.map(|file| file.text);
    let models = read_stable_optional(&models_path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 CodeBuddy".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let settings_after = merge_json_config(
                settings.as_deref().unwrap_or_default(),
                "CodeBuddy 设置",
                |root| {
                    root.insert("model".into(), JsonValue::String(model.into()));
                    Ok(())
                },
            )?;
            let models_after = merge_models(
                models.as_deref().unwrap_or_default(),
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
                    path: models_path,
                    before: models,
                    after: models_after,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err("CodeBuddy 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(
    settings_text: &str,
    models_text: Option<&str>,
) -> Result<Option<(String, String)>, ()> {
    if settings_text.trim().is_empty() {
        return Ok(None);
    }
    let settings = parse_jsonc_document(settings_text).map_err(|_| ())?;
    let model = settings
        .get("model")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(model) = model else {
        return Ok(None);
    };
    let Some(models_text) = models_text.filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    let models = parse_jsonc_document(models_text).map_err(|_| ())?;
    let entry = models
        .get("models")
        .and_then(JsonValue::as_array)
        .or_else(|| models.as_array())
        .and_then(|models| {
            models.iter().find(|entry| {
                entry.get("id").and_then(JsonValue::as_str) == Some(model)
                    && entry.get("vendor").and_then(JsonValue::as_str) == Some(VENDOR)
            })
        })
        .and_then(JsonValue::as_object);
    let url = entry
        .and_then(|entry| entry.get("url"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(url) = url else {
        return Ok(None);
    };
    let base_url = url
        .trim_end_matches('/')
        .strip_suffix("/chat/completions")
        .unwrap_or(url)
        .trim_end_matches('/');
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let api_key = entry
        .and_then(|entry| entry.get("apiKey"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .map(str::to_string);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

fn merge_models(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    if source.trim().is_empty() {
        return serde_json::to_string_pretty(&json!({
            "models": [entry(model, &url, api_key, None)],
        }))
        .map_err(|err| format!("生成 CodeBuddy 模型配置失败: {err}"));
    }
    let value = parse_jsonc_document(source)?;
    match value {
        JsonValue::Object(mut root) => {
            {
                let models = json_array_member(&mut root, "models", "CodeBuddy 模型配置")?;
                replace_entry(models, model, &url, api_key);
            }
            if let Some(available) = root
                .get_mut("availableModels")
                .and_then(JsonValue::as_array_mut)
            {
                if !available.is_empty() {
                    available.retain(|id| id.as_str() != Some(model));
                    available.push(JsonValue::String(model.into()));
                }
            }
            let desired = JsonValue::Object(root);
            rewrite_json_document(source, &desired, true)
                .map_err(|err| format!("生成 CodeBuddy 模型配置失败: {err}"))
        }
        JsonValue::Array(mut models) => {
            replace_entry(&mut models, model, &url, api_key);
            serde_json::to_string_pretty(&JsonValue::Array(models))
                .map_err(|err| format!("生成 CodeBuddy 模型配置失败: {err}"))
        }
        _ => Err("CodeBuddy 模型配置根节点必须是对象或数组".into()),
    }
}

fn replace_entry(models: &mut Vec<JsonValue>, model: &str, url: &str, api_key: &str) {
    let existing = models
        .iter()
        .find(|item| {
            item.get("id").and_then(JsonValue::as_str) == Some(model)
                && item.get("vendor").and_then(JsonValue::as_str) == Some(VENDOR)
        })
        .and_then(JsonValue::as_object)
        .cloned();
    let merged = entry(model, url, api_key, existing.as_ref());
    let mut replaced = false;
    let mut output = Vec::with_capacity(models.len() + 1);
    for item in std::mem::take(models) {
        let ours = item.get("vendor").and_then(JsonValue::as_str) == Some(VENDOR);
        if ours {
            if !replaced
                && item.get("id").and_then(JsonValue::as_str) == Some(model)
            {
                output.push(merged.clone());
                replaced = true;
            }
            continue;
        }
        output.push(item);
    }
    if !replaced {
        output.push(merged);
    }
    *models = output;
}

fn entry(
    model: &str,
    url: &str,
    api_key: &str,
    existing: Option<&Map<String, JsonValue>>,
) -> JsonValue {
    let mut entry = existing.cloned().unwrap_or_default();
    entry.insert("id".into(), JsonValue::String(model.into()));
    if entry
        .get("name")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .is_none_or(|name| name.is_empty())
    {
        entry.insert("name".into(), JsonValue::String(model.into()));
    }
    entry.insert("vendor".into(), JsonValue::String(VENDOR.into()));
    entry.insert("apiKey".into(), JsonValue::String(api_key.into()));
    entry.insert("url".into(), JsonValue::String(url.into()));
    entry.insert("supportsToolCall".into(), JsonValue::Bool(true));
    entry.insert("supportsImages".into(), JsonValue::Bool(false));
    entry.insert("supportsReasoning".into(), JsonValue::Bool(false));
    JsonValue::Object(entry)
}
