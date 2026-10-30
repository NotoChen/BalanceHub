use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_native_oauth, match_provider_key,
    oauth_snapshot, read_cli_config, read_stable_optional, relay_snapshot,
    rewrite_json_string_fields, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, OAuthVendor, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

pub(super) fn config_dir() -> Option<PathBuf> {
    configured_config_dir().or_else(|| user_home().map(|home| home.join(".claude")))
}

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_config_dir().unwrap_or_else(|| home.join(".claude"))
}

fn configured_config_dir() -> Option<PathBuf> {
    configured_path("BALANCEHUB_CLAUDE_CONFIG_DIR").or_else(|| configured_path("CLAUDE_CONFIG_DIR"))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(config_dir) = config_dir() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let settings_path = config_dir.join("settings.json");
    let credentials_path = config_dir.join(".credentials.json");
    let settings = match read_stable_optional(&settings_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Claude Code 配置文件失败"),
    };
    // The native login file is only evidence for the /login case; an unreadable
    // file must not hide a working relay configuration.
    let credentials = read_stable_optional(&credentials_path).ok().flatten();
    let modified_at = latest_modified_at([settings.as_ref(), credentials.as_ref()]);
    // A native switch writes the account's own token here; recognizing it keeps
    // the card marked as this CLI's configuration.
    let native_oauth = credentials
        .as_ref()
        .and_then(|file| match_native_oauth(providers, OAuthVendor::Claude, &file.text));
    let Some(settings) = settings else {
        return oauth_snapshot(cli_kind, native_oauth, modified_at);
    };
    match parse_claude_config(&settings.text) {
        Ok(Some((base_url, api_key))) => {
            let matched = match_provider_key(providers, cli_kind, &base_url, &api_key);
            relay_snapshot(cli_kind, matched, modified_at)
        }
        Ok(None) => oauth_snapshot(cli_kind, native_oauth, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Claude Code 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey {
            base_url, api_key, ..
        } => {
            let root = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
            let path = root.join("settings.json");
            let before = read_cli_config(&path, "读取 Claude Code 配置文件")?;
            let after = rewrite_claude_config(&before, &base_url, &api_key)?;
            Ok(vec![ProviderConfigurationCandidate {
                path,
                before: Some(before),
                after,
            }])
        }
        CliConfigTarget::OAuth { auth, identity } => {
            let root = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
            let credentials_path = root.join(".credentials.json");
            let settings_path = root.join("settings.json");
            let credentials = read_stable_optional(&credentials_path)?.map(|file| file.text);
            let settings = read_stable_optional(&settings_path)?.map(|file| file.text);
            let mut candidates = Vec::new();
            // A relay token would outrank the stored login, so the switch also
            // clears the relay fields the API-key path writes.
            if let Some(settings) = settings.as_deref() {
                reject_cloud_provider_settings(settings)?;
                let next = clear_claude_relay_settings(settings)?;
                if next != settings {
                    candidates.push(ProviderConfigurationCandidate {
                        path: settings_path,
                        before: Some(settings.to_string()),
                        after: next,
                    });
                }
            }
            candidates.push(ProviderConfigurationCandidate {
                path: credentials_path,
                before: credentials,
                after: claude_oauth_document(auth, identity)?,
            });
            Ok(candidates)
        }
    }
}

/// A cloud-provider switch in `settings.json` outranks every stored login, so
/// the vendor login could not take effect until it is removed.
fn reject_cloud_provider_settings(settings: &str) -> Result<(), String> {
    let parsed = crate::services::agent_cli::environment::config_document::parse(
        settings.as_bytes(),
        crate::services::agent_cli::environment::config_document::ConfigDocumentFormat::Json,
    )
    .ok_or_else(|| "Claude Code 配置文件格式无效".to_string())?;
    let Some(env) = parsed.get("env").and_then(JsonValue::as_object) else {
        return Ok(());
    };
    for key in [
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        let enabled = env
            .get(key)
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .is_some_and(|value| !matches!(value, "" | "0" | "false"));
        if enabled {
            return Err(format!(
                "Claude Code 已启用 {key}，云服务商会优先于本账号授权；请先在 Claude Code 配置中关闭后再切换"
            ));
        }
    }
    Ok(())
}

fn claude_oauth_document(
    auth: &crate::models::ProviderAuth,
    identity: &crate::models::OAuthCredential,
) -> Result<String, String> {
    let document = crate::services::oauth_accounts::native::document(
        identity.vendor,
        auth,
        identity,
        true,
    );
    serde_json::to_string_pretty(&document)
        .map_err(|err| format!("生成 Claude Code 凭据文件失败: {err}"))
}

/// Removes the relay address and keys, leaving the native `/login` credential
/// as the only one the CLI can select.
fn clear_claude_relay_settings(settings: &str) -> Result<String, String> {
    let parsed = crate::services::agent_cli::environment::config_document::parse(
        settings.as_bytes(),
        crate::services::agent_cli::environment::config_document::ConfigDocumentFormat::Json,
    )
    .ok_or_else(|| "Claude Code 配置文件格式无效".to_string())?;
    let mut root = parsed
        .as_object()
        .cloned()
        .ok_or_else(|| "Claude Code 配置文件格式无效".to_string())?;
    let Some(env) = root.get_mut("env").and_then(JsonValue::as_object_mut) else {
        return Ok(settings.to_string());
    };
    for key in [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
    ] {
        env.remove(key);
    }
    if env.is_empty() {
        root.remove("env");
    }
    crate::services::agent_cli::config_support::rewrite_json_document(
        settings,
        &JsonValue::Object(root),
        true,
    )
    .map_err(|err| format!("生成 Claude Code 配置失败: {err}"))
}

fn parse_claude_config(settings: &str) -> Result<Option<(String, String)>, ()> {
    let settings = crate::services::agent_cli::environment::config_document::parse(
        settings.as_bytes(),
        crate::services::agent_cli::environment::config_document::ConfigDocumentFormat::Json,
    )
    .ok_or(())?;
    let env = settings.get("env").and_then(JsonValue::as_object);
    let base_url = env
        .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = env
        .and_then(|env| {
            env.get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| env.get("ANTHROPIC_API_KEY"))
        })
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url.to_string(), api_key.to_string())),
        _ => None,
    })
}

