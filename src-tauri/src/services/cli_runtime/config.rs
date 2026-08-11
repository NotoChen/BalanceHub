use super::{latest_modified_at, read_stable_optional};

mod formats;
mod json_source;
#[cfg(test)]
mod tests;
use crate::{
    limits,
    models::{
        normalize_api_key_for_protocol, CliConfigFile, CliConfigPreview, CliConfigSnapshot,
        LivenessCliKind, Provider,
    },
    services::cli_paths::{claude_config_dir, codex_home},
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
    let Some(codex_home) = codex_home() else {
        return config_error("无法定位用户目录");
    };
    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
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
    let Some(config_dir) = claude_config_dir() else {
        return config_error("无法定位用户目录");
    };
    let settings_path = config_dir.join("settings.json");
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
    let mut original_files = Vec::new();
    let mut files = Vec::new();

    let revision = match cli_kind {
        LivenessCliKind::Codex => {
            let codex_home = codex_home().ok_or_else(|| "无法定位用户目录".to_string())?;
            let config_path = codex_home.join("config.toml");
            let auth_path = codex_home.join("auth.json");
            let config_text = read_cli_config(&config_path, "读取 Codex 配置文件")?;
            let auth_text = read_cli_config(&auth_path, "读取 Codex 认证文件")?;
            let (next_config, next_auth) =
                rewrite_codex_config(&config_text, &auth_text, &base_url, &api_key)?;
            original_files.push(CliConfigFile {
                file_path: config_path.to_string_lossy().into_owned(),
                content: config_text.clone(),
            });
            original_files.push(CliConfigFile {
                file_path: auth_path.to_string_lossy().into_owned(),
                content: auth_text.clone(),
            });
            files.push(CliConfigFile {
                file_path: config_path.to_string_lossy().into_owned(),
                content: next_config,
            });
            files.push(CliConfigFile {
                file_path: auth_path.to_string_lossy().into_owned(),
                content: next_auth,
            });
            config_revision(&[&config_text, &auth_text, &base_url, &api_key])
        }
        LivenessCliKind::ClaudeCode => {
            let config_dir = claude_config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
            let settings_path = config_dir.join("settings.json");
            let settings_text = read_cli_config(&settings_path, "读取 Claude Code 配置文件")?;
            let next_settings = rewrite_claude_config(&settings_text, &base_url, &api_key)?;
            original_files.push(CliConfigFile {
                file_path: settings_path.to_string_lossy().into_owned(),
                content: settings_text.clone(),
            });
            files.push(CliConfigFile {
                file_path: settings_path.to_string_lossy().into_owned(),
                content: next_settings,
            });
            config_revision(&[&settings_text, &base_url, &api_key])
        }
    };

    Ok(CliConfigPreview {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.identity.name.clone(),
        cli_kind,
        revision,
        original_files,
        files,
    })
}

pub fn switch_config(
    provider: &Provider,
    cli_kind: LivenessCliKind,
    expected_revision: Option<&str>,
    files: &[CliConfigFile],
) -> Result<(), String> {
    let (base_url, api_key) = cli_target(provider, cli_kind)?;
    match cli_kind {
        LivenessCliKind::Codex => {
            switch_codex_config(&base_url, &api_key, expected_revision, files)
        }
        LivenessCliKind::ClaudeCode => {
            switch_claude_config(&base_url, &api_key, expected_revision, files)
        }
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
    files: &[CliConfigFile],
) -> Result<(), String> {
    let codex_home = codex_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
    let config_text = read_cli_config(&config_path, "读取 Codex 配置文件")?;
    let auth_text = read_cli_config(&auth_path, "读取 Codex 认证文件")?;
    validate_file_set(files, [&config_path, &auth_path])?;
    ensure_revision(
        expected_revision,
        config_revision(&[&config_text, &auth_text, base_url, api_key]),
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

fn switch_claude_config(
    base_url: &str,
    api_key: &str,
    expected_revision: Option<&str>,
    files: &[CliConfigFile],
) -> Result<(), String> {
    let config_dir = claude_config_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = config_dir.join("settings.json");
    let settings_text = read_cli_config(&settings_path, "读取 Claude Code 配置文件")?;
    validate_file_set(files, [&settings_path])?;
    ensure_revision(
        expected_revision,
        config_revision(&[&settings_text, base_url, api_key]),
    )?;
    let edited_settings = file_content(files, &settings_path)?;
    serde_json::from_str::<JsonValue>(edited_settings)
        .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
    write_config_text(&settings_path, edited_settings, "Claude Code 配置")
}

fn read_cli_config(path: &std::path::Path, context: &str) -> Result<String, String> {
    read_text_file_limited(path, limits::MAX_CLI_CONFIG_FILE_BYTES, context)
}

fn file_content<'a>(files: &'a [CliConfigFile], path: &std::path::Path) -> Result<&'a str, String> {
    let expected = path.to_string_lossy();
    let matches = files
        .iter()
        .filter(|file| file.file_path == expected)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!("缺少或重复提交 CLI 配置文件：{}", path.display()));
    }
    let content = matches[0].content.as_str();
    if content.len() > limits::MAX_CLI_CONFIG_FILE_BYTES {
        return Err(format!("CLI 配置文件过大：{}", path.display()));
    }
    Ok(content)
}

fn validate_file_set<const N: usize>(
    files: &[CliConfigFile],
    expected_paths: [&std::path::Path; N],
) -> Result<(), String> {
    if files.len() != N
        || files.iter().any(|file| {
            !expected_paths
                .iter()
                .any(|path| file.file_path == path.to_string_lossy())
        })
    {
        return Err("只能提交当前 CLI 预览中的配置文件".to_string());
    }
    Ok(())
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
