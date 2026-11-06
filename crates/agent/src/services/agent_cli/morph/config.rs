use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, plain_secret, read_stable_optional, relay_snapshot, yaml_source, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::{expand_tilde, validate_native_value};
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

const INFERENCE_PROVIDER: &str = "openai_response_compatible";
const USER_AGENT: &str = "mistermorph";

/// Mister Morph reads `$MISTER_MORPH_CONFIG`, else `~/.morph/config.yaml`.
pub(super) fn config_path_for_home(home: &Path) -> PathBuf {
    configured_path("MISTER_MORPH_CONFIG")
        .map(|path| expand_tilde(&path.to_string_lossy(), home))
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".morph/config.yaml"))
}

pub(super) fn config_path() -> Option<PathBuf> {
    user_home().map(|home| config_path_for_home(&home))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(path) = config_path() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Mister Morph 配置失败"),
    };
    let modified_at = latest_modified_at([file.as_ref()]);
    let Some(file) = file else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&file.text) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Mister Morph 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let path = config_path().ok_or_else(|| "无法定位用户目录".to_string())?;
    let before = read_stable_optional(&path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Mister Morph".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let after = merge_config(
                before.as_deref().unwrap_or_default(),
                model,
                &base_url,
                &api_key,
            )?;
            Ok(vec![ProviderConfigurationCandidate {
                path,
                before,
                after,
            }])
        }
        CliConfigTarget::OAuth { .. } => Err("Mister Morph 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let root = yaml_source::value(text).map_err(|_| ())?;
    let provider = root
        .pointer("/llm/inference_provider")
        .and_then(JsonValue::as_str)
        .map(str::trim);
    let model = root
        .pointer("/llm/model")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if provider != Some(INFERENCE_PROVIDER) || model.is_none() {
        return Ok(None);
    }
    let endpoint = root
        .pointer("/llm/endpoint")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };
    if normalize_endpoint(endpoint).is_none() {
        return Err(());
    }
    let api_key = root
        .pointer("/llm/api_key")
        .and_then(JsonValue::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (endpoint.to_string(), api_key)))
}

fn merge_config(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let endpoint = super::super::config_support::openai_base_url(base_url);
    yaml_source::edit(source, "Mister Morph 配置", |root| {
        let llm = yaml_source::mapping_member(root, "llm", "Mister Morph 配置")?;
        llm.set("inference_provider", INFERENCE_PROVIDER);
        llm.set("endpoint", endpoint.as_str());
        llm.set("api_key", api_key);
        llm.set("model", model);
        let headers = yaml_source::mapping_member(&llm, "headers", "Mister Morph 配置")?;
        headers.set("User-Agent", USER_AGENT);
        Ok(())
    })
}
