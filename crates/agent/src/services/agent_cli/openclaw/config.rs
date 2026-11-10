use super::super::config_support::{
    cli_config_target, config_error, json_array_member, json_object_member, latest_modified_at,
    match_provider_key, merge_json_config, normalize_endpoint, oauth_snapshot, parse_jsonc_document,
    plain_secret, read_stable_optional, relay_snapshot, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::{expand_tilde, validate_native_value};
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::{json, Map, Value as JsonValue};
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";

/// OpenClaw's state folder follows its own resolution order:
/// `$OPENCLAW_CONFIG_PATH`, then `$OPENCLAW_STATE_DIR`, then
/// `$OPENCLAW_HOME/.openclaw[-profile]`.
pub(super) fn config_path_for_home(home: &Path) -> PathBuf {
    let openclaw_home = openclaw_home(home);
    if let Some(path) = configured_path("OPENCLAW_CONFIG_PATH") {
        return expand_from(path, &openclaw_home);
    }
    config_dir_for_home(home).join("openclaw.json")
}

pub(super) fn state_dir() -> Option<PathBuf> {
    Some(config_dir_for_home(&user_home()?))
}

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    let openclaw_home = openclaw_home(home);
    let mut dir = openclaw_home.join(".openclaw");
    if let Some(profile) = std::env::var("OPENCLAW_PROFILE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("default"))
        .filter(|value| valid_profile(value))
    {
        dir = openclaw_home.join(format!(".openclaw-{profile}"));
    }
    if let Some(state) = configured_path("OPENCLAW_STATE_DIR") {
        dir = expand_from(state, &openclaw_home);
    }
    dir
}

fn openclaw_home(home: &Path) -> PathBuf {
    configured_path("OPENCLAW_HOME").map_or_else(|| home.to_path_buf(), |path| {
        expand_from(path, home)
    })
}

fn expand_from(path: PathBuf, home: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text.starts_with('~') {
        expand_tilde(&text, home)
    } else {
        path
    }
}

fn valid_profile(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 64
        && first.is_ascii_alphanumeric()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn config_path() -> Option<PathBuf> {
    Some(config_path_for_home(&user_home()?))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(path) = config_path() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 OpenClaw 配置失败"),
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
            ..config_error(cli_kind, "OpenClaw 配置文件格式无效")
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
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 OpenClaw".into());
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
        CliConfigTarget::OAuth { .. } => Err("OpenClaw 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = parse_jsonc_document(text).map_err(|_| ())?;
    let selected = value
        .pointer("/agents/defaults/model")
        .and_then(|model| {
            model
                .as_str()
                .or_else(|| model.get("primary").and_then(JsonValue::as_str))
        })
        .map(str::trim)
        .and_then(|value| value.strip_prefix("balancehub/"))
        .filter(|value| !value.is_empty());
    if selected.is_none() {
        return Ok(None);
    }
    let entry = value
        .pointer("/models/providers/balancehub")
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

/// Temporary-launch provider document: no native file is read, and the
/// launch directory owns the resulting config for the lifetime of the TUI.
pub(super) fn temporary_provider_config(
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    merge_config("", model, base_url, api_key)
}

/// Native launches keep `OPENCLAW_STATE_DIR` untouched so sessions still land
/// in the user's own library. Only the model pointer moves: the native config
/// is copied to the launch directory with that one value replaced.
pub(super) fn temporary_native_config(model: &str) -> Result<String, String> {
    let path = config_path().ok_or_else(|| "无法定位 OpenClaw 配置".to_string())?;
    let source = read_stable_optional(&path)
        .map_err(|error| format!("读取 OpenClaw 配置失败：{error}"))?
        .map(|file| file.text)
        .ok_or_else(|| "OpenClaw 原生配置不存在".to_string())?;
    rewrite_native_model(&source, model)
}

fn rewrite_native_model(source: &str, model: &str) -> Result<String, String> {
    let mut root = parse_jsonc_document(source)
        .map_err(|_| "OpenClaw 配置文件格式无效".to_string())?;
    let reference = native_model_reference(&root, model)?;
    set_default_model(&mut root, &reference)?;
    serde_json::to_string_pretty(&root)
        .map_err(|error| format!("生成 OpenClaw 临时配置失败：{error}"))
}

fn native_model_reference(root: &JsonValue, model: &str) -> Result<String, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("模型 ID 不能为空".to_string());
    }
    if model.contains('/') {
        return Ok(model.to_string());
    }
    if let Some(provider) = provider_with_model(root, model) {
        return Ok(format!("{provider}/{model}"));
    }
    if let Some(reference) = current_default_model(root) {
        if let Some((provider, _)) = reference.split_once('/') {
            return Ok(format!("{provider}/{model}"));
        }
    }
    Err("OpenClaw 原生模式需要使用 provider/model 形式的模型 ID".to_string())
}

fn provider_with_model(root: &JsonValue, model: &str) -> Option<String> {
    root.pointer("/models/providers")?
        .as_object()?
        .iter()
        .find_map(|(provider, entry)| {
            entry
                .get("models")?
                .as_array()?
                .iter()
                .any(|item| item.get("id").and_then(JsonValue::as_str) == Some(model))
                .then(|| provider.clone())
        })
}

fn current_default_model(root: &JsonValue) -> Option<&str> {
    let model = root.pointer("/agents/defaults/model")?;
    model
        .as_str()
        .or_else(|| model.get("primary").and_then(JsonValue::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn set_default_model(root: &mut JsonValue, reference: &str) -> Result<(), String> {
    let root = root
        .as_object_mut()
        .ok_or_else(|| "OpenClaw 配置根节点不是对象".to_string())?;
    let agents = json_object_member(root, "agents", "OpenClaw 配置")?;
    let defaults = json_object_member(agents, "defaults", "OpenClaw 配置")?;
    match defaults.get_mut("model") {
        Some(JsonValue::Object(existing)) => {
            existing.insert("primary".into(), JsonValue::String(reference.into()));
        }
        _ => {
            defaults.insert("model".into(), JsonValue::String(reference.into()));
        }
    }
    Ok(())
}

fn merge_config(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let reference = format!("{PROVIDER_ID}/{model}");
    merge_json_config(source, "OpenClaw 配置", |root| {
        {
            let models = json_object_member(root, "models", "OpenClaw 配置")?;
            let providers = json_object_member(models, "providers", "OpenClaw 配置")?;
            let entry = providers
                .entry(PROVIDER_ID.to_string())
                .or_insert_with(|| JsonValue::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| "OpenClaw 配置中的 models.providers.balancehub 必须是对象".to_string())?;
            entry.insert("baseUrl".into(), JsonValue::String(base_url.into()));
            entry.insert("apiKey".into(), JsonValue::String(api_key.into()));
            entry.insert("api".into(), JsonValue::String("openai-completions".into()));
            let list = json_array_member(entry, "models", "OpenClaw 配置")?;
            let existing = list
                .iter()
                .find(|item| item.get("id").and_then(JsonValue::as_str) == Some(model))
                .and_then(JsonValue::as_object)
                .cloned();
            let mut merged = existing.unwrap_or_default();
            merged.insert("id".into(), JsonValue::String(model.into()));
            merged.insert("name".into(), JsonValue::String(model.into()));
            merged.insert("reasoning".into(), JsonValue::Bool(false));
            merged.insert("input".into(), json!(["text"]));
            merged.insert("contextWindow".into(), json!(128000));
            list.clear();
            list.push(JsonValue::Object(merged));
        }
        {
            let agents = json_object_member(root, "agents", "OpenClaw 配置")?;
            let defaults = json_object_member(agents, "defaults", "OpenClaw 配置")?;
            match defaults.get_mut("model") {
                Some(JsonValue::Object(existing)) => {
                    existing.insert("primary".into(), JsonValue::String(reference.clone()));
                }
                _ => {
                    defaults.insert("model".into(), JsonValue::String(reference.clone()));
                }
            }
            if let Some(known) = defaults.get_mut("models").and_then(JsonValue::as_object_mut) {
                known.retain(|key, value| {
                    !(key.starts_with("balancehub/")
                        && value.as_object().is_some_and(|entry| entry.is_empty()))
                });
                known.insert(reference.clone(), JsonValue::Object(Map::new()));
            }
            if let Some(allow) = defaults
                .get_mut("modelPolicy")
                .and_then(JsonValue::as_object_mut)
                .and_then(|policy| policy.get_mut("allow"))
                .and_then(JsonValue::as_array_mut)
            {
                if !allow.is_empty()
                    && !allow.iter().any(|value| {
                        value
                            .as_str()
                            .is_some_and(|value| {
                                value == "*"
                                    || value == reference
                                    || value == "balancehub/*"
                            })
                    })
                {
                    allow.push(JsonValue::String("balancehub/*".into()));
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_model_rewrite_resolves_a_bare_model_against_the_native_provider() {
        let source = r#"{
          // Comments survive only while the native file is on disk.
          "models": {
            "providers": {
              "openai": { "models": [{ "id": "gpt-4o" }, { "id": "gpt-5" }] }
            }
          },
          "agents": { "defaults": { "model": { "primary": "openai/gpt-4o" } } }
        }"#;
        let rewritten = rewrite_native_model(source, "gpt-5").unwrap();
        let value: JsonValue = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(
            value
                .pointer("/agents/defaults/model/primary")
                .and_then(JsonValue::as_str),
            Some("openai/gpt-5")
        );
        assert!(value
            .pointer("/models/providers/openai/models/1/id")
            .is_some());
    }

    #[test]
    fn native_model_rewrite_keeps_a_string_model_shape_and_full_reference() {
        let source = r#"{
          "models": { "providers": {} },
          "agents": { "defaults": { "model": "openai/gpt-4o" } }
        }"#;
        let rewritten = rewrite_native_model(source, "relay/custom-model").unwrap();
        let value: JsonValue = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(
            value.pointer("/agents/defaults/model").and_then(JsonValue::as_str),
            Some("relay/custom-model")
        );
    }

    #[test]
    fn native_model_rewrite_rejects_a_bare_model_without_a_provider() {
        let source = r#"{
          "models": { "providers": {} },
          "agents": { "defaults": {} }
        }"#;
        let error = rewrite_native_model(source, "gpt-5").unwrap_err();
        assert!(error.contains("provider/model"));
    }
}
