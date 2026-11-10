use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, read_stable_optional, relay_snapshot, rewrite_json_document, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::agent_cli::integration::validate_native_value,
    services::cli_paths::{configured_path, user_home},
};
use serde_json::{Map, Value as JsonValue};
use std::path::{Path, PathBuf};

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_path("QWEN_HOME").unwrap_or_else(|| home.join(".qwen"))
}

pub(super) fn settings_path_for_config_root(root: &Path) -> PathBuf {
    root.join("settings.json")
}

fn config_dir() -> Option<PathBuf> {
    configured_path("QWEN_HOME").or_else(|| user_home().map(|home| home.join(".qwen")))
}

/// Qwen writes project logs beneath the runtime base, which may differ from
/// `QWEN_HOME` when a separate runtime directory is configured.
pub(super) fn runtime_base_dir() -> Option<PathBuf> {
    configured_path("QWEN_RUNTIME_DIR")
        .filter(|path| path.is_absolute())
        .or_else(config_dir)
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(root) = config_dir() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let path = settings_path_for_config_root(&root);
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Qwen Code 设置失败"),
    };
    let modified_at = latest_modified_at([file.as_ref()]);
    let Some(file) = file else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_settings(&file.text) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Qwen Code 设置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let root = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let path = settings_path_for_config_root(&root);
    let before = read_stable_optional(&path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Qwen Code".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let after = merge_settings(
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
        CliConfigTarget::OAuth { .. } => Err("Qwen Code 不支持 OAuth 账号切换".into()),
    }
}

fn parse_settings(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = super::super::config_support::parse_jsonc_document(text).map_err(|_| ())?;
    let root = value.as_object().ok_or(())?;
    let selected_type = value
        .pointer("/security/auth/selectedType")
        .and_then(JsonValue::as_str);
    if selected_type.is_some_and(|value| value != "openai") {
        return Ok(None);
    }
    let env = root.get("env").and_then(JsonValue::as_object);
    let active_model = value
        .pointer("/model/name")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let active_entry = active_model.and_then(|model| {
        value
            .pointer("/modelProviders/openai")
            .and_then(JsonValue::as_array)?
            .iter()
            .find(|entry| entry.get("id").and_then(JsonValue::as_str) == Some(model))
    });
    let base_url = active_entry
        .and_then(|entry| entry.get("baseUrl"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| env_value(env, "OPENAI_BASE_URL"));
    let key_name = active_entry
        .and_then(|entry| entry.get("envKey"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("OPENAI_API_KEY");
    let api_key = env_value(env, key_name);
    if let Some(base_url) = base_url {
        if normalize_endpoint(base_url).is_none() {
            return Err(());
        }
    }
    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => {
            validate_native_value(base_url, "中转站地址").map_err(|_| ())?;
            validate_native_value(api_key, "API Key").map_err(|_| ())?;
            Some((base_url.to_string(), api_key.to_string()))
        }
        _ => None,
    })
}

fn env_value<'a>(env: Option<&'a Map<String, JsonValue>>, key: &str) -> Option<&'a str> {
    env.and_then(|env| env.get(key))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn merge_settings(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let mut root = if source.trim().is_empty() {
        Map::new()
    } else {
        super::super::config_support::parse_jsonc_document(source)?
            .as_object()
            .cloned()
            .ok_or_else(|| "Qwen Code 设置文件根节点不是对象".to_string())?
    };
    {
        let security = object_member(&mut root, "security")?;
        let auth = object_member(security, "auth")?;
        auth.insert("selectedType".into(), JsonValue::String("openai".into()));
    }
    {
        let env = object_member(&mut root, "env")?;
        env.insert(
            "OPENAI_API_KEY".into(),
            JsonValue::String(api_key.to_string()),
        );
        env.insert(
            "OPENAI_BASE_URL".into(),
            JsonValue::String(base_url.to_string()),
        );
        env.insert("OPENAI_MODEL".into(), JsonValue::String(model.to_string()));
    }
    {
        let model_member = object_member(&mut root, "model")?;
        model_member.insert("name".into(), JsonValue::String(model.to_string()));
    }
    {
        let providers = object_member(&mut root, "modelProviders")?;
        let entries = providers
            .entry("openai".to_string())
            .or_insert_with(|| JsonValue::Array(Vec::new()));
        let entries = entries
            .as_array_mut()
            .ok_or_else(|| "Qwen Code 设置中的 modelProviders.openai 必须是数组".to_string())?;
        match entries
            .iter_mut()
            .find(|entry| entry.get("id").and_then(JsonValue::as_str) == Some(model))
        {
            Some(entry) => {
                let entry = entry
                    .as_object_mut()
                    .ok_or_else(|| "Qwen Code 模型条目必须是对象".to_string())?;
                entry.insert(
                    "baseUrl".into(),
                    JsonValue::String(base_url.to_string()),
                );
                entry.insert(
                    "envKey".into(),
                    JsonValue::String("OPENAI_API_KEY".into()),
                );
            }
            None => entries.push(serde_json::json!({
                "id": model,
                "name": model,
                "baseUrl": base_url,
                "envKey": "OPENAI_API_KEY",
            })),
        }
    }
    let desired = JsonValue::Object(root);
    if source.trim().is_empty() {
        serde_json::to_string_pretty(&desired)
            .map_err(|err| format!("生成 Qwen Code 设置失败: {err}"))
    } else {
        rewrite_json_document(source, &desired, true)
            .map_err(|err| format!("生成 Qwen Code 设置失败: {err}"))
    }
}

