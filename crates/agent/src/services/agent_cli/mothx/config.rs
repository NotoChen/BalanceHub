use super::super::config_support::{
    cli_config_target, config_error, json_array_member, json_object_member, latest_modified_at,
    match_provider_key, merge_json_config, normalize_endpoint, oauth_snapshot,
    parse_jsonc_document, plain_secret, read_stable_optional, relay_snapshot, CliConfigTarget,
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

/// MothX keeps `settings.json` in `$MOTHX_DIR`, else `%APPDATA%\mothx` on
/// Windows and `~/.mothx` elsewhere.
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    if let Some(dir) = configured_path("MOTHX_DIR") {
        return dir;
    }
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA").filter(|value| !value.is_empty()) {
            return PathBuf::from(appdata).join("mothx");
        }
        return home.join("AppData/Roaming/mothx");
    }
    home.join(".mothx")
}

pub(super) fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

fn settings_file() -> Option<PathBuf> {
    Some(settings_path(&config_dir_for_home(&user_home()?)))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(path) = settings_file() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 MothX 设置失败"),
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
            ..config_error(cli_kind, "MothX 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let path = settings_file().ok_or_else(|| "无法定位用户目录".to_string())?;
    let before = read_stable_optional(&path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 MothX".into());
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
        CliConfigTarget::OAuth { .. } => Err("MothX 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = parse_jsonc_document(text).map_err(|_| ())?;
    if value
        .get("defaultProvider")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        != Some(PROVIDER_ID)
    {
        return Ok(None);
    }
    let model = value
        .get("defaultModel")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if model.is_none() {
        return Ok(None);
    }
    let entry = value
        .get("providers")
        .and_then(|providers| providers.get(PROVIDER_ID))
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

fn merge_config(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    merge_json_config(source, "MothX 设置", |root| {
        root.insert(
            "defaultProvider".into(),
            JsonValue::String(PROVIDER_ID.into()),
        );
        root.insert("defaultModel".into(), JsonValue::String(model.into()));
        let providers = json_object_member(root, "providers", "MothX 设置")?;
        let entry = providers
            .entry(PROVIDER_ID.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "MothX 设置中的 providers.balancehub 必须是对象".to_string())?;
        entry.insert("baseUrl".into(), JsonValue::String(base_url.into()));
        entry.insert("api".into(), JsonValue::String("openai-chat".into()));
        entry.insert("apiKey".into(), JsonValue::String(api_key.into()));
        // `thinkingFormat` decides how a thinking level is sent; keep the
        // user's choice when one is already set.
        if !entry.contains_key("thinkingFormat") {
            entry.insert("thinkingFormat".into(), JsonValue::String("openai".into()));
        }
        let models = json_array_member(entry, "models", "MothX 设置")?;
        let existing = models
            .iter_mut()
            .find(|item| item.get("id").and_then(JsonValue::as_str) == Some(model));
        match existing {
            Some(item) => {
                let item = item
                    .as_object_mut()
                    .ok_or_else(|| "MothX 模型条目必须是对象".to_string())?;
                if item
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .map(str::trim)
                    .is_none_or(|name| name.is_empty())
                {
                    item.insert("name".into(), JsonValue::String(model.into()));
                }
            }
            None => models.push(serde_json::json!({
                "id": model,
                "name": model,
                "input": ["text"],
            })),
        }
        Ok(())
    })
}
