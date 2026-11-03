use super::super::config_support::{
    cli_config_target, config_error, env_value, latest_modified_at, match_native_oauth,
    match_provider_key, oauth_snapshot, read_stable_optional, relay_snapshot,
    rewrite_env_values, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, OAuthVendor, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

pub(super) fn managed_root() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/GeminiCli")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\ProgramData\gemini-cli")
    } else {
        PathBuf::from("/etc/gemini-cli")
    }
}

pub(super) fn configured_system_file(name: &str, default: PathBuf, workdir: &Path) -> PathBuf {
    configured_path(name)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                workdir.join(path)
            }
        })
        .unwrap_or(default)
}

pub(crate) fn merge_temporary_system_settings(
    workdir: &Path,
    overrides: JsonValue,
) -> Result<JsonValue, String> {
    let path = configured_system_file(
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        managed_root().join("settings.json"),
        workdir,
    );
    let mut base = read_stable_optional(&path)?
        .map(|file| super::super::config_support::parse_jsonc_document(&file.text))
        .transpose()?
        .unwrap_or_else(|| serde_json::json!({}));
    if !base.is_object() {
        return Err("Gemini 系统设置必须是 JSON 对象".into());
    }
    if let (Some(enforced), Some(selected)) = (
        base.pointer("/security/auth/enforcedType")
            .and_then(JsonValue::as_str),
        overrides
            .pointer("/security/auth/selectedType")
            .and_then(JsonValue::as_str),
    ) {
        if enforced != selected {
            return Err("Gemini 的组织策略要求使用其他认证方式，无法通过当前中转站启动".into());
        }
    }
    let mut overrides = overrides;
    for key in ["/skills/disabled", "/mcp/excluded"] {
        if let (Some(original), Some(selected)) = (
            base.pointer(key).and_then(JsonValue::as_array),
            overrides.pointer_mut(key).and_then(JsonValue::as_array_mut),
        ) {
            for value in original {
                if !selected.contains(value) {
                    selected.push(value.clone());
                }
            }
        }
    }
    merge_settings(&mut base, overrides);
    Ok(base)
}

fn merge_settings(base: &mut JsonValue, overrides: JsonValue) {
    if let (Some(base), Some(overrides)) = (base.as_object_mut(), overrides.as_object()) {
        for (key, value) in overrides {
            merge_settings(
                base.entry(key.clone()).or_insert(JsonValue::Null),
                value.clone(),
            );
        }
    } else {
        *base = overrides;
    }
}

pub(super) fn config_dir() -> Option<PathBuf> {
    configured_config_dir().or_else(|| user_home().map(|home| home.join(".gemini")))
}

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_config_dir().unwrap_or_else(|| home.join(".gemini"))
}