fn rewrite_claude_config(settings: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    let parsed = crate::services::agent_cli::environment::config_document::parse(
        settings.as_bytes(),
        crate::services::agent_cli::environment::config_document::ConfigDocumentFormat::Json,
    )
    .ok_or_else(|| "Claude Code 配置文件格式无效".to_string())?;
    let root = parsed
        .as_object()
        .ok_or_else(|| "Claude Code 配置文件格式无效".to_string())?;
    let env_value = root.get("env");
    let env = env_value.and_then(JsonValue::as_object);
    if env_value.is_some() && env.is_none() {
        return Err("Claude Code 配置中的 env 不是对象".to_string());
    }

    let has_auth_token = env.is_some_and(|env| env.contains_key("ANTHROPIC_AUTH_TOKEN"));
    let has_api_key = env.is_some_and(|env| env.contains_key("ANTHROPIC_API_KEY"));
    let mut fields = vec![("ANTHROPIC_BASE_URL", base_url.trim())];
    if has_auth_token || !has_api_key {
        fields.push(("ANTHROPIC_AUTH_TOKEN", api_key.trim()));
    }
    if has_api_key {
        fields.push(("ANTHROPIC_API_KEY", api_key.trim()));
    }

    rewrite_json_string_fields(settings, &["env"], &fields)
        .map_err(|err| format!("生成 Claude Code 配置失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_settings_env() {
        let parsed = parse_claude_config(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com","ANTHROPIC_AUTH_TOKEN":"sk-test"}}"#,
        )
        .expect("settings should parse")
        .expect("settings should be complete");

        assert_eq!(parsed.0, "https://relay.example.com");
        assert_eq!(parsed.1, "sk-test");
    }

    #[test]
    fn rewrite_preserves_other_settings_and_updates_existing_key_fields() {
        let settings = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://old.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-old",
    "ANTHROPIC_API_KEY": "sk-old-api",
    "KEEP_ME": "yes"
  },
  "permissions": { "defaultMode": "bypassPermissions" }
}"#;

        let rewritten =
            rewrite_claude_config(settings, "https://new.example.com", "sk-new").unwrap();
        assert_eq!(
            rewritten,
            r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://new.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-new",
    "ANTHROPIC_API_KEY": "sk-new",
    "KEEP_ME": "yes"
  },
  "permissions": { "defaultMode": "bypassPermissions" }
}"#
        );
        let settings = serde_json::from_str::<JsonValue>(&rewritten).unwrap();

        assert_eq!(
            settings["env"]["ANTHROPIC_BASE_URL"],
            "https://new.example.com"
        );
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-new");
        assert_eq!(settings["env"]["ANTHROPIC_API_KEY"], "sk-new");
        assert_eq!(settings["env"]["KEEP_ME"], "yes");
        assert_eq!(settings["permissions"]["defaultMode"], "bypassPermissions");
    }

    #[test]
    fn rewrite_preserves_compact_layout_and_adds_only_missing_fields() {
        let settings = r#"{"permissions":{"defaultMode":"bypassPermissions"},"env":{"KEEP_ME":[1,2],"ANTHROPIC_API_KEY":"sk-old"}}"#;

        let rewritten =
            rewrite_claude_config(settings, "https://new.example.com", "sk-new").unwrap();

        assert_eq!(
            rewritten,
            r#"{"permissions":{"defaultMode":"bypassPermissions"},"env":{"KEEP_ME":[1,2],"ANTHROPIC_API_KEY":"sk-new", "ANTHROPIC_BASE_URL": "https://new.example.com"}}"#
        );
        let parsed = serde_json::from_str::<JsonValue>(&rewritten).unwrap();
        assert_eq!(parsed["env"]["KEEP_ME"], serde_json::json!([1, 2]));
        assert_eq!(parsed["env"]["ANTHROPIC_API_KEY"], "sk-new");
        assert!(parsed["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    }

    #[test]
    fn rewrite_adds_env_without_reformatting_existing_root() {
        let settings = "{\r\n    \"permissions\": {\"defaultMode\": \"bypassPermissions\"}\r\n}";

        let rewritten =
            rewrite_claude_config(settings, "https://new.example.com", "sk-new").unwrap();

        assert_eq!(
            rewritten,
            "{\r\n    \"permissions\": {\"defaultMode\": \"bypassPermissions\"},\r\n    \"env\": {\r\n        \"ANTHROPIC_BASE_URL\": \"https://new.example.com\",\r\n        \"ANTHROPIC_AUTH_TOKEN\": \"sk-new\"\r\n    }\r\n}"
        );
        assert!(serde_json::from_str::<JsonValue>(&rewritten).is_ok());
    }

    #[test]
    fn rewrite_keeps_source_byte_for_byte_when_values_are_unchanged() {
        let settings = r#"{ "env" : { "ANTHROPIC_BASE_URL" : "https://same.example.com", "ANTHROPIC_AUTH_TOKEN" : "sk-same" } }"#;

        let rewritten =
            rewrite_claude_config(settings, "https://same.example.com", "sk-same").unwrap();

        assert_eq!(rewritten, settings);
    }
}
