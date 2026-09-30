pub(crate) mod directories;
pub(crate) mod env_source;
pub(crate) mod json_source;

pub(crate) use env_source::{env_value, rewrite_env_values};
pub(crate) use json_source::{
    align_array as align_json_array, parse_jsonc_document, rewrite_json_document,
    rewrite_json_string_fields,
};

use crate::{
    limits,
    models::{
        is_full_api_key_value, normalize_api_key_for_protocol, AgentCliKind, CliConfigSnapshot,
        Provider, ProviderApiKeyOption,
    },
    services::agent_cli,
    util::read_text_file_limited,
};
use std::{fs, path::Path, time::SystemTime};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CliConfigTarget {
    pub base_url: String,
    pub api_key: String,
    pub api_key_local_id: String,
    pub api_key_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CliConfigProviderMatch {
    pub provider_id: String,
    pub api_key_local_id: String,
}

#[derive(Clone, PartialEq, Eq)]
struct FileSignature {
    len: u64,
    modified: Option<SystemTime>,
}

pub(crate) struct StableFile {
    pub text: String,
    pub modified_at: Option<u128>,
}

pub(crate) fn read_stable_optional(path: &Path) -> Result<Option<StableFile>, String> {
    for _ in 0..2 {
        let before = match file_signature(path) {
            Ok(Some(signature)) => signature,
            Ok(None) => return Ok(None),
            Err(err) => return Err(err),
        };
        let text =
            read_text_file_limited(path, limits::MAX_CLI_CONFIG_FILE_BYTES, "读取 CLI 配置文件")?;
        let after = match file_signature(path)? {
            Some(signature) => signature,
            None => continue,
        };
        if before == after {
            return Ok(Some(StableFile {
                text,
                modified_at: after.modified.and_then(system_time_millis),
            }));
        }
    }
    Err(format!("文件读取期间发生变化({})", path.display()))
}

pub(crate) fn latest_modified_at<'a>(
    files: impl IntoIterator<Item = Option<&'a StableFile>>,
) -> Option<String> {
    files
        .into_iter()
        .flatten()
        .filter_map(|file| file.modified_at)
        .max()
        .map(|value| value.to_string())
}

pub(crate) fn read_cli_config(path: &Path, context: &str) -> Result<String, String> {
    read_text_file_limited(path, limits::MAX_CLI_CONFIG_FILE_BYTES, context)
}

pub(crate) fn cli_target_for_key(
    provider: &Provider,
    cli_kind: AgentCliKind,
    api_key_local_id: &str,
) -> Result<CliConfigTarget, String> {
    let requested_local_id = api_key_local_id.trim();
    let selected_option = if requested_local_id.is_empty() {
        provider.auth.api_key_options.iter().find(|option| {
            normalize_api_key_for_protocol(&option.key, provider.identity.protocol)
                == normalize_api_key_for_protocol(
                    &provider.auth.api_key,
                    provider.identity.protocol,
                )
        })
    } else {
        Some(
            provider
                .auth
                .api_key_options
                .iter()
                .find(|option| option.local_id == requested_local_id)
                .ok_or_else(|| "所选 API Key 已不存在，请重新选择".to_string())?,
        )
    };
    let raw_api_key = selected_option
        .map(|option| option.key.as_str())
        .unwrap_or(provider.auth.api_key.as_str());
    let api_key = normalize_api_key_for_protocol(raw_api_key, provider.identity.protocol);
    if !is_full_api_key_value(&api_key) {
        return Err(if requested_local_id.is_empty() {
            "中转站缺少完整 API Key，无法切换 CLI 配置".to_string()
        } else {
            "所选 API Key 未读取到完整值，无法切换 CLI 配置".to_string()
        });
    }
    let base_url = agent_cli::provider_base_url(cli_kind, provider);
    if normalize_endpoint(&base_url).is_none() {
        return Err("中转站地址无效，无法切换 CLI 配置".to_string());
    }
    Ok(CliConfigTarget {
        base_url,
        api_key,
        api_key_local_id: selected_option
            .map(|option| option.local_id.clone())
            .unwrap_or_default(),
        api_key_label: selected_option
            .map(api_key_label)
            .unwrap_or_else(|| "当前配置 API Key".to_string()),
    })
}

pub(crate) fn match_provider_key(
    providers: &[Provider],
    cli_kind: AgentCliKind,
    base_url: &str,
    api_key: &str,
) -> Option<CliConfigProviderMatch> {
    let expected_url = normalize_endpoint(base_url)?;
    for provider in providers {
        let provider_url = agent_cli::provider_base_url(cli_kind, provider);
        if normalize_endpoint(&provider_url).as_deref() != Some(expected_url.as_str()) {
            continue;
        }
        let expected_key = normalize_api_key_for_protocol(api_key, provider.identity.protocol);
        if let Some(option) = provider.auth.api_key_options.iter().find(|option| {
            normalize_api_key_for_protocol(&option.key, provider.identity.protocol) == expected_key
        }) {
            return Some(CliConfigProviderMatch {
                provider_id: provider.identity.id.clone(),
                api_key_local_id: option.local_id.clone(),
            });
        }
        if normalize_api_key_for_protocol(&provider.auth.api_key, provider.identity.protocol)
            == expected_key
        {
            return Some(CliConfigProviderMatch {
                provider_id: provider.identity.id.clone(),
                api_key_local_id: String::new(),
            });
        }
    }
    None
}

fn api_key_label(option: &ProviderApiKeyOption) -> String {
    let label = if !option.local_name.trim().is_empty() {
        option.local_name.trim()
    } else if !option.name.trim().is_empty()
        && option.name.trim() != "当前 API Key"
        && option.name.trim() != "当前配置 API Key"
    {
        option.name.trim()
    } else {
        "未命名 API Key"
    };
    label.chars().take(160).collect()
}

pub(crate) fn normalize_endpoint(value: &str) -> Option<String> {
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

pub(crate) fn config_error(cli_kind: AgentCliKind, message: &str) -> CliConfigSnapshot {
    CliConfigSnapshot {
        cli_kind,
        configured: false,
        provider_id: None,
        api_key_local_id: None,
        modified_at: None,
        error_message: Some(message.to_string()),
    }
}

fn file_signature(path: &Path) -> Result<Option<FileSignature>, String> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(FileSignature {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("读取文件元数据失败({}): {err}", path.display())),
    }
}

fn system_time_millis(value: SystemTime) -> Option<u128> {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}
