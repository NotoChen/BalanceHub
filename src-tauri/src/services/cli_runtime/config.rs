use super::{home_dir, latest_modified_at, read_stable_optional};

mod formats;
#[cfg(test)]
mod tests;
use crate::{
    limits,
    models::{
        normalize_api_key_for_protocol, CliConfigChange, CliConfigPreview, CliConfigSnapshot,
        LivenessCliKind, Provider,
    },
    services::liveness::{anthropic_base_url, openai_base_url},
    util::read_text_file_limited,
};
use serde_json::Value as JsonValue;
use std::hash::{Hash, Hasher};
use toml_edit::Document as TomlDocument;

use formats::{
    parse_claude_config, parse_codex_config, rewrite_claude_config, rewrite_codex_config,
    write_config_text,
};

pub(super) fn codex_config_snapshot(providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = home_dir() else {
        return config_error("无法定位用户目录");
    };
    let config_path = home.join(".codex").join("config.toml");
    let auth_path = home.join(".codex").join("auth.json");
    let config = match read_stable_optional(&config_path) {
        Ok(value) => value,
        Err(_) => return config_error("读取 Codex 配置文件失败"),
    };
    let auth = match read_stable_optional(&auth_path) {
        Ok(value) => value,
        Err(_) => return config_error("读取 Codex 认证文件失败"),
    };
    let modified_at = latest_modified_at([config.as_ref(), auth.as_ref()]);
    let (Some(config), Some(auth)) = (config, auth) else {
        return CliConfigSnapshot {
            modified_at,
            ..CliConfigSnapshot::default()
        };
    };

    match parse_codex_config(&config.text, &auth.text) {
        Ok(Some((base_url, api_key))) => CliConfigSnapshot {
            configured: true,
            provider_id: match_provider(providers, LivenessCliKind::Codex, &base_url, &api_key),
            modified_at,
            error_message: None,
        },
        Ok(None) => CliConfigSnapshot {
            modified_at,
            ..CliConfigSnapshot::default()
        },
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error("Codex 配置文件格式无效")
        },
    }
}

pub(super) fn claude_config_snapshot(providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = home_dir() else {
        return config_error("无法定位用户目录");
    };
    let settings_path = home.join(".claude").join("settings.json");
    let settings = match read_stable_optional(&settings_path) {
        Ok(value) => value,
        Err(_) => return config_error("读取 Claude Code 配置文件失败"),
    };
    let modified_at = latest_modified_at([settings.as_ref()]);
    let Some(settings) = settings else {
        return CliConfigSnapshot {
            modified_at,
            ..CliConfigSnapshot::default()
        };
    };

    match parse_claude_config(&settings.text) {
        Ok(Some((base_url, api_key))) => CliConfigSnapshot {
            configured: true,
            provider_id: match_provider(
                providers,
                LivenessCliKind::ClaudeCode,
                &base_url,
                &api_key,
            ),
            modified_at,
            error_message: None,
        },
        Ok(None) => CliConfigSnapshot {
            modified_at,
            ..CliConfigSnapshot::default()
        },
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error("Claude Code 配置文件格式无效")
        },
    }
}

pub fn preview_config(
    provider: &Provider,
    cli_kind: LivenessCliKind,
) -> Result<CliConfigPreview, String> {
    let (base_url, api_key) = cli_target(provider, cli_kind)?;
    let home = home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let mut changes = Vec::new();

    let revision = match cli_kind {
        LivenessCliKind::Codex => {
            let config_path = home.join(".codex").join("config.toml");
            let auth_path = home.join(".codex").join("auth.json");
            let config_text = read_cli_config(&config_path, "读取 Codex 配置文件")?;
            let auth_text = read_cli_config(&auth_path, "读取 Codex 认证文件")?;
            let provider_document = config_text
                .parse::<TomlDocument>()
                .map_err(|_| "Codex 配置文件格式无效".to_string())?;
            let provider_name = codex_provider_name(&provider_document)?;
            let before_url = provider_document
                .get("model_providers")
                .and_then(toml_edit::Item::as_table_like)
                .and_then(|providers| providers.get(&provider_name))
                .and_then(toml_edit::Item::as_table_like)
                .and_then(|selected| selected.get("base_url"))
                .and_then(toml_edit::Item::as_str);
            let auth_document = serde_json::from_str::<JsonValue>(&auth_text)
                .map_err(|_| "Codex 认证文件格式无效".to_string())?;
            let before_key = auth_document
                .get("OPENAI_API_KEY")
                .and_then(JsonValue::as_str);
            push_config_change(
                &mut changes,
                config_path.to_string_lossy().as_ref(),
                &format!("model_providers.{provider_name}.base_url"),
                before_url,
                Some(base_url.as_str()),
                false,
            );
            push_config_change(
                &mut changes,
                auth_path.to_string_lossy().as_ref(),
                "OPENAI_API_KEY",
                before_key,
                Some(api_key.as_str()),
                true,
            );
            config_revision(&[&config_text, &auth_text, &base_url, &api_key])
        }
        LivenessCliKind::ClaudeCode => {
            let settings_path = home.join(".claude").join("settings.json");
            let settings_text = read_cli_config(&settings_path, "读取 Claude Code 配置文件")?;
            let settings = serde_json::from_str::<JsonValue>(&settings_text)
                .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
            let env = settings.get("env").and_then(JsonValue::as_object);
            let next_settings = rewrite_claude_config(&settings_text, &base_url, &api_key)?;
            let next = serde_json::from_str::<JsonValue>(&next_settings)
                .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
            let next_env = next.get("env").and_then(JsonValue::as_object);
            for (field, sensitive) in [
                ("ANTHROPIC_BASE_URL", false),
                ("ANTHROPIC_AUTH_TOKEN", true),
                ("ANTHROPIC_API_KEY", true),
            ] {
                let before = env
                    .and_then(|values| values.get(field))
                    .and_then(JsonValue::as_str);
                let after = next_env
                    .and_then(|values| values.get(field))
                    .and_then(JsonValue::as_str);
                push_config_change(
                    &mut changes,
                    settings_path.to_string_lossy().as_ref(),
                    &format!("env.{field}"),
                    before,
                    after,
                    sensitive,
                );
            }
            config_revision(&[&settings_text, &base_url, &api_key])
        }
    };

    Ok(CliConfigPreview {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.identity.name.clone(),
        cli_kind,
        revision,
        changes,
    })
}

