use super::super::config_support::{
    cli_target, config_error, config_revision, ensure_revision, file_content, latest_modified_at,
    match_provider, normalize_endpoint, read_optional_cli_config, read_stable_optional,
    restore_config_file, rewrite_json_string_fields, validate_file_set, write_config_text,
};
use crate::{
    models::{AgentCliKind, CliConfigFile, CliConfigPreview, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use chrono::{SecondsFormat, Utc};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use toml_edit::{value as toml_value, Document as TomlDocument, Item, Table};

const API_KEY_SCOPE: &str = "xai::api_key";

pub(super) fn config_dir() -> Option<PathBuf> {
    configured_path("BALANCEHUB_GROK_HOME")
        .or_else(|| configured_path("GROK_HOME"))
        .or_else(|| user_home().map(|home| home.join(".grok")))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(config_dir) = config_dir() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let config_path = config_dir.join("config.toml");
    let auth_path = config_dir.join("auth.json");
    let config = match read_stable_optional(&config_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Grok Build 配置文件失败"),
    };
    let auth = match read_stable_optional(&auth_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Grok Build 认证文件失败"),
    };
    let modified_at = latest_modified_at([config.as_ref(), auth.as_ref()]);
    let (Some(config), Some(auth)) = (config, auth) else {
        return CliConfigSnapshot {
            cli_kind,
            configured: false,
            provider_id: None,
            modified_at,
            error_message: None,
        };
    };
    match parse_grok_config(&config.text, &auth.text) {
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
            ..config_error(cli_kind, "Grok Build 配置文件格式无效")
        },
    }
}

pub(super) fn preview(
    cli_kind: AgentCliKind,
    provider: &Provider,
) -> Result<CliConfigPreview, String> {
    let (base_url, api_key) = cli_target(provider, cli_kind)?;
    let config_dir = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let config_path = config_dir.join("config.toml");
    let auth_path = config_dir.join("auth.json");
    let config_text = read_optional_cli_config(&config_path)?;
    let auth_text = read_optional_cli_config(&auth_path)?;
    let (next_config, next_auth) =
        rewrite_grok_config(&config_text, &auth_text, &base_url, &api_key)?;

    Ok(CliConfigPreview {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.identity.name.clone(),
        cli_kind,
        revision: config_revision(&[&config_text, &auth_text, &base_url, &api_key]),
        original_files: vec![
            config_file(&config_path, config_text),
            config_file(&auth_path, auth_text),
        ],
        files: vec![
            config_file(&config_path, next_config),
            config_file(&auth_path, next_auth),
        ],
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
    let config_path = config_dir.join("config.toml");
    let auth_path = config_dir.join("auth.json");
    let config = read_stable_optional(&config_path)?;
    let auth = read_stable_optional(&auth_path)?;
    let config_text = config
        .as_ref()
        .map(|file| file.text.as_str())
        .unwrap_or_default();
    let auth_text = auth
        .as_ref()
        .map(|file| file.text.as_str())
        .unwrap_or_default();
    validate_file_set(files, &[&config_path, &auth_path])?;
    ensure_revision(
        expected_revision,
        config_revision(&[config_text, auth_text, &base_url, &api_key]),
    )?;
    let edited_config = file_content(files, &config_path)?;
    let edited_auth = file_content(files, &auth_path)?;
    parse_grok_config(edited_config, edited_auth)
        .ok()
        .flatten()
        .ok_or_else(|| "Grok Build 配置必须包含有效的模型地址和 API Key 认证记录".to_string())?;

    write_config_text(&config_path, edited_config, "Grok Build 配置")?;
    if let Err(err) = write_config_text(&auth_path, edited_auth, "Grok Build 认证") {
        let rollback_error = restore_config_file(
            &config_path,
            config.as_ref().map(|file| file.text.as_str()),
            "Grok Build 配置回滚",
        )
        .err();
        return Err(match rollback_error {
            Some(rollback) => format!("{err}；{rollback}"),
            None => err,
        });
    }
    Ok(())
}

fn config_file(path: &Path, content: String) -> CliConfigFile {
    CliConfigFile {
        file_path: path.to_string_lossy().into_owned(),
        content,
    }
}

fn parse_grok_config(config: &str, auth: &str) -> Result<Option<(String, String)>, ()> {
    let config = config.parse::<toml::Value>().map_err(|_| ())?;
    let auth = serde_json::from_str::<JsonValue>(auth).map_err(|_| ())?;
    if !auth.is_object() {
        return Err(());
    }
    let base_url = config
        .get("endpoints")
        .and_then(|endpoints| endpoints.get("models_base_url"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if base_url.is_some_and(|base_url| normalize_endpoint(base_url).is_none()) {
        return Err(());
    }
    let Some(record) = auth.get(API_KEY_SCOPE) else {
        return Ok(None);
    };
    validate_api_key_record(record).map_err(|_| ())?;
    let api_key = record
        .get("key")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url.to_string(), api_key.to_string())),
        _ => None,
    })
}

fn rewrite_grok_config(
    config: &str,
    auth: &str,
    base_url: &str,
    api_key: &str,
) -> Result<(String, String), String> {
    let mut document = config
        .parse::<TomlDocument>()
        .map_err(|_| "Grok Build 配置文件格式无效".to_string())?;
    if !document.contains_key("endpoints") {
        document.insert("endpoints", Item::Table(Table::new()));
    }
    let endpoints = document
        .get_mut("endpoints")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| "Grok Build 配置中的 endpoints 必须是对象".to_string())?;
    endpoints.insert("models_base_url", toml_value(base_url.trim()));

    let auth_source = if auth.trim().is_empty() { "{}\n" } else { auth };
    let auth_value = serde_json::from_str::<JsonValue>(auth_source)
        .map_err(|_| "Grok Build 认证文件格式无效".to_string())?;
    let root = auth_value
        .as_object()
        .ok_or_else(|| "Grok Build 认证文件格式无效".to_string())?;
    let existing = match root.get(API_KEY_SCOPE) {
        Some(value) => Some(
            value
                .as_object()
                .ok_or_else(|| "Grok Build API Key 认证记录格式无效".to_string())?,
        ),
        None => None,
    };
    let create_time = existing
        .and_then(|record| record.get("create_time"))
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .ok_or_else(|| "Grok Build API Key 的 create_time 格式无效".to_string())
        })
        .transpose()?
        .unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true));
    let user_id = existing
        .and_then(|record| record.get("user_id"))
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "Grok Build API Key 的 user_id 格式无效".to_string())
        })
        .transpose()?
        .unwrap_or_default();
    let fields = [
        ("key", api_key.trim()),
        ("auth_mode", "api_key"),
        ("create_time", create_time.as_str()),
        ("user_id", user_id.as_str()),
    ];
    let rewritten_auth = rewrite_json_string_fields(auth_source, &[API_KEY_SCOPE], &fields)
        .map_err(|err| format!("生成 Grok Build 认证配置失败: {err}"))?;
    let rewritten_value = serde_json::from_str::<JsonValue>(&rewritten_auth)
        .map_err(|_| "Grok Build 认证文件格式无效".to_string())?;
    validate_api_key_record(
        rewritten_value
            .get(API_KEY_SCOPE)
            .ok_or_else(|| "Grok Build 认证文件缺少 API Key 记录".to_string())?,
    )?;

    Ok((document.to_string(), rewritten_auth))
}