fn configured_config_dir() -> Option<PathBuf> {
    configured_path("BALANCEHUB_GEMINI_CONFIG_DIR")
        .or_else(|| configured_path("GEMINI_CLI_HOME").map(|home| home.join(".gemini")))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(config_dir) = config_dir() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let settings_path = config_dir.join("settings.json");
    let env_path = config_dir.join(".env");
    let credentials_path = config_dir.join("oauth_creds.json");
    let settings = match read_stable_optional(&settings_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Gemini CLI 配置文件失败"),
    };
    let env = match read_stable_optional(&env_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Gemini CLI 环境配置失败"),
    };
    // The native login cache is only evidence for the personal-login case; an
    // unreadable cache must not hide a working relay configuration.
    let credentials = read_stable_optional(&credentials_path).ok().flatten();
    let modified_at =
        latest_modified_at([settings.as_ref(), env.as_ref(), credentials.as_ref()]);
    let native_oauth = credentials
        .as_ref()
        .and_then(|file| match_native_oauth(providers, OAuthVendor::Gemini, &file.text));
    let Some(settings) = settings else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    let env = env.as_ref().map(|file| file.text.as_str()).unwrap_or("");
    match parse_gemini_config(&settings.text, env) {
        Ok(Some((base_url, api_key))) => {
            let matched = match_provider_key(providers, cli_kind, &base_url, &api_key);
            relay_snapshot(cli_kind, matched, modified_at)
        }
        // The personal login only counts when it is the selected auth type; a
        // leftover cache file must not look active.
        Ok(None) => oauth_snapshot(
            cli_kind,
            if gemini_selected_type(&settings.text).ok().flatten().as_deref()
                == Some("oauth-personal")
            {
                native_oauth
            } else {
                None
            },
            modified_at,
        ),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Gemini CLI 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let root = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = root.join("settings.json");
    let env_path = root.join(".env");
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey {
            base_url, api_key, ..
        } => {
            let settings = read_stable_optional(&settings_path)?.map(|value| value.text);
            let env = read_stable_optional(&env_path)?.map(|value| value.text);
            let settings_source = settings
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or("{}\n");
            let next_settings = rewrite_gemini_settings(settings_source, "gemini-api-key")?;
            let next_env = rewrite_gemini_env(env.as_deref().unwrap_or(""), &base_url, &api_key)?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: settings_path,
                    before: settings,
                    after: next_settings,
                },
                ProviderConfigurationCandidate {
                    path: env_path,
                    before: env,
                    after: next_env,
                },
            ])
        }
        CliConfigTarget::OAuth { auth, identity } => {
            let credentials_path = root.join("oauth_creds.json");
            let credentials = read_stable_optional(&credentials_path)?.map(|value| value.text);
            let settings = read_stable_optional(&settings_path)?.map(|value| value.text);
            let env = read_stable_optional(&env_path)?.map(|value| value.text);
            let settings_source = settings
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or("{}\n");
            let mut candidates = Vec::new();
            let next_settings = rewrite_gemini_settings(settings_source, "oauth-personal")?;
            if settings.as_deref() != Some(next_settings.as_str()) {
                candidates.push(ProviderConfigurationCandidate {
                    path: settings_path,
                    before: settings,
                    after: next_settings,
                });
            }
            // The official personal login uses Google's own endpoint, so relay
            // overrides in `.env` have to go with the key.
            if let Some(env) = env.as_deref() {
                let next_env = crate::services::agent_cli::config_support::remove_env_keys(
                    env,
                    &["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GEMINI_BASE_URL"],
                );
                if next_env != env {
                    candidates.push(ProviderConfigurationCandidate {
                        path: env_path.clone(),
                        before: Some(env.to_string()),
                        after: next_env,
                    });
                }
            }
            candidates.push(ProviderConfigurationCandidate {
                path: credentials_path,
                before: credentials,
                after: gemini_oauth_document(auth, identity)?,
            });
            Ok(candidates)
        }
    }
}

fn gemini_oauth_document(
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
        .map_err(|err| format!("生成 Gemini 授权缓存失败: {err}"))
}

fn parse_gemini_config(settings: &str, env: &str) -> Result<Option<(String, String)>, ()> {
    if gemini_selected_type(settings)?.as_deref() != Some("gemini-api-key") {
        return Ok(None);
    }
    let base_url = env_value(env, "GOOGLE_GEMINI_BASE_URL");
    let api_key = env_value(env, "GEMINI_API_KEY");
    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url, api_key)),
        _ => None,
    })
}

fn gemini_selected_type(settings: &str) -> Result<Option<String>, ()> {
    let settings = crate::services::agent_cli::config_support::parse_jsonc_document(settings)
        .map_err(|_| ())?;
    if !settings.is_object() {
        return Err(());
    }
    Ok(settings
        .get("security")
        .and_then(|security| security.get("auth"))
        .and_then(|auth| auth.get("selectedType"))
        .and_then(JsonValue::as_str)
        .map(str::to_string))
}

fn rewrite_gemini_settings(settings: &str, auth_type: &str) -> Result<String, String> {
    let mut value = crate::services::agent_cli::config_support::parse_jsonc_document(settings)?;
    let invalid = || "Gemini security.auth 必须是对象".to_owned();
    let root = value.as_object_mut().ok_or_else(invalid)?;
    let security = root
        .entry("security")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(invalid)?;
    let auth = security
        .entry("auth")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(invalid)?;
    auth.insert(
        "selectedType".to_owned(),
        JsonValue::String(auth_type.to_owned()),
    );
    crate::services::agent_cli::config_support::rewrite_json_document(settings, &value, true)
        .map_err(|_| "生成 Gemini CLI 配置失败".to_owned())
}

