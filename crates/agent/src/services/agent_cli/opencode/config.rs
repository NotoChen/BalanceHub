use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, parse_jsonc_document, read_stable_optional, relay_snapshot,
    rewrite_json_document, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::xdg_config_home;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::agent_cli::integration::validate_native_value,
    services::cli_paths::{configured_path, user_home},
};
use serde_json::{Map, Value as JsonValue};
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_config_file()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| xdg_config_home(home).join("opencode"))
}

pub(super) fn config_path_for_config_root(root: &Path) -> PathBuf {
    if let Some(configured) = configured_config_file() {
        if configured.parent() == Some(root) {
            return configured;
        }
    }
    existing_config_in_dir(root)
}

fn config_path() -> Option<PathBuf> {
    if let Some(configured) = configured_config_file() {
        return Some(configured);
    }
    let home = user_home()?;
    Some(existing_config_in_dir(&xdg_config_home(&home).join("opencode")))
}

fn configured_config_file() -> Option<PathBuf> {
    let path = configured_path("OPENCODE_CONFIG")?;
    if path.is_absolute() {
        Some(path)
    } else {
        std::env::current_dir().ok().map(|cwd| cwd.join(path))
    }
}

fn existing_config_in_dir(directory: &Path) -> PathBuf {
    for name in ["opencode.json", "opencode.jsonc"] {
        let path = directory.join(name);
        if path.is_file() {
            return path;
        }
    }
    directory.join("opencode.json")
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(path) = config_path() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 OpenCode 配置失败"),
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
            ..config_error(cli_kind, "OpenCode 配置文件格式无效")
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
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 OpenCode".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let after = merge_config(
                before.as_deref().unwrap_or_default(),
                &provider.identity.name,
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
        CliConfigTarget::OAuth { .. } => Err("OpenCode 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = parse_jsonc_document(text).map_err(|_| ())?;
    let Some((provider_id, _)) = value
        .get("model")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|model| model.split_once('/'))
    else {
        return Ok(None);
    };
    if provider_id != PROVIDER_ID {
        return Ok(None);
    }
    let options = value
        .get("provider")
        .and_then(|providers| providers.get(provider_id))
        .and_then(|provider| provider.get("options"))
        .and_then(JsonValue::as_object);
    let base_url = options
        .and_then(|options| options.get("baseURL"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = options
        .and_then(|options| options.get("apiKey"))
        .and_then(JsonValue::as_str)
        .and_then(literal_or_environment);
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

/// OpenCode expands `{env:NAME}` inside option strings. Resolve it for the
/// active-config match without ever returning the value to the frontend.
fn literal_or_environment(value: &str) -> Option<String> {
    let value = value.trim();
    let name = value
        .strip_prefix("{env:")
        .and_then(|value| value.strip_suffix('}'))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let resolved = match name {
        Some(name) => std::env::var(name).ok(),
        None => Some(value.to_string()),
    }?;
    let resolved = resolved.trim();
    if resolved.is_empty() || resolved.chars().any(char::is_control) {
        return None;
    }
    Some(resolved.to_string())
}

fn merge_config(
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
            .ok_or_else(|| "OpenCode 配置根节点不是对象".to_string())?
    };
    if source.trim().is_empty() {
        root.insert(
            "$schema".into(),
            JsonValue::String("https://opencode.ai/config.json".into()),
        );
    }
    {
        let providers = object_member(&mut root, "provider")?;
        let entry = providers
            .entry(PROVIDER_ID.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "OpenCode 配置中的 provider.balancehub 必须是对象".to_string())?;
        entry.insert(
            "npm".into(),
            JsonValue::String("@ai-sdk/openai-compatible".into()),
        );
        let display_name = provider_name.trim();
        entry.insert(
            "name".into(),
            JsonValue::String(if display_name.is_empty() {
                "BalanceHub".into()
            } else {
                format!("BalanceHub · {display_name}")
            }),
        );
        let options = object_member(entry, "options")?;
        options.insert("baseURL".into(), JsonValue::String(base_url.to_string()));
        options.insert("apiKey".into(), JsonValue::String(api_key.to_string()));
        let models = object_member(entry, "models")?;
        models
            .entry(model.to_string())
            .or_insert_with(|| serde_json::json!({ "name": model }));
    }
    root.insert(
        "model".into(),
        JsonValue::String(format!("{PROVIDER_ID}/{model}")),
    );
    let desired = JsonValue::Object(root);
    if source.trim().is_empty() {
        serde_json::to_string_pretty(&desired)
            .map_err(|err| format!("生成 OpenCode 配置失败: {err}"))
    } else {
        rewrite_json_document(source, &desired, true)
            .map_err(|err| format!("生成 OpenCode 配置失败: {err}"))
    }
}

fn object_member<'a>(
    root: &'a mut Map<String, JsonValue>,
    key: &str,
) -> Result<&'a mut Map<String, JsonValue>, String> {
    root.entry(key.to_string())
        .or_insert_with(|| JsonValue::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("OpenCode 配置中的 {key} 必须是对象"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_active_balancehub_provider() {
        let parsed = parse_config(
            r#"{
              // selected provider
              "model": "balancehub/relay-model",
              "provider": {
                "balancehub": {
                  "npm": "@ai-sdk/openai-compatible",
                  "options": {
                    "baseURL": "https://relay.example.com/v1",
                    "apiKey": "sk-test"
                  }
                }
              }
            }"#,
        )
        .unwrap()
        .unwrap();

        assert_eq!(parsed.0, "https://relay.example.com/v1");
        assert_eq!(parsed.1, "sk-test");
    }

    #[test]
    fn ignores_a_model_from_another_provider() {
        assert!(parse_config(
            r#"{
              "model": "anthropic/claude-sonnet-4",
              "provider": {
                "balancehub": {
                  "options": {
                    "baseURL": "https://relay.example.com/v1",
                    "apiKey": "sk-test"
                  }
                }
              }
            }"#
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn rewrite_updates_only_the_balancehub_provider() {
        let source = r#"{
  "theme": "system",
  "provider": {
    "other": { "npm": "@ai-sdk/openai", "options": { "apiKey": "keep" } }
  }
}
"#;
        let after = merge_config(
            source,
            "Relay",
            "model-1",
            "https://relay.example.com/v1",
            "sk-new",
        )
        .unwrap();
        let value = parse_jsonc_document(&after).unwrap();

        assert_eq!(value["theme"], "system");
        assert_eq!(value["provider"]["other"]["options"]["apiKey"], "keep");
        assert_eq!(value["provider"][PROVIDER_ID]["npm"], "@ai-sdk/openai-compatible");
        assert_eq!(
            value["provider"][PROVIDER_ID]["options"]["baseURL"],
            "https://relay.example.com/v1"
        );
        assert_eq!(value["model"], "balancehub/model-1");
    }

    #[test]
    fn rewrite_preserves_existing_model_metadata() {
        let source = r#"{
  "provider": {
    "balancehub": {
      "models": {
        "model-1": { "name": "Known model", "limit": { "context": 200000 } }
      }
    }
  }
}
"#;
        let after = merge_config(
            source,
            "",
            "model-1",
            "https://relay.example.com/v1",
            "sk-new",
        )
        .unwrap();
        let value = parse_jsonc_document(&after).unwrap();

        assert_eq!(
            value["provider"][PROVIDER_ID]["models"]["model-1"]["name"],
            "Known model"
        );
        assert_eq!(
            value["provider"][PROVIDER_ID]["models"]["model-1"]["limit"]["context"],
            200000
        );
    }
}
