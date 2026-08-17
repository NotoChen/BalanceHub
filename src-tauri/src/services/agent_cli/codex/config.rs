use super::super::config_support::{
    cli_target, config_error, config_revision, ensure_revision, file_content, latest_modified_at,
    match_provider, read_cli_config, read_stable_optional, validate_file_set, write_config_text,
};
use crate::{
    models::{AgentCliKind, CliConfigFile, CliConfigPreview, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::PathBuf;
use toml_edit::{value as toml_value, Document as TomlDocument};

pub(super) fn config_dir() -> Option<PathBuf> {
    configured_path("BALANCEHUB_CODEX_HOME")
        .or_else(|| configured_path("CODEX_HOME"))
        .or_else(|| user_home().map(|home| home.join(".codex")))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(config_dir) = config_dir() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let config_path = config_dir.join("config.toml");
    let auth_path = config_dir.join("auth.json");
    let config = match read_stable_optional(&config_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Codex 配置文件失败"),
    };
    let auth = match read_stable_optional(&auth_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Codex 认证文件失败"),
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
    match parse_codex_config(&config.text, &auth.text) {
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
            ..config_error(cli_kind, "Codex 配置文件格式无效")
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
    let config_text = read_cli_config(&config_path, "读取 Codex 配置文件")?;
    let auth_text = read_cli_config(&auth_path, "读取 Codex 认证文件")?;
    let (next_config, next_auth) =
        rewrite_codex_config(&config_text, &auth_text, &base_url, &api_key)?;
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
    let config_text = read_cli_config(&config_path, "读取 Codex 配置文件")?;
    let auth_text = read_cli_config(&auth_path, "读取 Codex 认证文件")?;
    validate_file_set(files, &[&config_path, &auth_path])?;
    ensure_revision(
        expected_revision,
        config_revision(&[&config_text, &auth_text, &base_url, &api_key]),
    )?;
    let edited_config = file_content(files, &config_path)?;
    let edited_auth = file_content(files, &auth_path)?;
    let config_document = edited_config
        .parse::<TomlDocument>()
        .map_err(|_| "Codex 配置文件格式无效".to_string())?;
    let auth = serde_json::from_str::<JsonValue>(edited_auth)
        .map_err(|_| "Codex 认证文件格式无效".to_string())?;
    let next_config = config_document.to_string();
    let next_auth = serde_json::to_string_pretty(&auth)
        .map_err(|err| format!("生成 Codex 认证配置失败: {err}"))?
        + "\n";

    write_config_text(&config_path, &next_config, "Codex 配置")?;
    if let Err(err) = write_config_text(&auth_path, &next_auth, "Codex 认证") {
        let rollback_error = write_config_text(&config_path, &config_text, "Codex 配置回滚").err();
        return Err(match rollback_error {
            Some(rollback) => format!("{err}；{rollback}"),
            None => err,
        });
    }
    Ok(())
}

fn config_file(path: &std::path::Path, content: String) -> CliConfigFile {
    CliConfigFile {
        file_path: path.to_string_lossy().into_owned(),
        content,
    }
}

fn parse_codex_config(config: &str, auth: &str) -> Result<Option<(String, String)>, ()> {
    let config = config.parse::<toml::Value>().map_err(|_| ())?;
    let auth = serde_json::from_str::<JsonValue>(auth).map_err(|_| ())?;
    let provider_name = config.get("model_provider").and_then(toml::Value::as_str);
    let base_url = provider_name
        .and_then(|name| config.get("model_providers")?.get(name))
        .and_then(|provider| provider.get("base_url"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = auth
        .get("OPENAI_API_KEY")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url.to_string(), api_key.to_string())),
        _ => None,
    })
}

fn rewrite_codex_config(
    config: &str,
    auth: &str,
    base_url: &str,
    api_key: &str,
) -> Result<(String, String), String> {
    let mut document = config
        .parse::<TomlDocument>()
        .map_err(|_| "Codex 配置文件格式无效".to_string())?;
    let provider_name = document
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Codex 配置缺少 model_provider，无法只更新中转站地址".to_string())?;
    let providers = document
        .get_mut("model_providers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| "Codex 配置缺少 model_providers".to_string())?;
    let selected = providers
        .get_mut(&provider_name)
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| format!("Codex 配置缺少当前 provider：{provider_name}"))?;
    selected.insert("base_url", toml_value(base_url.trim()));

    let mut auth = serde_json::from_str::<JsonValue>(auth)
        .map_err(|_| "Codex 认证文件格式无效".to_string())?;
    let auth = auth
        .as_object_mut()
        .ok_or_else(|| "Codex 认证文件格式无效".to_string())?;
    auth.insert(
        "OPENAI_API_KEY".to_string(),
        JsonValue::String(api_key.trim().to_string()),
    );
    let auth = serde_json::to_string_pretty(auth)
        .map_err(|err| format!("生成 Codex 认证配置失败: {err}"))?;

    Ok((document.to_string(), format!("{auth}\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_selected_provider_and_auth_file() {
        let parsed = parse_codex_config(
            r#"
model_provider = "relay"

[model_providers.relay]
base_url = "https://relay.example.com/v1"
"#,
            r#"{"OPENAI_API_KEY":"sk-test"}"#,
        )
        .expect("config should parse")
        .expect("config should be complete");

        assert_eq!(parsed.0, "https://relay.example.com/v1");
        assert_eq!(parsed.1, "sk-test");
    }

    #[test]
    fn rewrite_only_updates_selected_provider_url_and_api_key() {
        let config = r#"model_provider = "relay"
model = "gpt-test"

[model_providers.relay]
name = "Relay"
base_url = "https://old.example.com/v1"
wire_api = "responses"

[mcp_servers.local]
command = "node"
"#;
        let auth = r#"{"OPENAI_API_KEY":"sk-old","tokens":{"access":"keep"}}"#;

        let (config, auth) =
            rewrite_codex_config(config, auth, "https://new.example.com/v1", "sk-new").unwrap();

        assert!(config.contains("base_url = \"https://new.example.com/v1\""));
        assert!(config.contains("model = \"gpt-test\""));
        assert!(config.contains("[mcp_servers.local]"));
        let auth = serde_json::from_str::<JsonValue>(&auth).unwrap();
        assert_eq!(auth["OPENAI_API_KEY"], "sk-new");
        assert_eq!(auth["tokens"]["access"], "keep");
    }
}
