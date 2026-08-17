use super::super::config_support::{
    cli_target, config_error, config_revision, ensure_revision, file_content, latest_modified_at,
    match_provider, read_cli_config, read_stable_optional, rewrite_json_string_fields,
    validate_file_set, write_config_text,
};
use crate::{
    models::{AgentCliKind, CliConfigFile, CliConfigPreview, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::PathBuf;

pub(super) fn config_dir() -> Option<PathBuf> {
    configured_path("BALANCEHUB_CLAUDE_CONFIG_DIR")
        .or_else(|| configured_path("CLAUDE_CONFIG_DIR"))
        .or_else(|| user_home().map(|home| home.join(".claude")))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(config_dir) = config_dir() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let settings_path = config_dir.join("settings.json");
    let settings = match read_stable_optional(&settings_path) {
        Ok(value) => value,
        Err(_) => {
            return config_error(cli_kind, "读取 Claude Code 配置文件失败")
        }
    };
    let modified_at = latest_modified_at([settings.as_ref()]);
    let Some(settings) = settings else {
        return CliConfigSnapshot {
            cli_kind,
            configured: false,
            provider_id: None,
            modified_at,
            error_message: None,
        };
    };
    match parse_claude_config(&settings.text) {
        Ok(Some((base_url, api_key))) => CliConfigSnapshot {
            cli_kind,
            configured: true,
            provider_id: match_provider(providers, cli_kind, &base_url, &api_key),
            modified_at,
            error_message: None,
        },
        Ok(None) => CliConfigSnapshot {
            cli_kind,
            configured: false,
            provider_id: None,
            modified_at,
            error_message: None,
        },
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Claude Code 配置文件格式无效")
        },
    }
}

pub(super) fn preview(
    cli_kind: AgentCliKind,
    provider: &Provider,
) -> Result<CliConfigPreview, String> {
    let (base_url, api_key) = cli_target(provider, cli_kind)?;
    let config_dir = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = config_dir.join("settings.json");
    let settings_text = read_cli_config(&settings_path, "读取 Claude Code 配置文件")?;
    let next_settings = rewrite_claude_config(&settings_text, &base_url, &api_key)?;
    Ok(CliConfigPreview {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.identity.name.clone(),
        cli_kind,
        revision: config_revision(&[&settings_text, &base_url, &api_key]),
        original_files: vec![config_file(&settings_path, settings_text)],
        files: vec![config_file(&settings_path, next_settings)],
    })
}

pub(super) fn switch(
    cli_kind: AgentCliKind,
    provider: &Provider,
    expected_revision: Option<&str>,
    files: &[CliConfigFile],
) -> Result<(), String> {
    let (base_url, api_key) = cli_target(provider, cli_kind)?;
    let config_dir = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = config_dir.join("settings.json");
    let settings_text = read_cli_config(&settings_path, "读取 Claude Code 配置文件")?;
    validate_file_set(files, &[&settings_path])?;
    ensure_revision(
        expected_revision,
        config_revision(&[&settings_text, &base_url, &api_key]),
    )?;
    let edited_settings = file_content(files, &settings_path)?;
    serde_json::from_str::<JsonValue>(edited_settings)
        .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
    write_config_text(&settings_path, edited_settings, "Claude Code 配置")
}

fn config_file(path: &std::path::Path, content: String) -> CliConfigFile {
    CliConfigFile {
        file_path: path.to_string_lossy().into_owned(),
        content,
    }
}

fn parse_claude_config(settings: &str) -> Result<Option<(String, String)>, ()> {
    let settings = serde_json::from_str::<JsonValue>(settings).map_err(|_| ())?;
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

fn rewrite_claude_config(
    settings: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let parsed = serde_json::from_str::<JsonValue>(settings)
        .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
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