fn validate_api_key_record(value: &JsonValue) -> Result<(), String> {
    let record = value
        .as_object()
        .ok_or_else(|| "Grok Build API Key 认证记录格式无效".to_string())?;
    let key = record
        .get("key")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Grok Build API Key 认证记录缺少 key".to_string())?;
    if key.chars().any(char::is_control) {
        return Err("Grok Build API Key 不能包含控制字符".to_string());
    }
    if record.get("auth_mode").and_then(JsonValue::as_str) != Some("api_key") {
        return Err("Grok Build API Key 认证记录的 auth_mode 必须是 api_key".to_string());
    }
    let create_time = record
        .get("create_time")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Grok Build API Key 认证记录缺少 create_time".to_string())?;
    chrono::DateTime::parse_from_rfc3339(create_time)
        .map_err(|_| "Grok Build API Key 认证记录的 create_time 格式无效".to_string())?;
    if record.get("user_id").and_then(JsonValue::as_str).is_none() {
        return Err("Grok Build API Key 认证记录缺少 user_id".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_endpoint_and_api_key_scope() {
        let parsed = parse_grok_config(
            "[endpoints]\nmodels_base_url = \"https://relay.example.com/v1\"\n",
            r#"{
              "xai::api_key": {
                "key": "xai-test",
                "auth_mode": "api_key",
                "create_time": "2026-08-14T08:00:00Z",
                "user_id": ""
              }
            }"#,
        )
        .expect("config should parse")
        .expect("config should be active");

        assert_eq!(parsed.0, "https://relay.example.com/v1");
        assert_eq!(parsed.1, "xai-test");
    }

    #[test]
    fn rewrite_preserves_unrelated_toml_and_auth_scopes() {
        let config = r#"[cli]
installer = "internal"

[ui]
screen_mode = "minimal"
"#;
        let auth = r#"{
  "team::oauth": {
    "key": "keep"
  },
  "xai::api_key": {
    "key": "old-key",
    "auth_mode": "api_key",
    "create_time": "2026-08-14T08:00:00Z",
    "user_id": "user-1",
    "email": "keep@example.com"
  }
}
"#;

        let (config, auth) = rewrite_grok_config(
            config,
            auth,
            "https://relay.example.com/v1",
            "new-key",
        )
        .unwrap();

        assert!(config.contains("installer = \"internal\""));
        assert!(config.contains("screen_mode = \"minimal\""));
        assert!(config.contains("models_base_url = \"https://relay.example.com/v1\""));
        let auth = serde_json::from_str::<JsonValue>(&auth).unwrap();
        assert_eq!(auth["team::oauth"]["key"], "keep");
        assert_eq!(auth[API_KEY_SCOPE]["key"], "new-key");
        assert_eq!(auth[API_KEY_SCOPE]["create_time"], "2026-08-14T08:00:00Z");
        assert_eq!(auth[API_KEY_SCOPE]["user_id"], "user-1");
        assert_eq!(auth[API_KEY_SCOPE]["email"], "keep@example.com");
    }

    #[test]
    fn rewrite_creates_minimal_official_files_without_a_default_model() {
        let (config, auth) = rewrite_grok_config(
            "[cli]\ninstaller = \"internal\"\n",
            "",
            "https://relay.example.com/v1",
            "new-key",
        )
        .unwrap();

        assert!(config.contains("[endpoints]"));
        assert!(!config.contains("[models]"));
        let auth = serde_json::from_str::<JsonValue>(&auth).unwrap();
        assert_eq!(auth[API_KEY_SCOPE]["key"], "new-key");
        assert_eq!(auth[API_KEY_SCOPE]["auth_mode"], "api_key");
        assert_eq!(auth[API_KEY_SCOPE]["user_id"], "");
        assert!(chrono::DateTime::parse_from_rfc3339(
            auth[API_KEY_SCOPE]["create_time"].as_str().unwrap()
        )
        .is_ok());
    }

    #[test]
    fn invalid_custom_endpoint_is_not_reported_as_an_active_config() {
        assert!(parse_grok_config(
            "[endpoints]\nmodels_base_url = \"file:///tmp/grok\"\n",
            r#"{
              "xai::api_key": {
                "key": "xai-test",
                "auth_mode": "api_key",
                "create_time": "2026-08-14T08:00:00Z",
                "user_id": ""
              }
            }"#,
        )
        .is_err());
    }
}