fn object_member<'a>(
    root: &'a mut Map<String, JsonValue>,
    key: &str,
) -> Result<&'a mut Map<String, JsonValue>, String> {
    root.entry(key.to_string())
        .or_insert_with(|| JsonValue::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("Qwen Code 设置中的 {key} 必须是对象"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_cli::config_support::parse_jsonc_document;

    #[test]
    fn parses_active_openai_provider_from_env_and_model_entry() {
        let parsed = parse_settings(
            r#"{
              // user settings keep comments
              "security": { "auth": { "selectedType": "openai" } },
              "model": { "name": "relay-model" },
              "modelProviders": {
                "openai": [
                  { "id": "relay-model", "baseUrl": "https://relay.example.com/v1", "envKey": "RELAY_KEY" }
                ]
              },
              "env": { "RELAY_KEY": "sk-test" }
            }"#,
        )
        .unwrap()
        .unwrap();

        assert_eq!(parsed.0, "https://relay.example.com/v1");
        assert_eq!(parsed.1, "sk-test");
    }

    #[test]
    fn non_openai_auth_type_is_not_reported_as_an_active_relay() {
        assert!(parse_settings(
            r#"{"security":{"auth":{"selectedType":"qwen-oauth"}},"env":{"OPENAI_API_KEY":"sk-test","OPENAI_BASE_URL":"https://relay.example.com/v1"}}"#
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn rewrite_preserves_unrelated_settings_and_updates_the_provider_entry() {
        let source = r#"{
  "theme": "dark",
  "modelProviders": {
    "openai": [
      { "id": "old-model", "baseUrl": "https://old.example.com/v1" }
    ]
  }
}
"#;
        let after = merge_settings(
            source,
            "new-model",
            "https://relay.example.com/v1",
            "sk-new",
        )
        .unwrap();
        let value = parse_jsonc_document(&after).unwrap();

        assert_eq!(value["theme"], "dark");
        assert_eq!(value["security"]["auth"]["selectedType"], "openai");
        assert_eq!(value["model"]["name"], "new-model");
        assert_eq!(
            value["modelProviders"]["openai"][0]["id"],
            "old-model"
        );
        assert_eq!(
            value["modelProviders"]["openai"][1]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(value["env"]["OPENAI_API_KEY"], "sk-new");
    }

    #[test]
    fn rewrite_creates_a_pretty_settings_file_without_a_source() {
        let after = merge_settings("", "m", "https://relay.example.com/v1", "key").unwrap();
        assert!(after.contains("\n  \"security\""));
        let value: JsonValue = serde_json::from_str(&after).unwrap();
        assert_eq!(value["modelProviders"]["openai"][0]["id"], "m");
    }
}