fn rewrite_gemini_env(source: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    rewrite_env_values(
        source,
        &[
            ("GEMINI_API_KEY", api_key),
            ("GOOGLE_GEMINI_BASE_URL", base_url),
        ],
    )
    .map_err(|err| format!("生成 Gemini CLI 环境配置失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_settings_and_env() {
        let parsed = parse_gemini_config(
            r#"{"security":{"auth":{"selectedType":"gemini-api-key"}}}"#,
            "GEMINI_API_KEY=gemini-test\nGOOGLE_GEMINI_BASE_URL=https://relay.example.com/gemini\n",
        )
        .expect("config should parse")
        .expect("config should be complete");

        assert_eq!(parsed.0, "https://relay.example.com/gemini");
        assert_eq!(parsed.1, "gemini-test");
    }

    #[test]
    fn config_is_inactive_when_api_key_auth_is_not_selected() {
        assert_eq!(
            parse_gemini_config(
                r#"{"security":{"auth":{"selectedType":"oauth-personal"}}}"#,
                "GEMINI_API_KEY=gemini-test\nGOOGLE_GEMINI_BASE_URL=https://relay.example.com\n",
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn settings_rewrite_preserves_layout_and_other_fields() {
        let settings = r#"{
  "theme": "dark",
  "security": {
    "auth": {
      "selectedType": "oauth-personal",
      "useExternal": false
    }
  }
}"#;

        let rewritten = rewrite_gemini_settings(settings, "gemini-api-key").unwrap();

        assert_eq!(
            rewritten,
            r#"{
  "theme": "dark",
  "security": {
    "auth": {
      "selectedType": "gemini-api-key",
      "useExternal": false
    }
  }
}"#
        );
    }

    #[test]
    fn settings_rewrite_adds_nested_auth_without_reformatting_root() {
        let rewritten = rewrite_gemini_settings(r#"{"theme":"dark"}"#, "gemini-api-key").unwrap();

        assert_eq!(
            rewritten,
            r#"{"theme":"dark", "security": {"auth": {"selectedType": "gemini-api-key"}}}"#
        );
    }

    #[test]
    fn settings_rewrite_uses_existing_multiline_indentation() {
        let rewritten = rewrite_gemini_settings("{\r\n    \"theme\": \"dark\"\r\n}", "gemini-api-key").unwrap();

        assert_eq!(
            rewritten,
            "{\r\n    \"theme\": \"dark\",\r\n    \"security\": {\r\n        \"auth\": {\r\n            \"selectedType\": \"gemini-api-key\"\r\n        }\r\n    }\r\n}"
        );
    }

    #[test]
    fn env_rewrite_preserves_comments_order_and_unrelated_values() {
        let env = "# Gemini\nKEEP_ME=yes\nexport GEMINI_API_KEY='old-key' # auth\nGOOGLE_GEMINI_BASE_URL=https://old.example.com\n";

        let rewritten =
            rewrite_gemini_env(env, "https://new.example.com/gemini", "new-key").unwrap();

        assert_eq!(
            rewritten,
            "# Gemini\nKEEP_ME=yes\nexport GEMINI_API_KEY='new-key' # auth\nGOOGLE_GEMINI_BASE_URL=https://new.example.com/gemini\n"
        );
        assert_eq!(
            (
                env_value(&rewritten, "GOOGLE_GEMINI_BASE_URL"),
                env_value(&rewritten, "GEMINI_API_KEY"),
            ),
            (
                Some("https://new.example.com/gemini".to_string()),
                Some("new-key".to_string()),
            )
        );
    }

    #[test]
    fn env_rewrite_adds_only_missing_assignments() {
        let rewritten =
            rewrite_gemini_env("KEEP_ME=yes", "https://new.example.com/gemini", "new-key").unwrap();

        assert_eq!(
            rewritten,
            "KEEP_ME=yes\nGEMINI_API_KEY=new-key\nGOOGLE_GEMINI_BASE_URL=https://new.example.com/gemini\n"
        );
    }

    #[test]
    fn env_rewrite_quotes_values_that_contain_comments_or_spaces() {
        let rewritten = rewrite_gemini_env(
            "",
            "https://new.example.com/gemini path#v1",
            "key#with-comment",
        )
        .unwrap();

        assert_eq!(
            (
                env_value(&rewritten, "GOOGLE_GEMINI_BASE_URL"),
                env_value(&rewritten, "GEMINI_API_KEY"),
            ),
            (
                Some("https://new.example.com/gemini path#v1".to_string()),
                Some("key#with-comment".to_string()),
            )
        );
    }
}
