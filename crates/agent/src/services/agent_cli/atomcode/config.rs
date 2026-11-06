use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, plain_secret, read_stable_optional, relay_snapshot, toml_table_member,
    CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::services::agent_cli::config_support::text_document;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use std::path::{Path, PathBuf};
use toml_edit::{value as toml_value, Document as TomlDocument};

const PROVIDER_ID: &str = "balancehub";
const DEFAULT_CONTEXT: i64 = 128_000;

fn model_key(model: &str) -> String {
    format!("{PROVIDER_ID}/{model}")
}

/// AtomCode keeps `config.toml` in `$ATOMCODE_HOME` (absolute), else
/// `~/.atomcode`.
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_path("ATOMCODE_HOME")
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| home.join(".atomcode"))
}

pub(super) fn config_path(root: &Path) -> PathBuf {
    root.join("config.toml")
}

fn config_file() -> Option<PathBuf> {
    Some(config_path(&config_dir_for_home(&user_home()?)))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(path) = config_file() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 AtomCode 配置失败"),
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
            ..config_error(cli_kind, "AtomCode 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let path = config_file().ok_or_else(|| "无法定位用户目录".to_string())?;
    let before = read_stable_optional(&path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 AtomCode".into());
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
        CliConfigTarget::OAuth { .. } => Err("AtomCode 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = text_document::body(text)
        .parse::<toml::Value>()
        .map_err(|_| ())?;
    let key = value
        .get("default_model")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            value
                .get("default_provider")
                .and_then(toml::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        });
    let Some(key) = key else {
        return Ok(None);
    };
    let entry = value
        .get("models")
        .and_then(|models| models.get(key))
        .and_then(toml::Value::as_table);
    if entry
        .and_then(|entry| entry.get("account"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        != Some(PROVIDER_ID)
    {
        return Ok(None);
    }
    let account = value
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(PROVIDER_ID));
    let base_url = account
        .and_then(|account| account.get("base_url"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let api_key = account
        .and_then(|account| account.get("api_key"))
        .and_then(toml::Value::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

fn merge_config(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let mut document = if source.trim().is_empty() {
        TomlDocument::new()
    } else {
        text_document::body(source)
            .parse::<TomlDocument>()
            .map_err(|_| "AtomCode 配置文件格式无效".to_string())?
    };
    let key = model_key(model);
    {
        let table = document.as_table_mut();
        table.insert("default_provider", toml_value(key.as_str()));
        table.insert("default_model", toml_value(key.as_str()));
    }
    {
        let accounts =
            toml_table_member(document.as_table_mut(), "provider_accounts", "AtomCode 配置")?;
        let entry = toml_table_member(accounts, PROVIDER_ID, "AtomCode 配置")?;
        entry.insert("provider", toml_value("openai-compatible"));
        entry.insert("base_url", toml_value(base_url.trim()));
        entry.insert("api_key", toml_value(api_key.trim()));
    }
    {
        let models = toml_table_member(document.as_table_mut(), "models", "AtomCode 配置")?;
        let entry = toml_table_member(models, &key, "AtomCode 配置")?;
        entry.insert("account", toml_value(PROVIDER_ID));
        entry.insert("model", toml_value(model));
        if !entry.contains_key("context_window") {
            entry.insert("context_window", toml_value(DEFAULT_CONTEXT));
        }
    }
    Ok(text_document::restore_layout(source, &document.to_string()))
}
