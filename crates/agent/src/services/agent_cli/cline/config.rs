use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, parse_jsonc_document, read_stable_optional, relay_snapshot,
    rewrite_json_document, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::{Map, Value as JsonValue};
use std::path::{Path, PathBuf};

const SLOT: &str = "openai-compatible";

/// Cline's CLI keeps its provider data under `$CLINE_DATA_DIR`, else
/// `<CLINE_DIR>/data`; `CLINE_DIR` defaults to `~/.cline`.
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_path("CLINE_DIR").unwrap_or_else(|| home.join(".cline"))
}

pub(super) fn data_dir_for_home(home: &Path) -> PathBuf {
    configured_path("CLINE_DATA_DIR")
        .unwrap_or_else(|| config_dir_for_home(home).join("data"))
}

pub(super) fn providers_path(root: &Path) -> PathBuf {
    root.join("settings/providers.json")
}

pub(super) fn models_path(root: &Path) -> PathBuf {
    root.join("settings/models.json")
}

fn config_root() -> Option<PathBuf> {
    Some(data_dir_for_home(&user_home()?))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(root) = config_root() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let settings = match read_stable_optional(&providers_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Cline Provider 配置失败"),
    };
    let models = match read_stable_optional(&models_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Cline 模型配置失败"),
    };
    let modified_at = latest_modified_at([settings.as_ref(), models.as_ref()]);
    let Some(settings) = settings else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&settings.text) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Cline 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let root = config_root().ok_or_else(|| "无法定位用户目录".to_string())?;
    let providers_path = providers_path(&root);
    let models_path = models_path(&root);
    let providers = read_stable_optional(&providers_path)?.map(|file| file.text);
    let models = read_stable_optional(&models_path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Cline".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let next_providers = merge_providers(
                providers.as_deref().unwrap_or_default(),
                model,
                &base_url,
                &api_key,
            )?;
            let next_models = merge_models(
                models.as_deref().unwrap_or_default(),
                &provider.identity.name,
                model,
                &base_url,
            )?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: providers_path,
                    before: providers,
                    after: next_providers,
                },
                ProviderConfigurationCandidate {
                    path: models_path,
                    before: models,
                    after: next_models,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err("Cline 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = parse_jsonc_document(text).map_err(|_| ())?;
    if value
        .get("lastUsedProvider")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        != Some(SLOT)
    {
        return Ok(None);
    }
    let entry = value
        .pointer(&format!("/providers/{SLOT}/settings"))
        .and_then(JsonValue::as_object);
    if entry
        .and_then(|settings| settings.get("provider"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        != Some(SLOT)
    {
        return Ok(None);
    }
    let base_url = entry
        .and_then(|settings| settings.get("baseUrl"))
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
        .and_then(|settings| settings.get("apiKey"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key.to_string())))
}

fn merge_providers(
    source: &str,
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
            .ok_or_else(|| "Cline Provider 配置根节点不是对象".to_string())?
    };
    root.entry("version".to_string())
        .or_insert_with(|| JsonValue::from(1));
    root.entry("modes".to_string())
        .or_insert_with(|| JsonValue::Object(Map::new()));
    root.insert(
        "lastUsedProvider".into(),
        JsonValue::String(SLOT.into()),
    );
    {
        let providers = object_member(&mut root, "providers", "Cline Provider 配置")?;
        let entry = providers
            .entry(SLOT.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "Cline Provider 配置中的 openai-compatible 必须是对象".to_string())?;
        let settings = object_member(entry, "settings", "Cline Provider 配置")?;
        settings.insert("provider".into(), JsonValue::String(SLOT.into()));
        settings.insert("apiKey".into(), JsonValue::String(api_key.to_string()));
        settings.insert("model".into(), JsonValue::String(model.to_string()));
        settings.insert("baseUrl".into(), JsonValue::String(base_url.to_string()));
        entry.insert("tokenSource".into(), JsonValue::String("manual".into()));
        entry.insert("updatedAt".into(), JsonValue::String(updated_at()));
    }
    let desired = JsonValue::Object(root);
    if source.trim().is_empty() {
        serde_json::to_string_pretty(&desired)
            .map_err(|err| format!("生成 Cline Provider 配置失败: {err}"))
    } else {
        rewrite_json_document(source, &desired, true)
            .map_err(|err| format!("生成 Cline Provider 配置失败: {err}"))
    }
}

fn merge_models(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
) -> Result<String, String> {
    let mut root = if source.trim().is_empty() {
        Map::new()
    } else {
        parse_jsonc_document(source)?
            .as_object()
            .cloned()
            .ok_or_else(|| "Cline 模型配置根节点不是对象".to_string())?
    };
    {
        let providers = object_member(&mut root, "providers", "Cline 模型配置")?;
        let entry = providers
            .entry(SLOT.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "Cline 模型配置中的 openai-compatible 必须是对象".to_string())?;
        let display_name = provider_name.trim();
        let provider = object_member(entry, "provider", "Cline 模型配置")?;
        provider.insert(
            "name".into(),
            JsonValue::String(if display_name.is_empty() {
                "BalanceHub".into()
            } else {
                format!("BalanceHub · {display_name}")
            }),
        );
        provider.insert("baseUrl".into(), JsonValue::String(base_url.to_string()));
        provider.insert(
            "defaultModelId".into(),
            JsonValue::String(model.to_string()),
        );
        let models = object_member(entry, "models", "Cline 模型配置")?;
        let model_entry = models
            .entry(model.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "Cline 模型条目必须是对象".to_string())?;
        model_entry.insert("id".into(), JsonValue::String(model.to_string()));
        let named = model_entry
            .get("name")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .is_some_and(|name| !name.is_empty());
        if !named {
            model_entry.insert("name".into(), JsonValue::String(model.to_string()));
        }
        if !model_entry.contains_key("capabilities") {
            model_entry.insert(
                "capabilities".into(),
                serde_json::json!(["streaming", "tools"]),
            );
        }
    }
    let desired = JsonValue::Object(root);
    if source.trim().is_empty() {
        serde_json::to_string_pretty(&desired)
            .map_err(|err| format!("生成 Cline 模型配置失败: {err}"))
    } else {
        rewrite_json_document(source, &desired, true)
            .map_err(|err| format!("生成 Cline 模型配置失败: {err}"))
    }
}

fn updated_at() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn object_member<'a>(
    root: &'a mut Map<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<&'a mut Map<String, JsonValue>, String> {
    root.entry(key.to_string())
        .or_insert_with(|| JsonValue::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("{context}中的 {key} 必须是对象"))
}
