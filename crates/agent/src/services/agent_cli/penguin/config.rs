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
use toml_edit::{
    value as toml_value, ArrayOfTables, Document as TomlDocument, InlineTable, Item, Table, Value,
};

const PROVIDER_ID: &str = "balancehub";
const DEFAULT_CONTEXT: i64 = 128_000;
/// Penguin's Chat Completions client; the gateway protocol stays whatever the
/// relay serves instead of asking for the Responses API.
const CLIENT_TYPE: &str = "openai-chat";

/// PenguinHarness keeps one config per Project under `$PENGUIN_HOME`, else
/// `~/.penguin/data`, in the Project `$PENGUIN_PROJECT_ID` names or
/// `default_project`.
pub(super) fn data_dir_for_home(home: &Path) -> PathBuf {
    configured_path("PENGUIN_HOME").unwrap_or_else(|| home.join(".penguin/data"))
}

pub(super) fn project_dir_for_home(home: &Path) -> PathBuf {
    data_dir_for_home(home).join(project_name())
}

pub(super) fn config_path(root: &Path) -> PathBuf {
    root.join(".project_config.toml")
}

fn project_name() -> String {
    configured_path("PENGUIN_PROJECT_ID")
        .map(|value| value.to_string_lossy().trim().to_owned())
        .filter(|value| {
            !value.is_empty()
                && value != "."
                && value != ".."
                && !value.contains(['/', '\\'])
        })
        .unwrap_or_else(|| "default_project".into())
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = user_home() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let path = config_path(&project_dir_for_home(&home));
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 PenguinHarness 配置失败"),
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
            ..config_error(cli_kind, "PenguinHarness 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let path = config_path(&project_dir_for_home(&home));
    let before = read_stable_optional(&path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err(
                    "请先在中转站编辑窗口填写 CLI 首选模型，再切换到 PenguinHarness".into(),
                );
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
        CliConfigTarget::OAuth { .. } => Err("PenguinHarness 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = text_document::body(text)
        .parse::<toml::Value>()
        .map_err(|_| ())?;
    let Some(default) = value.get("default_model").and_then(toml::Value::as_table) else {
        return Ok(None);
    };
    let provider = default
        .get("provider")
        .and_then(toml::Value::as_str)
        .map(str::trim);
    let model_id = default
        .get("model_id")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (Some(provider), Some(model_id)) = (provider, model_id) else {
        return Ok(None);
    };
    if provider != PROVIDER_ID {
        return Ok(None);
    }
    let models = value
        .get("models")
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let entry = models.iter().find(|entry| {
        entry
            .get("provider")
            .and_then(toml::Value::as_str)
            .map(str::trim)
            == Some(PROVIDER_ID)
            && entry
                .get("model_id")
                .and_then(toml::Value::as_str)
                .map(str::trim)
                == Some(model_id)
    });
    let Some(entry) = entry else {
        return Ok(None);
    };
    let account = value
        .get("providers")
        .and_then(|providers| providers.get(PROVIDER_ID));
    // A model entry may carry its own endpoint and key; otherwise it uses the
    // provider table's, as Penguin resolves both.
    let base_url = setting(entry, account, "base_url");
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let api_key = setting(entry, account, "api_key").and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

fn setting<'a>(
    entry: &'a toml::Value,
    account: Option<&'a toml::Value>,
    key: &str,
) -> Option<&'a str> {
    entry
        .get(key)
        .and_then(toml::Value::as_str)
        .or_else(|| {
            account
                .and_then(|account| account.get(key))
                .and_then(toml::Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
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
            .map_err(|_| "PenguinHarness 配置文件格式无效".to_string())?
    };
    {
        let mut reference = InlineTable::new();
        reference.insert("provider", Value::from(PROVIDER_ID));
        reference.insert("model_id", Value::from(model));
        document
            .as_table_mut()
            .insert("default_model", Item::Value(Value::InlineTable(reference)));
    }
    {
        let providers =
            toml_table_member(document.as_table_mut(), "providers", "PenguinHarness 配置")?;
        let entry = toml_table_member(providers, PROVIDER_ID, "PenguinHarness 配置")?;
        entry.insert("base_url", toml_value(base_url.trim()));
        entry.insert("client_type", toml_value(CLIENT_TYPE));
        entry.insert("api_key", toml_value(api_key.trim()));
    }
    {
        let table = document.as_table_mut();
        if !table.contains_key("models") {
            table.insert("models", Item::ArrayOfTables(ArrayOfTables::new()));
        }
        let models = table
            .get_mut("models")
            .and_then(Item::as_array_of_tables_mut)
            .ok_or_else(|| "PenguinHarness 配置中的 models 必须是表数组".to_string())?;
        let entry = models.iter_mut().find(|entry| {
            entry.get("provider").and_then(Item::as_str) == Some(PROVIDER_ID)
                && entry.get("model_id").and_then(Item::as_str) == Some(model)
        });
        match entry {
            Some(entry) => {
                entry.insert("provider", toml_value(PROVIDER_ID));
                entry.insert("model_id", toml_value(model));
                if !entry.contains_key("display_name") {
                    entry.insert("display_name", toml_value(model));
                }
            }
            None => {
                let mut entry = Table::new();
                entry.insert("provider", toml_value(PROVIDER_ID));
                entry.insert("model_id", toml_value(model));
                entry.insert("display_name", toml_value(model));
                entry.insert("context_window", toml_value(DEFAULT_CONTEXT));
                models.push(entry);
            }
        }
    }
    Ok(text_document::restore_layout(source, &document.to_string()))
}