pub fn switch_config(
    provider: &Provider,
    cli_kind: LivenessCliKind,
    expected_revision: Option<&str>,
) -> Result<(), String> {
    let (base_url, api_key) = cli_target(provider, cli_kind)?;
    match cli_kind {
        LivenessCliKind::Codex => switch_codex_config(&base_url, &api_key, expected_revision),
        LivenessCliKind::ClaudeCode => switch_claude_config(&base_url, &api_key, expected_revision),
    }
}

fn cli_target(provider: &Provider, cli_kind: LivenessCliKind) -> Result<(String, String), String> {
    let api_key =
        normalize_api_key_for_protocol(&provider.auth.api_key, provider.identity.protocol);
    if api_key.is_empty() {
        return Err("中转站缺少 API Key，无法切换 CLI 配置".to_string());
    }

    let base_url = match cli_kind {
        LivenessCliKind::Codex => openai_base_url(provider),
        LivenessCliKind::ClaudeCode => anthropic_base_url(provider),
    };
    if normalize_endpoint(&base_url).is_none() {
        return Err("中转站地址无效，无法切换 CLI 配置".to_string());
    }
    Ok((base_url, api_key))
}

fn switch_codex_config(
    base_url: &str,
    api_key: &str,
    expected_revision: Option<&str>,
) -> Result<(), String> {
    let home = home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let config_path = home.join(".codex").join("config.toml");
    let auth_path = home.join(".codex").join("auth.json");
    let config_text = read_cli_config(&config_path, "读取 Codex 配置文件")?;
    let auth_text = read_cli_config(&auth_path, "读取 Codex 认证文件")?;
    ensure_revision(
        expected_revision,
        config_revision(&[&config_text, &auth_text, base_url, api_key]),
    )?;
    let (next_config, next_auth) =
        rewrite_codex_config(&config_text, &auth_text, base_url, api_key)?;

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

fn switch_claude_config(
    base_url: &str,
    api_key: &str,
    expected_revision: Option<&str>,
) -> Result<(), String> {
    let home = home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = home.join(".claude").join("settings.json");
    let settings_text = read_cli_config(&settings_path, "读取 Claude Code 配置文件")?;
    ensure_revision(
        expected_revision,
        config_revision(&[&settings_text, base_url, api_key]),
    )?;
    let next_settings = rewrite_claude_config(&settings_text, base_url, api_key)?;
    write_config_text(&settings_path, &next_settings, "Claude Code 配置")
}

fn codex_provider_name(document: &TomlDocument) -> Result<String, String> {
    document
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Codex 配置缺少 model_provider，无法只更新中转站地址".to_string())
}

fn read_cli_config(path: &std::path::Path, context: &str) -> Result<String, String> {
    read_text_file_limited(path, limits::MAX_CLI_CONFIG_FILE_BYTES, context)
}

fn push_config_change(
    changes: &mut Vec<CliConfigChange>,
    file_path: &str,
    field_path: &str,
    before: Option<&str>,
    after: Option<&str>,
    sensitive: bool,
) {
    if before == after {
        return;
    }
    changes.push(CliConfigChange {
        file_path: file_path.to_string(),
        field_path: field_path.to_string(),
        before_value: before.map(str::to_string),
        after_value: after.map(str::to_string),
        sensitive,
    });
}

fn config_revision(parts: &[&str]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        part.len().hash(&mut hasher);
        part.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn ensure_revision(expected: Option<&str>, actual: String) -> Result<(), String> {
    if let Some(expected) = expected.filter(|value| !value.trim().is_empty()) {
        if expected != actual {
            return Err("CLI 配置文件在预览后发生变化，请重新打开预览".to_string());
        }
    }
    Ok(())
}

fn match_provider(
    providers: &[Provider],
    cli_kind: LivenessCliKind,
    base_url: &str,
    api_key: &str,
) -> Option<String> {
    let expected_url = normalize_endpoint(base_url)?;
    providers
        .iter()
        .find(|provider| {
            let provider_url = match cli_kind {
                LivenessCliKind::Codex => openai_base_url(provider),
                LivenessCliKind::ClaudeCode => anthropic_base_url(provider),
            };
            normalize_endpoint(&provider_url).as_deref() == Some(expected_url.as_str())
                && normalize_api_key_for_protocol(
                    &provider.auth.api_key,
                    provider.identity.protocol,
                ) == normalize_api_key_for_protocol(api_key, provider.identity.protocol)
        })
        .map(|provider| provider.identity.id.clone())
}

fn normalize_endpoint(value: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(value.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });
    Some(url.as_str().trim_end_matches('/').to_string())
}

fn config_error(message: &str) -> CliConfigSnapshot {
    CliConfigSnapshot {
        error_message: Some(message.to_string()),
        ..CliConfigSnapshot::default()
    }
}
