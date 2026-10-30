use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_native_oauth, match_provider_key,
    oauth_snapshot, read_cli_config, read_stable_optional, relay_snapshot, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use crate::services::agent_cli::config_support::text_document;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, OAuthVendor, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use toml_edit::{value as toml_value, Document as TomlDocument};

pub(super) fn config_dir() -> Option<PathBuf> {
    configured_config_dir().or_else(|| user_home().map(|home| home.join(".codex")))
}

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_config_dir().unwrap_or_else(|| home.join(".codex"))
}

fn configured_config_dir() -> Option<PathBuf> {
    configured_path("BALANCEHUB_CODEX_HOME").or_else(|| configured_path("CODEX_HOME"))
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
    // A login without `config.toml` still counts: Codex reads `auth.json` on
    // its own, and the relay route simply is not set.
    let Some(auth) = auth else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    let config = config.map(|file| file.text).unwrap_or_default();
    match parse_codex_config(&config, &auth.text) {
        Ok(Some((base_url, api_key))) => {
            let matched = match_provider_key(providers, cli_kind, &base_url, &api_key);
            relay_snapshot(cli_kind, matched, modified_at)
        }
        Ok(None) => oauth_snapshot(
            cli_kind,
            match_native_oauth(providers, OAuthVendor::Codex, &auth.text),
            modified_at,
        ),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Codex 配置文件格式无效")
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
            let config_path = root.join("config.toml");
            let auth_path = root.join("auth.json");
            let config_text = read_cli_config(&config_path, "读取 Codex 配置文件")?;
            let auth_text = read_cli_config(&auth_path, "读取 Codex 认证文件")?;
            let (config, auth) =
                rewrite_codex_config(&config_text, &auth_text, &base_url, &api_key)?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: config_path,
                    before: Some(config_text),
                    after: config,
                },
                ProviderConfigurationCandidate {
                    path: auth_path,
                    before: Some(auth_text),
                    after: auth,
                },
            ])
        }
        CliConfigTarget::OAuth { auth, identity } => {
            let root = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
            let config_path = root.join("config.toml");
            let auth_path = root.join("auth.json");
            let config_text = read_stable_optional(&config_path)?.map(|file| file.text);
            let auth_text = read_stable_optional(&auth_path)?.map(|file| file.text);
            let mut candidates = Vec::new();
            if let Some(config) = config_text.as_deref() {
                let next = rewrite_codex_official_config(config)?;
                if next != config {
                    candidates.push(ProviderConfigurationCandidate {
                        path: config_path,
                        before: Some(config.to_string()),
                        after: next,
                    });
                }
            }
            candidates.push(ProviderConfigurationCandidate {
                path: auth_path,
                before: auth_text,
                after: codex_oauth_document(auth, identity)?,
            });
            Ok(candidates)
        }
    }
}

/// Switching to a ChatGPT login removes the relay route the API key used: the
/// built-in `openai` provider is what reads `auth.json`.
fn rewrite_codex_official_config(config: &str) -> Result<String, String> {
    let mut document = text_document::body(config)
        .parse::<TomlDocument>()
        .map_err(|_| "Codex 配置文件格式无效".to_string())?;
    let store = document
        .get("cli_auth_credentials_store")
        .and_then(toml_edit::Item::as_str);
    if let Some(store) = store {
        if store != "file" {
            return Err(format!(
                "Codex 设置了 cli_auth_credentials_store = \"{store}\"，登录不会保存在 auth.json；请先在 Codex 配置中改回 file 再切换"
            ));
        }
    }
    document.remove("model_provider");
    document.remove("openai_base_url");
    Ok(text_document::restore_layout(config, &document.to_string()))
}

fn codex_oauth_document(
    auth: &crate::models::ProviderAuth,
    identity: &crate::models::OAuthCredential,
) -> Result<String, String> {
    let document = crate::services::oauth_accounts::native::document(
        identity.vendor,
        auth,
        identity,
        true,
    );
    serde_json::to_string_pretty(&document).map_err(|err| format!("生成 Codex 认证文件失败: {err}"))
}

fn parse_codex_config(config: &str, auth: &str) -> Result<Option<(String, String)>, ()> {
    let config = text_document::body(config)
        .parse::<toml::Value>()
        .map_err(|_| ())?;
    let auth = crate::services::agent_cli::environment::config_document::parse(
        auth.as_bytes(),
        crate::services::agent_cli::environment::config_document::ConfigDocumentFormat::Json,
    )
    .ok_or(())?;
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
    let mut document = text_document::body(config)
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

    let auth = crate::services::agent_cli::config_support::rewrite_json_string_fields(
        auth,
        &[],
        &[("OPENAI_API_KEY", api_key.trim())],
    )
    .map_err(|_| "Codex 认证文件格式无效".to_owned())?;
    Ok((
        text_document::restore_layout(config, &document.to_string()),
        auth,
    ))
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
