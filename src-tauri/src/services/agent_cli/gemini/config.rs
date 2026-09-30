use super::super::config_support::{
    cli_target_for_key, config_error, env_value, latest_modified_at, match_provider_key,
    read_stable_optional, rewrite_env_values,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

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
    let settings = match read_stable_optional(&settings_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Gemini CLI 配置文件失败"),
    };
    let env = match read_stable_optional(&env_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Gemini CLI 环境配置失败"),
    };
    let modified_at = latest_modified_at([settings.as_ref(), env.as_ref()]);
    let Some(settings) = settings else {
        return CliConfigSnapshot {
            cli_kind,
            configured: false,
            provider_id: None,
            api_key_local_id: None,
            modified_at,
            error_message: None,
        };
    };
    let env = env.as_ref().map(|file| file.text.as_str()).unwrap_or("");
    match parse_gemini_config(&settings.text, env) {
        Ok(Some((base_url, api_key))) => {
            let matched = match_provider_key(providers, cli_kind, &base_url, &api_key);
            CliConfigSnapshot {
                cli_kind,
                configured: true,
                provider_id: matched.as_ref().map(|value| value.provider_id.clone()),
                api_key_local_id: matched.map(|value| value.api_key_local_id),
                modified_at,
                error_message: None,
            }
        }
        Ok(None) => CliConfigSnapshot {
            cli_kind,
            configured: false,
            provider_id: None,
            api_key_local_id: None,
            modified_at,
            error_message: None,
        },
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
    let target = cli_target_for_key(provider, cli_kind, api_key_local_id)?;
    let root = config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = root.join("settings.json");
    let env_path = root.join(".env");
    let settings = read_stable_optional(&settings_path)?.map(|value| value.text);
    let env = read_stable_optional(&env_path)?.map(|value| value.text);
    let settings_source = settings
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("{}\n");
    let next_settings = rewrite_gemini_settings(settings_source)?;
    let next_env = rewrite_gemini_env(
        env.as_deref().unwrap_or(""),
        &target.base_url,
        &target.api_key,
    )?;
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

fn parse_gemini_config(settings: &str, env: &str) -> Result<Option<(String, String)>, ()> {
    let settings = crate::services::agent_cli::config_support::parse_jsonc_document(settings)
        .map_err(|_| ())?;
    if !settings.is_object() {
        return Err(());
    }
    let selected_type = settings
        .get("security")
        .and_then(|security| security.get("auth"))
        .and_then(|auth| auth.get("selectedType"))
        .and_then(JsonValue::as_str);
    if selected_type != Some("gemini-api-key") {
        return Ok(None);
    }
    let base_url = env_value(env, "GOOGLE_GEMINI_BASE_URL");
    let api_key = env_value(env, "GEMINI_API_KEY");
    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url, api_key)),
        _ => None,
    })
}

fn rewrite_gemini_settings(settings: &str) -> Result<String, String> {
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
        JsonValue::String("gemini-api-key".to_owned()),
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

        let rewritten = rewrite_gemini_settings(settings).unwrap();

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
        let rewritten = rewrite_gemini_settings(r#"{"theme":"dark"}"#).unwrap();

        assert_eq!(
            rewritten,
            r#"{"theme":"dark", "security": {"auth": {"selectedType": "gemini-api-key"}}}"#
        );
    }

    #[test]
    fn settings_rewrite_uses_existing_multiline_indentation() {
        let rewritten = rewrite_gemini_settings("{\r\n    \"theme\": \"dark\"\r\n}").unwrap();

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
