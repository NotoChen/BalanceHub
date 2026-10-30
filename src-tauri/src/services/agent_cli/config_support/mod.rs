pub(crate) mod directories;
pub(crate) mod document_locations;
pub(crate) mod env_source;
pub(crate) mod file_layout;
pub(crate) mod file_writer;
pub(crate) mod json_source;
pub(crate) mod native_history;
pub(crate) mod text_document;

pub(crate) use env_source::{env_value, remove_env_keys, rewrite_env_values};
pub(crate) use json_source::{
    align_array as align_json_array, parse_jsonc_document, rewrite_json_document,
    rewrite_json_string_fields,
};

use crate::{
    limits,
    models::{
        is_full_api_key_value, normalize_api_key_for_protocol, AgentCliKind, CliConfigSnapshot,
        OAuthCredential, OAuthVendor, Provider, ProviderAuth, ProviderProtocol,
    },
    services::{agent_cli, oauth_accounts::native},
    util::read_text_file_limited,
};
use std::{fs, path::Path, time::SystemTime};

/// What a switch writes into the agent's own configuration: a relay address
/// and key, or the OAuth account's vendor authorization.
pub(crate) enum CliConfigTarget<'a> {
    ApiKey {
        base_url: String,
        api_key: String,
    },
    OAuth {
        auth: &'a ProviderAuth,
        identity: &'a OAuthCredential,
    },
}

/// How a switch names the OAuth credential it hands over: the account's email,
/// falling back to the vendor name.
pub(crate) fn oauth_credential_label(identity: &OAuthCredential) -> String {
    let label = if identity.email.trim().is_empty() {
        identity.vendor.label()
    } else {
        identity.email.trim()
    };
    label.chars().take(160).collect()
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

/// The credential a native switch hands to the agent. An OAuth card switches
/// the agent's own vendor login; a relay card switches its address and key.
pub(crate) fn cli_config_target<'a>(
    provider: &'a Provider,
    cli_kind: AgentCliKind,
    api_key_local_id: &str,
) -> Result<CliConfigTarget<'a>, String> {
    if provider.identity.protocol == ProviderProtocol::OAuth {
        let (auth, identity) = oauth_credential(provider, cli_kind)?;
        return Ok(CliConfigTarget::OAuth { auth, identity });
    }
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
    Ok(CliConfigTarget::ApiKey { base_url, api_key })
}

/// An OAuth card can only hand its credential to the vendor's own CLI, and
/// only when the stored authorization is usable for that CLI.
pub(crate) fn oauth_credential(
    provider: &Provider,
    cli_kind: AgentCliKind,
) -> Result<(&ProviderAuth, &OAuthCredential), String> {
    let identity = provider
        .auth
        .oauth
        .as_ref()
        .ok_or("OAuth 账号缺少授权信息，请重新授权")?;
    let vendor_cli = identity.vendor.agent_cli();
    if vendor_cli != cli_kind {
        return Err(format!(
            "{} 授权只能用于 {}",
            identity.vendor.label(),
            agent_cli::definition(vendor_cli).label
        ));
    }
    if let Some(reason) = native::unavailable_reason(identity.vendor, &provider.auth, identity) {
        return Err(reason.to_string());
    }
    Ok((&provider.auth, identity))
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

/// Recognizes a native authorization switch: the agent's own config holds the
/// OAuth account's access token, so that card is what the CLI runs with. The
/// match carries no key id — an OAuth card has no relay key.
pub(crate) fn match_native_oauth(
    providers: &[Provider],
    vendor: OAuthVendor,
    document: &str,
) -> Option<CliConfigProviderMatch> {
    let document = crate::services::agent_cli::environment::config_document::parse(
        document.as_bytes(),
        crate::services::agent_cli::environment::config_document::ConfigDocumentFormat::Json,
    )?;
    let tokens = native::stored_access_tokens(vendor, &document);
    providers.iter().find_map(|provider| {
        let identity = provider.auth.oauth.as_ref()?;
        if provider.identity.protocol != ProviderProtocol::OAuth || identity.vendor != vendor {
            return None;
        }
        let access_token = provider.auth.access_token.trim();
        if access_token.is_empty() || !tokens.contains(&access_token) {
            return None;
        }
        Some(CliConfigProviderMatch {
            provider_id: provider.identity.id.clone(),
            api_key_local_id: String::new(),
        })
    })
}

/// A relay route is active whether or not its card still exists; the card is
/// only what the snapshot points at.
pub(crate) fn relay_snapshot(
    cli_kind: AgentCliKind,
    matched: Option<CliConfigProviderMatch>,
    modified_at: Option<String>,
) -> CliConfigSnapshot {
    snapshot(cli_kind, true, matched, modified_at)
}

/// A vendor login counts as a BalanceHub configuration only when it is one of
/// the cards' authorizations; another installer's login is not this card's.
pub(crate) fn oauth_snapshot(
    cli_kind: AgentCliKind,
    matched: Option<CliConfigProviderMatch>,
    modified_at: Option<String>,
) -> CliConfigSnapshot {
    let configured = matched.is_some();
    snapshot(cli_kind, configured, matched, modified_at)
}

fn snapshot(
    cli_kind: AgentCliKind,
    configured: bool,
    matched: Option<CliConfigProviderMatch>,
    modified_at: Option<String>,
) -> CliConfigSnapshot {
    CliConfigSnapshot {
        cli_kind,
        configured,
        provider_id: matched.as_ref().map(|value| value.provider_id.clone()),
        api_key_local_id: matched.map(|value| value.api_key_local_id),
        modified_at,
        error_message: None,
    }
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
